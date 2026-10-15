//! Open Core Router (OpenCR) — asl-router-drivers-virtio
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! VirtIO-Net Legacy (0.9.5) PCI Driver — real split-ring virtqueue DMA.
//! Layout region dihitung runtime dari q device (VBox: fixed 1024).
//! + Intel e1000 fallback inventory struct.

#![no_std]

use asl_router_pci::PciDevice;

// VirtIO PCI Status bits
pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
pub const VIRTIO_STATUS_FAILED: u8 = 128;

// VirtIO PCI Common cfg register offsets (relative I/O BAR)
pub const VIRTIO_PCI_HOST_FEATURES: u16 = 0;
pub const VIRTIO_PCI_GUEST_FEATURES: u16 = 4;
pub const VIRTIO_PCI_QUEUE_PFN: u16 = 8;
pub const VIRTIO_PCI_QUEUE_NUM: u16 = 12;
pub const VIRTIO_PCI_QUEUE_SEL: u16 = 14;
pub const VIRTIO_PCI_QUEUE_NOTIFY: u16 = 16;
pub const VIRTIO_PCI_STATUS: u16 = 18;
pub const VIRTIO_PCI_ISR: u16 = 19;
pub const VIRTIO_PCI_CONFIG_MAC: u16 = 20;

// Split-ring constants (virtio 0.9.5 legacy)
pub const DEFAULT_QSZ: u16 = 64; // dipakai hanya bila device tidak jawab (host test)
pub const MAXQ: usize = 1024; // kapasitas maksimum region driver
pub const QUEUE_BYTES: usize = 32768; // desc16K+avail2K(→pad20480)+used8.2K ≤ 32768
pub const NBUF: usize = 8; // buffer chains per queue (hdr+packet pair)
pub const PKTSZ: usize = 1536;
/// Legacy (0.9.5) virtio-net hdr: flags gso_type hdr_len gso_size csum_start csum_offset
pub const VIRTIO_NET_HDR_LEN: usize = 10;
pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;

// -----------------------------------------------------------------------------
// PORT I/O — hanya aktif pada bare-metal x86_64 (target_os = "none")
// -----------------------------------------------------------------------------

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let ret: u8;
        core::arch::asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let _ = port;
        0
    }
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = (port, val);
}

#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let ret: u16;
        core::arch::asm!("in ax, dx", out("ax") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let _ = port;
        0
    }
}

#[inline(always)]
unsafe fn outw(port: u16, val: u16) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = (port, val);
}

#[inline(always)]
unsafe fn ind(port: u16) -> u32 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let ret: u32;
        core::arch::asm!("in eax, dx", out("eax") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let _ = port;
        0
    }
}

#[inline(always)]
unsafe fn outd(port: u16, val: u32) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    core::arch::asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = (port, val);
}

// -----------------------------------------------------------------------------
// SPLIT VIRTQUEUE REGION (LEGACY 0.9.5) — layout per spec utk queue size q:
//   desc      : offset 0        , 16*q
//   avail     : offset 16*q     , 6 + 2*q  (flags,idx,ring[q],used_event)
//   used      : align_up(16q+6+2q, 4096), 6 + 8*q + 2 (flags,idx,elems[q],avail_event)
// Device menurunkan offset dari PFN + ukuran queue-nya sendiri (VBox fixed 1024),
// jadi pointer dihitung RUNTIME dari self.qsz, bukan konversi statis.
// -----------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct UsedElem {
    pub id: u32,
    pub len: u32,
}

pub const fn avail_off(q: usize) -> usize {
    16 * q
}

pub const fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

pub const fn used_off(q: usize) -> usize {
    align_up(16 * q + 6 + 2 * q, 4096)
}

pub const fn region_bytes_required(q: usize) -> usize {
    used_off(q) + 6 + 8 * q + 2
}

const _: () = assert!(region_bytes_required(MAXQ) <= QUEUE_BYTES);

#[repr(C, align(4096))]
pub struct QueueRegion {
    pub mem: [u8; QUEUE_BYTES],
}

impl QueueRegion {
    const fn zeroed() -> Self {
        Self {
            mem: [0u8; QUEUE_BYTES],
        }
    }

    #[inline(always)]
    fn base(&self) -> *mut u8 {
        self.mem.as_ptr() as *mut u8
    }

    #[inline(always)]
    fn desc_ptr(&self, i: usize) -> *mut VirtqDesc {
        unsafe { self.base().add(i * core::mem::size_of::<VirtqDesc>()) as *mut VirtqDesc }
    }

    #[inline(always)]
    fn avail_idx_ptr(&self, q: usize) -> *mut u16 {
        unsafe { self.base().add(avail_off(q) + 2) as *mut u16 }
    }

    #[inline(always)]
    fn avail_ring_ptr(&self, q: usize, slot: usize) -> *mut u16 {
        unsafe { self.base().add(avail_off(q) + 4 + slot * 2) as *mut u16 }
    }

    #[inline(always)]
    fn used_idx_ptr(&self, q: usize) -> *mut u16 {
        unsafe { self.base().add(used_off(q) + 2) as *mut u16 }
    }

    #[inline(always)]
    fn used_elem_ptr(&self, q: usize, slot: usize) -> *mut UsedElem {
        unsafe {
            self.base()
                .add(used_off(q) + 4 + slot * core::mem::size_of::<UsedElem>())
                as *mut UsedElem
        }
    }

    #[inline(always)]
    fn read_avail_idx(&self, q: usize) -> u16 {
        unsafe { core::ptr::read_volatile(self.avail_idx_ptr(q)) }
    }

    #[inline(always)]
    fn write_avail_idx(&self, q: usize, v: u16) {
        unsafe { core::ptr::write_volatile(self.avail_idx_ptr(q), v) }
    }

    #[inline(always)]
    fn read_used_idx(&self, q: usize) -> u16 {
        unsafe { core::ptr::read_volatile(self.used_idx_ptr(q)) }
    }

    #[inline(always)]
    fn write_avail_slot(&self, q: usize, slot: usize, id: u16) {
        unsafe { core::ptr::write(self.avail_ring_ptr(q, slot), id) }
    }

    #[inline(always)]
    fn read_used_elem(&self, q: usize, slot: usize) -> UsedElem {
        unsafe { core::ptr::read_volatile(self.used_elem_ptr(q, slot)) }
    }

    #[inline(always)]
    fn write_desc(&self, i: usize, d: VirtqDesc) {
        unsafe { core::ptr::write(self.desc_ptr(i), d) }
    }

    #[inline(always)]
    fn read_desc(&self, i: usize) -> VirtqDesc {
        unsafe { core::ptr::read(self.desc_ptr(i)) }
    }

    #[inline(always)]
    fn pa_of(&self, hhdm: u64) -> u64 {
        (self.base() as u64).wrapping_sub(hhdm)
    }
}

#[inline(always)]
fn pa_of(ptr: *const u8, hhdm: u64) -> u64 {
    (ptr as u64).wrapping_sub(hhdm)
}

// -----------------------------------------------------------------------------
// VIRTIO-NET DRIVER
// -----------------------------------------------------------------------------

pub struct VirtioNetDevice {
    pub io_base: u16,
    pub mac_address: [u8; 6],
    pub hhdm: u64,
    pub qsz: usize,
    rxq: QueueRegion,
    txq: QueueRegion,
    rx_hdr: [[u8; VIRTIO_NET_HDR_LEN]; NBUF],
    rx_buf: [[u8; PKTSZ]; NBUF],
    tx_hdr: [[u8; VIRTIO_NET_HDR_LEN]; NBUF],
    tx_buf: [[u8; PKTSZ]; NBUF],
    rx_used_next: u16,
    tx_used_next: u16,
    tx_rr: usize,
    tx_inflight: [bool; NBUF],
    pub ready: bool,
}

impl VirtioNetDevice {
    /// Instance kosong — WAJIB di-probe pada lokasi final (probe(&mut dev,..));
    /// alamat fisik ring diturunkan dari alamat struct, jadi device tidak boleh
    /// di-move setelah probe sukses.
    pub const fn empty() -> Self {
        Self {
            io_base: 0,
            mac_address: [0; 6],
            hhdm: 0,
            qsz: DEFAULT_QSZ as usize,
            rxq: QueueRegion::zeroed(),
            txq: QueueRegion::zeroed(),
            rx_hdr: [[0u8; VIRTIO_NET_HDR_LEN]; NBUF],
            rx_buf: [[0u8; PKTSZ]; NBUF],
            tx_hdr: [[0u8; VIRTIO_NET_HDR_LEN]; NBUF],
            tx_buf: [[0u8; PKTSZ]; NBUF],
            rx_used_next: 0,
            tx_used_next: 0,
            tx_rr: 0,
            tx_inflight: [false; NBUF],
            ready: false,
        }
    }

    /// Inisialisasi legacy virtio-net penuh: bus-master PCI, handshake status,
    /// guest features = 0, baca ukuran queue device (fixed di VBox), pasang
    /// PFN RX(0)/TX(1), DRIVER_OK, lalu prime NBUF rantai RX.
    pub fn probe(
        &mut self,
        pci: &PciDevice,
        hhdm: u64,
        log: &dyn Fn(&str),
    ) -> Result<(), &'static str> {
        let mut io_port = 0u16;
        for bar in pci.bars {
            if let asl_router_pci::PciBar::Io { port, .. } = bar {
                if port != 0 {
                    io_port = port;
                    break;
                }
            }
        }
        if io_port == 0 {
            return Err("No I/O BAR found on VirtIO device");
        }
        self.io_base = io_port;
        self.hhdm = hhdm;

        asl_router_pci::pci_enable_bus_master(pci.bus, pci.device, pci.function);

        unsafe {
            outb(io_port + VIRTIO_PCI_STATUS, 0);
            outb(io_port + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE);
            outb(
                io_port + VIRTIO_PCI_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER,
            );

            let host_features = ind(io_port + VIRTIO_PCI_HOST_FEATURES);
            let _ = host_features; // negosiasi nihil: guest features = 0
            outd(io_port + VIRTIO_PCI_GUEST_FEATURES, 0);
        }

        let mut mac = [0u8; 6];
        unsafe {
            for i in 0..6 {
                mac[i] = inb(io_port + VIRTIO_PCI_CONFIG_MAC + (i as u16));
            }
        }
        self.mac_address = mac;

        // Baca ukuran queue asli device (QUEUE_NUM legacy read-only di VBox).
        let (q0, q1) = unsafe {
            outw(io_port + VIRTIO_PCI_QUEUE_SEL, 0);
            let q0 = inw(io_port + VIRTIO_PCI_QUEUE_NUM);
            outw(io_port + VIRTIO_PCI_QUEUE_SEL, 1);
            let q1 = inw(io_port + VIRTIO_PCI_QUEUE_NUM);
            (q0, q1)
        };
        log("[R24] queue size device q0=");
        log_dec(q0 as u32, log);
        log(" q1=");
        log_dec(q1 as u32, log);
        log("\n");
        // Device yang tidak menjawab (mis. stub host-test) → pakai DEFAULT_QSZ.
        let q_eff = if q0 != q1 {
            return Err("queue size tidak konsisten antar queue");
        } else if q0 == 0 {
            DEFAULT_QSZ as usize
        } else {
            q0 as usize
        };
        if q_eff > MAXQ || NBUF > q_eff {
            return Err("device queue size di luar kapasitas region driver");
        }
        self.qsz = q_eff;

        let rx_pa_full = self.rxq.pa_of(hhdm);
        let tx_pa_full = self.txq.pa_of(hhdm);
        let rx_pfn = (rx_pa_full >> 12) as u32;
        let tx_pfn = (tx_pa_full >> 12) as u32;
        log("[R24] hhdm=0x");
        log_hex64(hhdm, log);
        log(" rx_va=0x");
        log_hex64(self.rxq.base() as u64, log);
        log(" rx_pa=0x");
        log_hex64(rx_pa_full, log);
        log(" rx_pfn=0x");
        log_hex64(rx_pfn as u64, log);
        log(" tx_pfn=0x");
        log_hex64(tx_pfn as u64, log);
        log("\n");
        unsafe {
            outw(io_port + VIRTIO_PCI_QUEUE_SEL, 0);
            outd(io_port + VIRTIO_PCI_QUEUE_PFN, rx_pfn);
            outw(io_port + VIRTIO_PCI_QUEUE_SEL, 1);
            outd(io_port + VIRTIO_PCI_QUEUE_PFN, tx_pfn);
            outb(
                io_port + VIRTIO_PCI_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK,
            );
        }

        self.prime_rx();
        self.ready = true;
        Ok(())
    }

    fn prime_rx(&mut self) {
        let q = self.qsz;
        for i in 0..NBUF {
            let hdr_pa = pa_of(self.rx_hdr[i].as_ptr(), self.hhdm);
            let buf_pa = pa_of(self.rx_buf[i].as_ptr(), self.hhdm);
            self.rxq.write_desc(
                2 * i,
                VirtqDesc {
                    addr: hdr_pa,
                    len: VIRTIO_NET_HDR_LEN as u32,
                    flags: VRING_DESC_F_WRITE | VRING_DESC_F_NEXT,
                    next: (2 * i + 1) as u16,
                },
            );
            self.rxq.write_desc(
                2 * i + 1,
                VirtqDesc {
                    addr: buf_pa,
                    len: PKTSZ as u32,
                    flags: VRING_DESC_F_WRITE,
                    next: 0,
                },
            );
            self.rxq.write_avail_slot(q, i, (2 * i) as u16);
        }
        self.rxq.write_avail_idx(q, NBUF as u16);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        unsafe {
            outw(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY, 0);
        }
    }

    /// Ambil satu frame Ethernet lengkap dari RX used ring. Some(len) = panjang
    /// frame (tanpa virtio-net header). Mengisi ulang avail slot setelah copy.
    pub fn poll_rx(&mut self, out: &mut [u8]) -> Option<usize> {
        if !self.ready {
            return None;
        }
        let q = self.qsz;
        let used_idx = self.rxq.read_used_idx(q);
        if used_idx == self.rx_used_next {
            return None;
        }
        let slot_u = (self.rx_used_next as usize) % q;
        let elem = self.rxq.read_used_elem(q, slot_u);
        self.rx_used_next = self.rx_used_next.wrapping_add(1);

        let chain = (elem.id as usize / 2) % NBUF;
        let frame_len = (elem.len as usize).saturating_sub(VIRTIO_NET_HDR_LEN);
        let n = if frame_len <= out.len() {
            frame_len
        } else {
            out.len()
        };
        out[..n].copy_from_slice(&self.rx_buf[chain][..n]);

        // Requeue rantai yang sudah dikonsumsi.
        let avail_ctr = self.rxq.read_avail_idx(q);
        self.rxq
            .write_avail_slot(q, (avail_ctr as usize) % q, elem.id as u16);
        self.rxq.write_avail_idx(q, avail_ctr.wrapping_add(1));
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        unsafe {
            outw(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY, 0);
        }
        if n > 0 {
            Some(n)
        } else {
            None
        }
    }

    /// Reap completion dari TX used ring.
    fn reap_tx(&mut self) {
        let q = self.qsz;
        let used_idx = self.txq.read_used_idx(q);
        while self.tx_used_next != used_idx {
            let slot_u = (self.tx_used_next as usize) % q;
            let elem = self.txq.read_used_elem(q, slot_u);
            let chain = (elem.id as usize / 2) % NBUF;
            self.tx_inflight[chain] = false;
            self.tx_used_next = self.tx_used_next.wrapping_add(1);
        }
    }

    /// Kirim satu frame Ethernet: salin ke buffer TX, rantai [hdr][pkt],
    /// push avail queue1, notify.
    pub fn transmit(&mut self, packet: &[u8]) -> Result<(), &'static str> {
        if !self.ready {
            return Err("VirtIO net not probed");
        }
        if packet.is_empty() || packet.len() > PKTSZ {
            return Err("Frame out of TX buffer size");
        }
        let q = self.qsz;
        self.reap_tx();
        let mut slot = self.tx_rr % NBUF;
        let mut tries = 0;
        while self.tx_inflight[slot] && tries < NBUF {
            self.tx_rr += 1;
            slot = self.tx_rr % NBUF;
            tries += 1;
        }
        if self.tx_inflight[slot] {
            return Err("TX ring full");
        }
        self.tx_rr += 1;
        self.tx_buf[slot][..packet.len()].copy_from_slice(packet);
        for b in self.tx_hdr[slot].iter_mut() {
            *b = 0; // hdr legacy nol: tanpa offload csum/gso
        }

        let hdr_pa = pa_of(self.tx_hdr[slot].as_ptr(), self.hhdm);
        let buf_pa = pa_of(self.tx_buf[slot].as_ptr(), self.hhdm);
        self.txq.write_desc(
            2 * slot,
            VirtqDesc {
                addr: hdr_pa,
                len: VIRTIO_NET_HDR_LEN as u32,
                flags: VRING_DESC_F_NEXT,
                next: (2 * slot + 1) as u16,
            },
        );
        self.txq.write_desc(
            2 * slot + 1,
            VirtqDesc {
                addr: buf_pa,
                len: packet.len() as u32,
                flags: 0,
                next: 0,
            },
        );
        let avail_ctr = self.txq.read_avail_idx(q);
        self.txq
            .write_avail_slot(q, (avail_ctr as usize) % q, (2 * slot) as u16);
        self.txq.write_avail_idx(q, avail_ctr.wrapping_add(1));
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        unsafe {
            outw(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY, 1);
        }
        self.tx_inflight[slot] = true;
        Ok(())
    }

    pub fn is_tx_path_armed(&self) -> bool {
        self.ready && self.io_base != 0
    }

    pub fn tx_pending_count(&self) -> usize {
        self.tx_inflight.iter().filter(|b| **b).count()
    }
}

// -----------------------------------------------------------------------------
// LOG HELPERS
// -----------------------------------------------------------------------------

fn log_dec(v: u32, log: &dyn Fn(&str)) {
    if v == 0 {
        log("0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let mut out = [0u8; 10];
    let mut j = 0;
    while j < i {
        out[j] = buf[i - 1 - j];
        j += 1;
    }
    log(core::str::from_utf8(&out[..i]).unwrap_or("?"));
}

fn log_hex64(v: u64, log: &dyn Fn(&str)) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        buf[15 - i] = HEX[((v >> (4 * i)) & 0xF) as usize];
        i += 1;
    }
    log(core::str::from_utf8(&buf).unwrap_or("????????????????"));
}

// -----------------------------------------------------------------------------
// INTEL 82540EM (E1000) FALLBACK DRIVER
// -----------------------------------------------------------------------------

pub struct E1000Device {
    pub io_base: u16,
    pub mem_base: u32,
    pub mac_address: [u8; 6],
}

impl E1000Device {
    pub fn new(pci: &PciDevice) -> Result<Self, &'static str> {
        let mut io_port = 0u16;
        let mut mem_addr = 0u32;
        for bar in pci.bars {
            match bar {
                asl_router_pci::PciBar::Io { port, .. } => io_port = port,
                asl_router_pci::PciBar::Memory { address, .. } => mem_addr = address,
                _ => {}
            }
        }

        // Default VirtualBox MAC format jika eeprom read bypassed
        let mac = [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33];

        Ok(Self {
            io_base: io_port,
            mem_base: mem_addr,
            mac_address: mac,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asl_router_pci::PciBar;

    #[test]
    fn test_virtio_status_constants() {
        assert_eq!(VIRTIO_STATUS_ACKNOWLEDGE, 1);
        assert_eq!(VIRTIO_STATUS_DRIVER, 2);
        assert_eq!(VIRTIO_STATUS_DRIVER_OK, 4);
    }

    #[test]
    fn test_legacy_layout_formulas() {
        assert_eq!(avail_off(64), 1024);
        assert_eq!(used_off(64), 4096);
        assert_eq!(avail_off(1024), 16384);
        assert_eq!(used_off(1024), 20480);
        assert_eq!(region_bytes_required(1024), 20480 + 6 + 8 * 1024 + 2);
        assert_eq!(core::mem::size_of::<VirtqDesc>(), 16);
        assert_eq!(core::mem::size_of::<UsedElem>(), 8);
    }

    #[test]
    fn test_e1000_device_creation() {
        let pci = PciDevice {
            bus: 0,
            device: 3,
            function: 0,
            vendor_id: 0x8086,
            device_id: 0x100E,
            class_code: 2,
            subclass: 0,
            prog_if: 0,
            header_type: 0,
            bars: [
                PciBar::Memory {
                    address: 0xF0000000,
                    size: 0x20000,
                    prefetchable: false,
                },
                PciBar::Io {
                    port: 0xD000,
                    size: 64,
                },
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
            ],
        };
        let e1000 = E1000Device::new(&pci).unwrap();
        assert_eq!(e1000.io_base, 0xD000);
        assert_eq!(e1000.mem_base, 0xF0000000);
    }

    fn mock_pci() -> PciDevice {
        PciDevice {
            bus: 0,
            device: 8,
            function: 0,
            vendor_id: 0x1AF4,
            device_id: 0x1000,
            class_code: 0x02,
            subclass: 0x00,
            prog_if: 0,
            header_type: 0,
            bars: [
                PciBar::Io {
                    port: 0xC000,
                    size: 64,
                },
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
            ],
        }
    }

    #[test]
    fn test_virtio_probe_and_tx_chain_host_side() {
        // Pada host, in/out adalah no-op (q device 0 -> DEFAULT_QSZ) dan hhdm=0.
        let mut dev = VirtioNetDevice::empty();
        assert!(!dev.is_tx_path_armed());
        assert!(dev.probe(&mock_pci(), 0, &|_s| {}).is_ok());
        assert_eq!(dev.qsz, DEFAULT_QSZ as usize);
        assert!(dev.ready);
        assert_eq!(dev.tx_pending_count(), 0);

        let frame = [0xABu8; 64];
        assert!(dev.transmit(&frame).is_ok());
        assert_eq!(dev.tx_pending_count(), 1);
        let d0 = unsafe { *dev.txq.desc_ptr(0) };
        assert_eq!(d0.flags, VRING_DESC_F_NEXT);
        assert_eq!(d0.len as usize, VIRTIO_NET_HDR_LEN);
        let d1 = unsafe { *dev.txq.desc_ptr(1) };
        assert_eq!(d1.len as usize, 64);
        assert_eq!(d1.flags, 0);
        assert_eq!(dev.txq.read_avail_idx(dev.qsz), 1);
    }

    #[test]
    fn test_rx_desc_chain_primed() {
        let mut dev = VirtioNetDevice::empty();
        assert!(dev.probe(&mock_pci(), 0, &|_s| {}).is_ok());
        assert_eq!(dev.rxq.read_avail_idx(dev.qsz), NBUF as u16);
        for i in 0..NBUF {
            let h = dev.rxq.read_desc(2 * i);
            let p = dev.rxq.read_desc(2 * i + 1);
            assert_eq!(h.flags, VRING_DESC_F_WRITE | VRING_DESC_F_NEXT);
            assert_eq!(h.next as usize, 2 * i + 1);
            assert_eq!(p.flags, VRING_DESC_F_WRITE);
            assert_eq!(p.len as usize, PKTSZ);
        }
    }

    #[test]
    fn test_transmit_rejects_oversize_and_empty() {
        let mut dev = VirtioNetDevice::empty();
        assert!(dev.probe(&mock_pci(), 0, &|_s| {}).is_ok());
        assert!(dev.transmit(&[]).is_err());
        let big = [0u8; PKTSZ + 1];
        assert!(dev.transmit(&big).is_err());
    }

    #[test]
    fn test_probe_requires_io_bar() {
        let mut pci = mock_pci();
        pci.bars = [PciBar::None; 6];
        let mut dev = VirtioNetDevice::empty();
        assert!(dev.probe(&pci, 0, &|_s| {}).is_err());
    }
}
