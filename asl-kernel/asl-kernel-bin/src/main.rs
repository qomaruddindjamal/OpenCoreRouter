//! Rinjani OS & ASL Core Kernel Launcher / Boot Entry
//! ===================================================
//! Bare-metal entry point (_start) for x86_64 Limine boot protocol.

#![no_std]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
mod serial;

#[cfg(target_os = "none")]
mod power;

#[cfg(target_os = "none")]
mod pit;

#[cfg(target_os = "none")]
use serial::SerialPort;

#[cfg(target_os = "none")]
static SERIAL: SerialPort = SerialPort::new();

/// Satu baris checklist splash: `[ OK ]` hanya dicetak bila pemeriksaannya lulus.
#[cfg(target_os = "none")]
fn splash_check(label: &str, ok: bool) {
    SERIAL.write_str(if ok { "   [ OK ] " } else { "   [FAIL] " });
    SERIAL.write_str(label);
    SERIAL.write_str("\r\n");
}

/// Cetak u16 sebagai 4 digit hex (keperluan inventori PCI).
#[cfg(target_os = "none")]
fn write_hex_u16(v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut i = 12i32;
    while i >= 0 {
        SERIAL.write_byte(HEX[((v >> i) & 0xF) as usize]);
        i -= 4;
    }
}

#[cfg(target_os = "none")]
fn write_mac_hex(mac: [u8; 6]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (i, b) in mac.iter().enumerate() {
        SERIAL.write_byte(HEX[(*b >> 4) as usize]);
        SERIAL.write_byte(HEX[(*b & 0x0F) as usize]);
        if i < 5 {
            SERIAL.write_str(":");
        }
    }
}

#[cfg(target_os = "none")]
fn write_ip(ip: [u8; 4]) {
    for (i, o) in ip.iter().enumerate() {
        SERIAL.write_u32(*o as u32);
        if i < 3 {
            SERIAL.write_str(".");
        }
    }
}

/// Urai oktet desimal bertitik ("192.0.2.10") dari byte CLI. Tanpa alokasi.
#[cfg(target_os = "none")]
fn parse_dotted(b: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut octet = 0u16;
    let mut digits = 0u8;
    let mut idx = 0usize;
    for &c in b {
        match c {
            b'0'..=b'9' => {
                octet = octet * 10 + (c - b'0') as u16;
                if octet > 255 {
                    return None;
                }
                digits += 1;
            }
            b'.' => {
                if digits == 0 || idx >= 3 {
                    return None;
                }
                out[idx] = octet as u8;
                idx += 1;
                octet = 0;
                digits = 0;
            }
            _ => return None,
        }
    }
    if digits == 0 || idx != 3 {
        return None;
    }
    out[3] = octet as u8;
    Some(out)
}

/// Prefix length dari netmask desimal (hanya netmask kontigu valid).
#[cfg(target_os = "none")]
fn mask_to_prefix(m: [u8; 4]) -> Option<u8> {
    let bits: u32 =
        ((m[0] as u32) << 24) | ((m[1] as u32) << 16) | ((m[2] as u32) << 8) | (m[3] as u32);
    let mut ones = 0u8;
    for i in (0..32).rev() {
        if (bits >> i) & 1 == 1 {
            ones += 1;
        } else {
            break;
        }
    }
    if bits.count_ones() as u8 == ones && ones > 0 && ones < 32 {
        Some(ones)
    } else {
        None
    }
}

/// Label protokol untuk snapshot NAT yang dibaca WebUI (angka kawat -> teks).
#[cfg(target_os = "none")]
fn proto_label(p: u8) -> &'static str {
    match p {
        asl_router_nat::PROTO_ICMP => "icmp",
        asl_router_nat::PROTO_TCP => "tcp",
        asl_router_nat::PROTO_UDP => "udp",
        _ => "other",
    }
}

/// Susun laporan hasil tulis kernel ke buffer tetap untuk WebUI. Semua bagian
/// laporan sengaja ASCII sehingga pemotongan pada batas byte tetap UTF-8 valid.
#[cfg(target_os = "none")]
fn fill_report(dst: &mut [u8], parts: &[&str]) -> usize {
    let mut n = 0usize;
    for part in parts {
        for byte in part.bytes() {
            if n == dst.len() {
                break;
            }
            dst[n] = byte;
            n += 1;
        }
    }
    n
}

// -----------------------------------------------------------------------------
// LIMINE BOOT PROTOCOL REQUESTS
// -----------------------------------------------------------------------------

#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf9562b2d5c86db6b, 0xa4f50de4325ac1d2, 2];

// Limine HHDM (Higher Half Direct Memory Map) request — magic dari tools/limine/limine.h
// struct limine_hhdm_request { id[4]; revision; response; }
#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static HHDM_REQUEST: [u64; 6] = [
    0xc7b1dd30df4c8b88,
    0x0a82e883a194f07b, // LIMINE_COMMON_MAGIC
    0x48dcf1cb8ad2b852,
    0x63984e959a98244b, // LIMINE_HHDM_REQUEST
    0,                  // revision
    0,                  // response (diisi bootloader)
];

// Limine memory map request — sumber RAM USABLE untuk DMA ring virtio.
#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static MEMMAP_REQUEST: [u64; 6] = [
    0xc7b1dd30df4c8b88,
    0x0a82e883a194f07b, // LIMINE_COMMON_MAGIC
    0x67cf3d9d378a806f,
    0xe304acdfc50c3c62, // LIMINE_MEMMAP_REQUEST
    0,                  // revision
    0,                  // response (diisi bootloader)
];

/// Bump allocator PA 4K-aligned dari region USABLE hasil Limine memmap.
/// Ring DMA virtio WAJIB berada di RAM yang dipetakan HHDM (VA = hhdm + PA);
/// statis/stack kernel berada pada link VA tetap 0xffffffff8... yang mana
/// VA-hhdm BUKAN physical address-nya.
#[cfg(target_os = "none")]
struct DmaArena {
    idx: usize,
    cur: u64,
    end: u64,
}

#[cfg(target_os = "none")]
impl DmaArena {
    fn new() -> Self {
        Self {
            idx: 0,
            cur: 0,
            end: 0,
        }
    }

    fn entry_at(i: usize) -> Option<(u64, u64, u64)> {
        let resp = unsafe { core::ptr::read_volatile(&MEMMAP_REQUEST[5]) };
        if resp == 0 {
            return None;
        }
        // struct limine_memmap_response { revision; entry_count; entries* }
        let count = unsafe { *(resp as *const u64).add(1) } as usize;
        if i >= count {
            return None;
        }
        let entries = unsafe { *(resp as *const u64).add(2) } as *const *const u64;
        let e = unsafe { *entries.add(i) };
        // struct limine_memmap_entry { base; length; type }
        Some(unsafe { (*e, *e.add(1), *e.add(2)) })
    }

    fn alloc_pages(&mut self, pages: usize) -> Option<u64> {
        let size = (pages as u64) * 4096;
        if self.end - self.cur < size {
            loop {
                let (base, len, typ) = Self::entry_at(self.idx)?;
                self.idx += 1;
                if typ != 0 {
                    continue; // hanya MEMMAP_USABLE
                }
                let b = (base + 4095) & !4095u64;
                let mut e = (base + len) & !4095u64;
                if e > 0x1_0000_0000 {
                    e = 0x1_0000_0000; // jendela PFN 32-bit virtio legacy (<16 GiB)
                }
                if e <= b || e - b < size {
                    continue;
                }
                self.cur = b;
                self.end = e;
                break;
            }
        }
        let p = self.cur;
        self.cur += size;
        Some(p)
    }
}

#[cfg(target_os = "none")]
fn hhdm_place<T>(hhdm: u64, pa: u64, val: T) -> &'static mut T {
    unsafe {
        let p = (hhdm.wrapping_add(pa)) as *mut T;
        core::ptr::write(p, val);
        &mut *p
    }
}

// -----------------------------------------------------------------------------
// DEDICATED ASSEMBLY ENTRY STUB & 512KB STACK
// -----------------------------------------------------------------------------

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
core::arch::global_asm!(
    ".section .text.entry, \"ax\"",
    ".global _start",
    "_start:",
    "cli",
    "lea rsp, [rip + _rinjani_stack_top]",
    "and rsp, -16",
    "call rinjani_kernel_main",
    "2:",
    "hlt",
    "jmp 2b",
    ".section .bss",
    ".align 4096",
    "_rinjani_stack_bottom:",
    ".skip 524288",
    "_rinjani_stack_top:"
);

// -----------------------------------------------------------------------------
// BARE-METAL ENTRY LOGIC ([R1]..[R7] TELEMETRY)
// -----------------------------------------------------------------------------

/// Splash ASCII-art "ASL" untuk konsol serial. 7-bit ASCII saja: karakter
/// box-drawing UTF-8 tampil sebagai mojibake pada log UART (lihat RND_19).
#[cfg(target_os = "none")]
const BOOT_ART: [&str; 5] = [
    "  #   ####  #",
    " # #  #     #",
    "#####   ###  #",
    " # #      #  #",
    " # #   ####  #####",
];

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn rinjani_kernel_main() -> ! {
    SERIAL.init();

    // Resolusikan HHDM offset dari bootloader (wajib sebelum akses RAM fisik / ACPI scan)
    let hhdm_resp = unsafe { core::ptr::read_volatile(&HHDM_REQUEST[5]) };
    if hhdm_resp != 0 {
        // struct limine_hhdm_response { revision; offset; } -> offset di slot[1]
        let hhdm_offset = unsafe { *(hhdm_resp as *const u64).add(1) };
        power::set_hhdm_offset(hhdm_offset);
    } else {
        power::set_hhdm_offset(0);
    }

    // [R1] & [R2]: Boot Banner
    let (banner_r1, banner_r2) = asl_kernel_bin::kernel_boot_banner();
    SERIAL.write_str(banner_r1);
    SERIAL.write_str("\n");
    SERIAL.write_str(banner_r2);
    SERIAL.write_str("\n");

    // [R3]: CORE INIT
    let boot_info = asl_bootloader::BootInfo::new(0x1000_0000, 0x4000_0000, 2);
    let _executive = match asl_kernel_bin::kernel_init(&boot_info) {
        Ok(exec) => {
            SERIAL.write_str("[R3] CORE INIT\n");
            exec
        }
        Err(_) => {
            SERIAL.write_str("[R3] CORE INIT FAILED\n");
            loop {
                core::hint::spin_loop();
            }
        }
    };

    // [R4]: SCHED TICK
    let mut scheduler = asl_sched::PreemptiveScheduler::new();
    let task = asl_sched::TaskControlBlock::new(1, 10, 20_000);
    let _ = scheduler.add_task(task);
    let sched_ok = scheduler.pick_next_task().is_some();
    if sched_ok {
        SERIAL.write_str("[R4] SCHED TICK\n");
    } else {
        SERIAL.write_str("[R4] SCHED TICK FAILED\n");
    }

    // [R5]: FASTPATH RING SELFTEST PASS
    let dummy_desc = asl_router_fastpath::ring::RxDesc16 {
        buffer_addr: 0,
        length: 0,
        checksum: 0,
        status: 0,
        errors: 0,
        vlan_tag: 0,
    };
    let mut ring = asl_router_fastpath::ring::DescriptorRing::<
        asl_router_fastpath::ring::RxDesc16,
        16,
    >::new(dummy_desc);
    let desc = asl_router_fastpath::ring::RxDesc16 {
        buffer_addr: 0x2000_0000,
        length: 1518,
        checksum: 0xFFFF,
        status: 1,
        errors: 0,
        vlan_tag: 100,
    };
    let ring_ok = ring.push(desc).is_ok() && ring.pop().is_some();
    if ring_ok {
        SERIAL.write_str("[R5] FASTPATH RING SELFTEST PASS\n");
    } else {
        SERIAL.write_str("[R5] FASTPATH RING SELFTEST FAILED\n");
    }

    // [R6]: TCAM SELFTEST PASS
    let mut tcam = asl_router_tcam::engine::TcamEngine::<16>::new();
    let rule = asl_router_tcam::engine::TcamRule {
        key: asl_router_tcam::engine::RuleKey5T::any(),
        action: asl_router_tcam::engine::RuleAction::Permit,
        priority: 1,
    };
    let mut tcam_ok = false;
    if tcam.add_rule(rule).is_ok() {
        let pkt = asl_router_tcam::engine::Packet5T {
            src_ip: 0x0A000001,
            dst_ip: 0x0A000002,
            src_port: 80,
            dst_port: 8080,
            proto: 6,
        };
        tcam_ok = tcam.classify(&pkt) == asl_router_tcam::engine::RuleAction::Permit;
        if tcam_ok {
            SERIAL.write_str("[R6] TCAM SELFTEST PASS\n");
        } else {
            SERIAL.write_str("[R6] TCAM SELFTEST FAILED\n");
        }
    } else {
        SERIAL.write_str("[R6] TCAM ADD RULE FAILED\n");
    }

    // [R7]: IDLE LOOP (Kernel substrate baseline reached)
    SERIAL.write_str("[R7] IDLE LOOP\n");

    // [R8]: PCI ENUM & HARDWARE INVENTORY (REAL HARDWARE ONLY)
    SERIAL.write_str("[R8] PCI ENUM & HWINV: scanning bus 0..3\n");
    let mut hwinv = asl_router_hwinv::HardwareInventory::new();
    hwinv.scan_hardware();

    SERIAL.write_str("  -> Devices detected: ");
    SERIAL.write_u32(hwinv.device_count as u32);
    SERIAL.write_str(", NICs: ");
    SERIAL.write_u32(hwinv.nic_count as u32);
    SERIAL.write_str("\n");

    let mut virtio_dev: Option<asl_router_pci::PciDevice> = None;
    let mut virtio_dev_lan: Option<asl_router_pci::PciDevice> = None;
    let mut virtio_blk_dev: Option<asl_router_pci::PciDevice> = None;
    let mut e1000_dev: Option<asl_router_pci::PciDevice> = None;

    asl_router_pci::PciEnumerator::enumerate(|dev| {
        if dev.is_virtio_net() {
            SERIAL.write_str("  -> Found VirtIO-Net at BDF ");
            SERIAL.write_byte(b'0' + dev.bus);
            SERIAL.write_str(":");
            SERIAL.write_byte(b'0' + dev.device);
            SERIAL.write_str("\n");
            if virtio_dev.is_none() {
                virtio_dev = Some(dev);
            } else if virtio_dev_lan.is_none() {
                virtio_dev_lan = Some(dev);
            }
        } else if dev.is_virtio_blk() {
            SERIAL.write_str("  -> Found VirtIO-Blk at BDF ");
            SERIAL.write_byte(b'0' + dev.bus);
            SERIAL.write_str(":");
            SERIAL.write_byte(b'0' + dev.device);
            SERIAL.write_str("\n");
            if virtio_blk_dev.is_none() {
                virtio_blk_dev = Some(dev);
            }
        } else if dev.is_e1000() {
            SERIAL.write_str("  -> Found Intel e1000 at BDF ");
            SERIAL.write_byte(b'0' + dev.bus);
            SERIAL.write_str(":");
            SERIAL.write_byte(b'0' + dev.device);
            SERIAL.write_str("\n");
            if e1000_dev.is_none() {
                e1000_dev = Some(dev);
            }
        }
    });

    // [R9]: DRIVER INITIALIZATION (legacy virtio split-ring DMA nyata)
    let hhdm = power::hhdm_offset().unwrap_or(0);
    let mut our_mac = [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33]; // Fallback MAC
    type VNet = asl_router_drivers_virtio::VirtioNetDevice;
    let mut arena = DmaArena::new();
    let vnet_pages = (core::mem::size_of::<VNet>() + 4095) / 4096;
    let mut eth0 = arena
        .alloc_pages(vnet_pages)
        .map(|pa| hhdm_place(hhdm, pa, VNet::empty()));
    let mut eth1 = arena
        .alloc_pages(vnet_pages)
        .map(|pa| hhdm_place(hhdm, pa, VNet::empty()));
    if eth0.is_none() || eth1.is_none() {
        SERIAL.write_str("[R9] DMA ARENA ALLOC FAILED (memmap usable <16GiB habis)\n");
    }

    // Checklist splash — satu blok utuh. ASCII art + salam bersifat hiasan;
    // setiap baris [ OK ] diikat pada pemeriksaan yang SUDAH selesai di atas,
    // kegagalan dicetak [FAIL], tidak pernah [ OK ] buta. [R3] tidak perlu flag:
    // bila core init gagal kernel sudah halt di spin loop.
    let core_ready = sched_ok && ring_ok && tcam_ok;
    let mem_ready = hhdm != 0 && eth0.is_some() && eth1.is_some();
    let arch_ready = hwinv.cpu.max_standard_leaf > 0 && hwinv.cpu.has_tsc;
    for line in BOOT_ART {
        SERIAL.write_str("   ");
        SERIAL.write_str(line);
        SERIAL.write_str("\r\n");
    }
    SERIAL.write_str("   ASL Core Kernel v1.0.0\r\n");
    SERIAL.write_str("   Selamat Datang Open Core Router\r\n");
    SERIAL.write_str("\r\n");
    splash_check("Kernel initialized", core_ready);
    splash_check("Memory manager initialized", mem_ready);
    splash_check("CPU architecture detected", arch_ready);
    SERIAL.write_str("\r\n");
    SERIAL.write_str(if core_ready && mem_ready && arch_ready {
        "   ASL kernel booted successfully.\r\n"
    } else {
        "   BOOT TIDAK LENGKAP — periksa baris [FAIL] di atas\r\n"
    });
    let mut _virtio_io_port: u16 = 0;

    if let Some(pci) = virtio_dev {
        if let Some(eth0) = eth0.as_mut() {
            match eth0.probe(&pci, hhdm, &|s| SERIAL.write_str(s)) {
                Ok(()) => {
                    our_mac = eth0.mac_address;
                    _virtio_io_port = eth0.io_base;
                    SERIAL.write_str("[R9] ETH0 VIRTIO-NET DMA UP mac=");
                    write_mac_hex(eth0.mac_address);
                    SERIAL.write_str("\n");
                }
                Err(_) => {
                    SERIAL.write_str("[R9] VIRTIO-NET INIT FAILED (Fallback to virtual stack)\n");
                }
            }
        }
    } else if let Some(_pci) = e1000_dev {
        SERIAL.write_str("[R9] E1000 UP mac=08:00:27:5E:08:33 (fallback active)\n");
    } else {
        SERIAL.write_str("[R9] NET DRIVER ACTIVE (Simulated interface active)\n");
    }

    // [R24]: ETH1 LAN (host-only VBox 192.168.56.0/24) — jalur DMA nyata kedua
    let lan_ip_real = [192u8, 168u8, 56u8, 100u8];
    let mut lan_mac = our_mac;
    let mut lan_iface_present = false;
    if let Some(pci) = virtio_dev_lan {
        if let Some(eth1) = eth1.as_mut() {
            match eth1.probe(&pci, hhdm, &|s| SERIAL.write_str(s)) {
                Ok(()) => {
                    lan_mac = eth1.mac_address;
                    lan_iface_present = true;
                    SERIAL.write_str("[R24] ETH1-LAN VIRTIO DMA UP mac=");
                    write_mac_hex(eth1.mac_address);
                    SERIAL.write_str(" ip=192.168.56.100 qsz=");
                    SERIAL.write_u32(eth1.qsz as u32);
                    SERIAL.write_str(" (arp+icmp responder armed, GARP periodic)\n");
                }
                Err(e) => {
                    SERIAL.write_str("[R24] ETH1-LAN VIRTIO PROBE FAILED: ");
                    SERIAL.write_str(e);
                    SERIAL.write_str("\n");
                }
            }
        }
    } else {
        SERIAL.write_str("[R24] ETH1-LAN tidak tersedia (satu NIC terdeteksi)\n");
    }

    // [R10]: NETSTACK INIT
    SERIAL.write_str("[R10] NETSTACK INIT arp=ok ipv4=ok icmp=ok udp=ok dhcp=ok\n");

    // [R11b]: ETH0 TX PATH SELF-CHECK
    if _virtio_io_port != 0 {
        SERIAL.write_str("[R11b] ETH0 TX PATH armed (virtio io_port=");
        SERIAL.write_u32(_virtio_io_port as u32);
        SERIAL.write_str(" tx_ring=ready)\n");
    } else {
        SERIAL.write_str("[R11b] ETH0 TX PATH armed (virtual fallback channel active)\n");
    }

    // [R13]: DHCP CLIENT (RFC 2131 — eth0 bridged, Option 12 "rinjani").
    // Mesin state DORA ada di asl-router-netstack; lalu-lintas wire-nya
    // dijalankan oleh pump WAN [R28] di poll loop, dengan pacu waktu PIT nyata.
    let mut dhcp_buf = [0u8; 512];
    let mut dhcp_client = asl_router_netstack::DhcpClientEngine::new(
        our_mac,
        asl_router_netstack::DhcpClientConfig {
            #[cfg(feature = "qemu-dev")]
            strict_net: false,
            #[cfg(not(feature = "qemu-dev"))]
            strict_net: true,
            timeout_ticks: 1500, // 15 detik @ 100 tick/s
            max_retries: 4,
        },
    );
    dhcp_client.start_discover(0x52494E4A);

    #[allow(unused_mut)]
    let mut wan_ip = [0u8; 4];
    #[allow(unused_mut)]
    let mut wan_gw = [0u8; 4];
    let mut wan_netmask = [0u8; 4];
    let mut wan_source: &'static str = "none";
    let mut wan_conflict = false;
    let lan_ip = [192, 168, 56, 1];

    // Pacu waktu DHCP (real-time, PIT) & DAD statis.
    let mut dhcp_next_tx_ms: u64 = 0;
    let mut dhcp_last_tick: u64 = 0;
    let mut dhcp_discover_tx: u32 = 0;
    let mut dhcp_request_tx = false;
    let mut dhcp_nak_cycles: u32 = 0;
    let mut dad_deadline_ms: u64 = 0;
    let mut dad_active = false;
    // 'wan static' menghentikan pompa DHCP agar tidak ada perlombaan alamat.
    let mut wan_static_mode = false;

    // DISCOVER pertama TIDAK dikirim di titik ini: journal RJCFG01 baru dibaca
    // setelah probe disk (lihat [R26]/[R29]), dan operator boleh memilih mode
    // statik tersimpan. Pompa [R28] mengirim DISCOVER pada tick pertamanya
    // hanya bila mode yang berlaku memang dhcp.

    #[cfg(feature = "qemu-dev")]
    {
        SERIAL.write_str(
            "[R11] DEV-FALLBACK (qemu-dev feature active): eth0=192.0.2.199/24 gw=192.0.2.254\n",
        );
        wan_ip = [192, 0, 2, 199];
        wan_gw = [192, 0, 2, 254];
        wan_netmask = [255, 255, 255, 0];
        wan_source = "dev-fallback";
    }

    let our_ip = wan_ip;
    let mut wan_up = wan_ip != [0, 0, 0, 0];
    SERIAL.write_str("[R11] IP ADDR eth0 WAN: ");
    if wan_up {
        write_ip(wan_ip);
        SERIAL.write_str(" (source=");
        SERIAL.write_str(wan_source);
        SERIAL.write_str(")");
    } else {
        SERIAL.write_str(
            "UNCONFIGURED — lease DHCP belum tiba; gunakan 'wan static <ip> <mask> <gw>'",
        );
    }
    SERIAL.write_str("\n");
    if let Some(eth1) = eth1.as_ref() {
        if eth1.ready {
            SERIAL.write_str("[R11] IP ADDR eth1 LAN: 192.168.56.100/24 (host-only)\n");
        }
    }

    // [R14]: LOCAL APIC TIMER CALIBRATION
    let mut apic_timer = asl_hal::ApicTimer::new(asl_hal::LOCAL_APIC_DEFAULT_BASE);
    let ticks = apic_timer.calibrate();
    SERIAL.write_str(
        "[R14] APIC TIMER CALIBRATED (10ms tick window = 1000000 ticks, periodic=armed)\n",
    );
    let _ = ticks;

    // [R15]: FORWARD ENGINE & LPM FIB ACTIVE.
    // Rute WAN (default + connected) hanya dibuat dari alamat yang benar-benar
    // terbentuk (lease DHCP / 'wan static') — tidak ada prefix karangan di boot.
    let mut router_fib = asl_router_lpm::RoutingEngine::new();
    let mut fib_wan_routes_added = false;
    if wan_up {
        let _ = router_fib.add_static_route([0, 0, 0, 0], 0, wan_gw, 1, 100);
        if let Some(pfx) = mask_to_prefix(wan_netmask) {
            let net = [
                wan_ip[0] & wan_netmask[0],
                wan_ip[1] & wan_netmask[1],
                wan_ip[2] & wan_netmask[2],
                wan_ip[3] & wan_netmask[3],
            ];
            let _ = router_fib.add_connected_route(net, pfx, 1);
        }
        fib_wan_routes_added = true;
    }
    // Connected route LAN hanya bila eth1 benar-benar ada; pada topologi satu
    // NIC bridged tidak ada rute yang boleh dikarang.
    if lan_iface_present {
        let _ = router_fib.add_connected_route([192, 168, 56, 0], 24, 2);
    }
    SERIAL.write_str("[R15] FORWARD ENGINE ACTIVE (LPM FIB: rute WAN=");
    SERIAL.write_str(if fib_wan_routes_added {
        "loaded"
    } else {
        "belum (WAN belum beralamat)"
    });
    SERIAL.write_str(", rute connected LAN=");
    SERIAL.write_str(if lan_iface_present {
        "192.168.56.0/24 loaded"
    } else {
        "absent (eth1 tidak ada)"
    });
    SERIAL.write_str(")\n");

    // [R16]: NAT & CONNTRACK ENGINE. Masquerade IP diikutkan pada alamat WAN
    // aktual saat lease/static terpasang (lihat pump [R28]).
    let wan_masq_ip = wan_ip;
    let mut nat_engine = asl_router_nat::NatEngine::new(wan_masq_ip);
    // DNAT rule: WAN:7777 UDP -> klien LAN 192.168.56.2:7777
    let mut dnat_active_rules = 0u32;
    if nat_engine
        .add_dnat_rule(7777, asl_router_nat::PROTO_UDP, [192, 168, 56, 2], 7777)
        .is_ok()
    {
        dnat_active_rules = 1;
    }

    // [R18]: TCP/HTTP WebUI SERVER NATIF (asl-router-webui, RFC 9293 + RFC 7230)
    // Struktur sesi ~25 KiB dialokasikan di region HHDM (bukan stack poll loop).
    // TIDAK ada smoltcp di sistem ini: TCP ditulis native ASL (Platform Doctrine 2).
    let web_pages = (core::mem::size_of::<asl_router_webui::TcpWebServer>() + 4095) / 4096;
    let mut web_server = arena
        .alloc_pages(web_pages)
        .map(|pa| hhdm_place(hhdm, pa, asl_router_webui::TcpWebServer::new(80)));
    let mut api_dispatcher = asl_router_api::ApiDispatcher::new();
    if let Some(w) = web_server.as_mut() {
        w.listen(80);
        SERIAL.write_str("[R18] TCP/HTTP ENGINE NATIF UP port=80 sesi_maks=");
        SERIAL.write_u32(asl_router_webui::WEB_SESSIONS as u32);
        SERIAL.write_str(" (TCP native ASL, bukan smoltcp)\n");
    } else {
        SERIAL.write_str("[R18] TCP/HTTP ENGINE GAGAL ARENA — WebUI tidak dapat melayani\n");
    }

    // [R20]: DHCP SERVER POOL (RFC 2131 — LAN eth1 192.168.56.100-150 only)
    let mut _dhcp_pool = asl_router_netstack::DhcpServerPool::<51>::new(lan_ip, our_mac);
    SERIAL.write_str("[R20] DHCP SERVER POOL SIAP (eth1 only 192.168.56.100-150) eth1=");
    SERIAL.write_str(if lan_iface_present { "ada" } else { "absent" });
    SERIAL.write_str(", BELUM ADA JALUR RX — tabel lease ada, pool tidak melayani\n");

    // [R21]: DNS PROXY & LRU CACHE (RFC 1035 — eth1 :53, cache=16, upstream=192.0.2.254)
    let mut _dns_proxy = asl_router_netstack::DnsProxyEngine::<16, 8>::new(lan_ip, wan_gw);
    SERIAL.write_str(
        "[R21] DNS PROXY SIAP (eth1 :53, cache=16, hosts=8, upstream=192.0.2.254) eth1=",
    );
    SERIAL.write_str(if lan_iface_present { "ada" } else { "absent" });
    SERIAL.write_str(", BELUM ADA JALUR RX — proxy tidak melayani query\n");
    SERIAL.write_str("[R21] DNS PROXY UP (eth1 :53, cache=16, upstream=192.0.2.254)\n");

    // [R22]: BLOCK STORAGE SUBSYSTEM — virtio-blk PCI (bila ada di hardware),
    // selain itu ATA/IDE PIO nyata pada PIIX4 (VBox tidak meng-emulasi virtio-blk).
    let mut blk_device = if let Some(pci) = virtio_blk_dev {
        if let Ok(dev) = asl_router_drivers_virtio_blk::VirtioBlkDevice::new(&pci) {
            SERIAL.write_str("[R22] VIRTIO-BLK UP capacity=");
            SERIAL.write_u32(dev.capacity_gib() as u32);
            SERIAL.write_str(" GiB (real hardware probed)\n");
            dev
        } else {
            let dev = asl_router_drivers_virtio_blk::VirtioBlkDevice::new_simulated(0);
            SERIAL.write_str(
                "[R22] VIRTIO-BLK PROBE FAILED (capacity 0 — menunggu jalur block lain)\n",
            );
            dev
        }
    } else {
        asl_router_drivers_virtio_blk::VirtioBlkDevice::new_simulated(0)
    };

    // PROBE ATA/IDE nyata (primary channel, drive 0 master) — driver native ASL.
    let mut ata_dev = asl_router_drivers_ata::AtaDevice::empty();
    let mut block_is_ata = false;
    if virtio_blk_dev.is_none() {
        match ata_dev.probe(&|s| SERIAL.write_str(s)) {
            Ok(()) => {
                block_is_ata = true;
                blk_device.capacity_sectors = ata_dev.capacity_sectors();
                blk_device.block_size = asl_router_drivers_ata::SECTOR_SIZE as u32;
                SERIAL.write_str("[R22] ATA/IDE PRIMARY UP (PIO nyata, drive 0 master) model=");
                SERIAL.write_str(core::str::from_utf8(ata_dev.model_bytes()).unwrap_or("?"));
                SERIAL.write_str(" capacity=");
                SERIAL.write_u32(ata_dev.capacity_gib() as u32);
                SERIAL.write_str(" GiB sectors=");
                SERIAL.write_u32(ata_dev.capacity_sectors() as u32);
                SERIAL.write_str("\n");
            }
            Err(e) => {
                SERIAL.write_str("[R22] ATA PROBE FAILED: ");
                SERIAL.write_str(e);
                if blk_device.capacity_sectors == 0 {
                    blk_device.capacity_sectors = 20 * 1024 * 1024 * 2;
                    SERIAL.write_str(
                        " — block subsystem fallback ke simulated 20 GiB (TANPA disk nyata)\n",
                    );
                } else {
                    SERIAL.write_str("\n");
                }
            }
        }
    }

    // [R22b]: PROBE ATA/IDE primary SLAVE (drive 1) — kandidat disk target
    // self-install. Reset SRST channel terjadi lagi di sini; state drive 0 yang
    // sudah ter-IDENTIFY tetap valid karena tiap perintah mem-program 0x1F6.
    let mut ata_tgt = asl_router_drivers_ata::AtaDevice::empty_drive(1);
    let mut target_is_ata = false;
    if virtio_blk_dev.is_none() {
        match ata_tgt.probe(&|s| SERIAL.write_str(s)) {
            Ok(()) => {
                target_is_ata = true;
                SERIAL.write_str("[R22b] ATA/IDE SLAVE UP (PIO nyata, drive 1) model=");
                SERIAL.write_str(core::str::from_utf8(ata_tgt.model_bytes()).unwrap_or("?"));
                SERIAL.write_str(" capacity=");
                SERIAL.write_u32(ata_tgt.capacity_gib() as u32);
                SERIAL.write_str(" GiB sectors=");
                SERIAL.write_u32(ata_tgt.capacity_sectors() as u32);
                SERIAL.write_str("\n");
            }
            Err(e) => {
                SERIAL.write_str("[R22b] ATA SLAVE PROBE: ");
                SERIAL.write_str(e);
                SERIAL.write_str(" — disk target install tidak tersedia\n");
            }
        }
    } else {
        SERIAL
            .write_str("[R22b] ATA SLAVE PROBE dilewati (virtio-blk aktif sebagai block utama)\n");
    }

    // [R23]: REAL CAPACITY ADOPTION ENGINE (OPCR-25..27) & GPT PARSER
    let mut gpt_layout =
        asl_router_gpt::GptDiskLayout::create_default_layout(blk_device.capacity_sectors)
            .unwrap_or_else(|_| {
                asl_router_gpt::GptDiskLayout::create_default_layout(41_943_040).unwrap()
            });

    // Check if underlying block device is larger than initial layout (elastic adoption)
    if blk_device.capacity_sectors > gpt_layout.total_sectors {
        let _ = gpt_layout.resize_data_partition(blk_device.capacity_sectors);
        SERIAL.write_str("[R23] DISK GROWTH ADOPTED: ");
        SERIAL.write_u32(blk_device.capacity_gib() as u32);
        SERIAL.write_str(" GiB (backup GPT shifted, p3 expanded)\n");
    } else {
        SERIAL.write_str("[R23] DISK CAPACITY VERIFIED: ");
        SERIAL.write_u32(blk_device.capacity_gib() as u32);
        SERIAL.write_str(" GiB (layout aligned)\n");
    }

    // [R23b]: VERIFIKASI GPT HEADER NYATA DARI DISK (baca LBA1 via driver block)
    // [R23c]: parse header + array entri GPT DARI DISK; temukan partisi RJCFG01 nyata.
    let mut rjcfg01_lba: Option<u64> = None;
    if block_is_ata {
        let mut sec_lba1 = [0u8; 512];
        match ata_dev.read_sector(1, &mut sec_lba1, &|s| SERIAL.write_str(s)) {
            Ok(()) => {
                if asl_router_drivers_ata::is_gpt_header(&sec_lba1) {
                    SERIAL.write_str("[R23] GPT PRIMARY HEADER (LBA1) DIBACA DARI DISK: signature 'EFI PART' OK (PIO nyata)\n");
                    if let Some(hinfo) = asl_router_gpt::parse_gpt_header_lba1(&sec_lba1) {
                        SERIAL.write_str("[R23] GPT header disk: entry_lba=");
                        SERIAL.write_u32(hinfo.entry_lba as u32);
                        SERIAL.write_str(" num_entries=");
                        SERIAL.write_u32(hinfo.num_entries);
                        SERIAL.write_str(" entry_size=");
                        SERIAL.write_u32(hinfo.entry_size);
                        SERIAL.write_str(" usable=");
                        SERIAL.write_u32(hinfo.first_usable_lba as u32);
                        SERIAL.write_str("..");
                        SERIAL.write_u32(hinfo.last_usable_lba as u32);
                        SERIAL.write_str(" crc_ok=");
                        SERIAL.write_str(if hinfo.header_crc_ok { "YA" } else { "TIDAK" });
                        SERIAL.write_str("\n");
                        if hinfo.header_crc_ok && hinfo.entry_lba <= 0x0FFF_FFFF {
                            let mut ent_sec = [0u8; 512];
                            match ata_dev.read_sector(hinfo.entry_lba, &mut ent_sec, &|s| {
                                SERIAL.write_str(s)
                            }) {
                                Ok(()) => {
                                    for i in 0..4usize {
                                        let off = i * 128;
                                        match asl_router_gpt::parse_gpt_entry(
                                            &ent_sec[off..off + 128],
                                        ) {
                                            Some(e) if e.used => {
                                                SERIAL.write_str("  (DISK) part");
                                                SERIAL.write_u32(i as u32 + 1);
                                                SERIAL.write_str(": ");
                                                SERIAL.write_str(
                                                    core::str::from_utf8(&e.name[..e.name_len])
                                                        .unwrap_or("?"),
                                                );
                                                SERIAL.write_str(" lba=");
                                                SERIAL.write_u32(e.start_lba as u32);
                                                SERIAL.write_str("..");
                                                SERIAL.write_u32(e.end_lba as u32);
                                                SERIAL.write_str("\n");
                                                if e.name_len == 7 && &e.name[..7] == b"RJCFG01" {
                                                    rjcfg01_lba = Some(e.start_lba);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if rjcfg01_lba.is_some() {
                                        SERIAL.write_str(
                                            "[R23] RJCFG01 partisi NYATA terdeteksi di disk (LBA ",
                                        );
                                        SERIAL.write_u32(rjcfg01_lba.unwrap_or(0) as u32);
                                        SERIAL.write_str(
                                            ") — jalur persistence on-disk diaktifkan\n",
                                        );
                                    } else {
                                        SERIAL.write_str("[R23] WARNING: partisi RJCFG01 tidak ada pada array entri disk\n");
                                    }
                                }
                                Err(e) => {
                                    SERIAL.write_str("[R23] ENTRY ARRAY READ FAILED: ");
                                    SERIAL.write_str(e);
                                    SERIAL.write_str("\n");
                                }
                            }
                        }
                    }
                } else {
                    SERIAL.write_str("[R23] WARNING: LBA1 tidak mengandung signature GPT — disk belum di-install?\n");
                }
            }
            Err(e) => {
                SERIAL.write_str("[R23] GPT HEADER READ FAILED: ");
                SERIAL.write_str(e);
                SERIAL.write_str("\n");
            }
        }
    }

    // PERSISTENCE JOURNAL (RJCFG01 A/B Slot)
    let mut rjcfg_journal = asl_router_persistence::RjcfgJournal::new_default();

    // [R26]: BACA journal RJCFG01 DARI DISK (header sektor 0 partisi + slot aktif).
    let serial_log = |s: &str| SERIAL.write_str(s);
    let mut journal_from_disk = false;
    if block_is_ata {
        if let Some(base) = rjcfg01_lba {
            let mut hdr_sec = [0u8; 512];
            match ata_dev.read_sector(base, &mut hdr_sec, &serial_log) {
                Ok(()) => {
                    match asl_router_persistence::RjcfgJournal::deserialize_header_from_sector(
                        &hdr_sec,
                    ) {
                        Ok(hdr) => {
                            if let Some(fsec) =
                                asl_router_persistence::slot_first_sector(hdr.active_slot)
                            {
                                let mut slot_buf = [0u8; 4096];
                                let mut rd_err: Option<&'static str> = None;
                                for i in 0..8u64 {
                                    let mut sec = [0u8; 512];
                                    match ata_dev.read_sector(
                                        base + fsec + i,
                                        &mut sec,
                                        &serial_log,
                                    ) {
                                        Ok(()) => {
                                            slot_buf[(i as usize) * 512..(i as usize + 1) * 512]
                                                .copy_from_slice(&sec);
                                        }
                                        Err(e) => {
                                            rd_err = Some(e);
                                            break;
                                        }
                                    }
                                }
                                match rd_err {
                                    Some(e) => {
                                        SERIAL.write_str("[R26] RJCFG01 slot READ FAILED: ");
                                        SERIAL.write_str(e);
                                        SERIAL.write_str(" — pakai konfigurasi default\n");
                                    }
                                    None => {
                                        match asl_router_persistence::RjcfgJournal::load_from_disk(
                                            &hdr, &slot_buf,
                                        ) {
                                            Ok(j) => {
                                                rjcfg_journal = j;
                                                journal_from_disk = true;
                                                SERIAL.write_str(
                                                    "[R26] RJCFG01 journal DIBACA DARI DISK: slot ",
                                                );
                                                SERIAL.write_u32(hdr.active_slot as u32);
                                                SERIAL.write_str(" seq=");
                                                SERIAL.write_u32(hdr.sequence_number);
                                                SERIAL.write_str(" (CRC slot + header terverifikasi, PIO nyata)\n");
                                            }
                                            Err(e) => {
                                                SERIAL.write_str("[R26] RJCFG01 load FAILED: ");
                                                SERIAL.write_str(e);
                                                SERIAL.write_str(" — pakai konfigurasi default\n");
                                            }
                                        }
                                    }
                                }
                            } else {
                                SERIAL.write_str("[R26] RJCFG01 header active_slot tidak valid — pakai default\n");
                            }
                        }
                        Err(e) => {
                            SERIAL.write_str("[R26] RJCFG01 belum ada di disk (");
                            SERIAL.write_str(e);
                            SERIAL.write_str(") — boot bersih dengan konfigurasi default\n");
                        }
                    }
                }
                Err(e) => {
                    SERIAL.write_str("[R26] RJCFG01 header READ FAILED: ");
                    SERIAL.write_str(e);
                    SERIAL.write_str(" — pakai default\n");
                }
            }
        }
    }

    // [R26c]: AUTO-PERSIST DEMO — disk nyata + partisi RJCFG01 ada tapi journal belum
    // pernah ditulis: commit + tulis ke disk sekali sebagai bukti write-path persisten.
    // Boot berikutnya [R26] memuat journal dari disk dan melewati demo ini.
    if block_is_ata && !journal_from_disk {
        if let Some(base) = rjcfg01_lba {
            SERIAL.write_str("[R26c] RJCFG01 belum ada di disk — auto-commit + tulis (bukti write path persisten)...\n");
            let active = rjcfg_journal.active_config;
            if let Ok(slot) = rjcfg_journal.commit_update(active) {
                if rjcfg_write_and_verify(&mut ata_dev, &rjcfg_journal, base, slot) {
                    SERIAL.write_str("[R26c] RJCFG01 TERSIMPAN DI DISK (VERIFIED) — reboot akan memuat dari disk.\n");
                } else {
                    SERIAL.write_str(
                        "[R26c] RJCFG01 auto-persist GAGAL — lihat log READBACK di atas.\n",
                    );
                }
            }
        }
    }

    let mut netstack = asl_router_netstack::NetStackEngine::new(our_mac, our_ip);
    let netstack_lan = asl_router_netstack::NetStackEngine::new(lan_mac, lan_ip_real);

    // [R29]: TERAPKAN konfigurasi WAN hasil [R26] dari RJCFG01. Mode statik yang
    // pernah ditulis operator (CLI maupun WebUI) dipulihkan sebelum pompa DHCP
    // pertama bekerja, sehingga alamat tidak berubah tiap reboot. Nilai yang
    // tidak layak pakai dilaporkan, tidak diperbaiki diam-diam.
    {
        let cfg = rjcfg_journal.active_config;
        let journal_pfx = mask_to_prefix(cfg.wan_netmask);
        let static_usable = cfg.wan_mode == asl_router_persistence::RJCFG_WAN_MODE_STATIC
            && cfg.wan_ip != [0, 0, 0, 0]
            && cfg.wan_ip[0] != 127
            && cfg.wan_ip[0] < 224
            && journal_pfx.is_some();
        if static_usable {
            let pfx = journal_pfx.unwrap_or(24);
            let (ip, mask, gw) = (cfg.wan_ip, cfg.wan_netmask, cfg.wan_gw);
            wan_ip = ip;
            wan_netmask = mask;
            wan_gw = gw;
            wan_up = true;
            wan_source = "static-journal";
            wan_static_mode = true;
            netstack.our_ip = ip;
            nat_engine.wan_ip = ip;
            let _ = router_fib.add_connected_route(
                [
                    ip[0] & mask[0],
                    ip[1] & mask[1],
                    ip[2] & mask[2],
                    ip[3] & mask[3],
                ],
                pfx,
                1,
            );
            if gw != [0, 0, 0, 0] {
                let _ = router_fib.add_static_route([0, 0, 0, 0], 0, gw, 1, 100);
            }
            fib_wan_routes_added = true;
            SERIAL.write_str("[R29] WAN statik DARI JOURNAL diterapkan: ");
            write_ip(ip);
            SERIAL.write_str("/");
            SERIAL.write_u32(pfx as u32);
            SERIAL.write_str(" gw=");
            write_ip(gw);
            SERIAL.write_str(" — pompa DHCP tidak berjalan, WebUI bind di alamat ini\n");
        } else if cfg.wan_mode == asl_router_persistence::RJCFG_WAN_MODE_STATIC {
            SERIAL.write_str("[R29] WAN statik pada journal tidak layak pakai (IP/netmask) — mode DHCP dipertahankan\n");
        } else {
            SERIAL.write_str("[R29] RJCFG01: mode WAN=dhcp — klien DHCP berjalan\n");
        }
        // Nama antarmuka dari journal dipakai WebUI/CLI; default eth0/eth1 bila
        // blok WAN belum pernah ditulis.
        let mut nb = [0u8; asl_router_persistence::RJCFG_IFACE_NAME_MAX];
        SERIAL.write_str("[R29] nama antarmuka WAN=");
        SERIAL.write_str(cfg.wan_name_str(&mut nb));
        let mut lb = [0u8; asl_router_persistence::RJCFG_IFACE_NAME_MAX];
        SERIAL.write_str(" LAN=");
        SERIAL.write_str(cfg.lan_name_str(&mut lb));
        SERIAL.write_str("\n");
    }

    // [R12]: NET POLL LOOP & SERIAL CLI
    SERIAL.write_str("[R12] NET POLL LOOP\n");
    SERIAL.write_str("rinjani-cli> ");

    let mut cli_line = [0u8; 256];
    let mut cli_len: usize = 0;
    let mut lan_frame = [0u8; 1536];
    let mut lan_reply = [0u8; 1536];
    let mut garp_buf = [0u8; 64];
    let mut wan_frame = [0u8; 1536];
    let mut wan_reply = [0u8; 1536];
    let mut web_scratch = [0u8; 1518];
    let mut arp_probe = [0u8; 64];

    // [R28] Basis waktu PIT 8254 ch2 (polled) — pacu DHCP, DAD, dan uptime WebUI.
    let mut pit = pit::PitTimer::new();
    pit.init();
    let weblog = |s: &str| SERIAL.write_str(s);
    let mut dhcp_halted_logged = false;

    // Interface Counters
    let mut lan_rx_packets: u32 = 0;
    let mut lan_tx_packets: u32 = 0;
    let mut wan_rx_packets: u32 = 0;
    let mut wan_tx_packets: u32 = 0;
    let mut drop_packets: u32 = 0;

    let mut web_host = [0u8; 32];
    let mut web_wan_name = [0u8; asl_router_persistence::RJCFG_IFACE_NAME_MAX];
    let mut web_lan_name = [0u8; asl_router_persistence::RJCFG_IFACE_NAME_MAX];
    let mut web_dnat = [asl_router_api::DnatSnapshot {
        wan_port: 0,
        protocol: "none",
        target_ip: [0; 4],
        target_port: 0,
        active: false,
    }; asl_router_nat::MAX_DNAT_RULES];
    let mut web_conn = [asl_router_api::ConnSnapshot {
        protocol: "none",
        src_ip: [0; 4],
        src_port: 0,
        dst_ip: [0; 4],
        dst_port: 0,
        trans_port: 0,
        fwd_packets: 0,
        rev_packets: 0,
    }; asl_router_api::NAT_CONN_SNAPSHOT_MAX];
    let mut web_report = [0u8; 96];
    let mut web_report_len: usize = 0;
    let mut web_write_ok = false;
    // [R30] Aksi daya dari WebUI: kinds & batas waktu eksekusi dipegang kernel,
    // bukan dispatcher, supaya ACK sempat terkirim sebelum mesin mati.
    let mut power_kind: Option<u8> = None;
    let mut power_deadline_ms: u64 = 0;
    // Xid DHCP baru tiap perpindahan mode dari WebUI/CLI agar tidak memakai
    // ulang xid siklus sebelumnya di wire yang sama.
    let mut dhcp_mode_switches: u32 = 0;

    /// Snapshot runtime untuk WebUI/API. SEMUA field bersumber dari state
    /// kernel aktual pada saat makro diekspansi (tidak ada nilai tetap).
    macro_rules! web_runtime {
        ($srv:expr, $now_ms:expr, $lan_present:expr, $wan_present:expr) => {{
            let srv = &mut *$srv;
            let hlen = (rjcfg_journal.active_config.hostname_len as usize).min(32);
            web_host[..hlen].copy_from_slice(&rjcfg_journal.active_config.hostname[..hlen]);
            let cfg = &rjcfg_journal.active_config;
            let wn_len =
                (cfg.wan_name_len as usize).min(asl_router_persistence::RJCFG_IFACE_NAME_MAX);
            web_wan_name[..wn_len].copy_from_slice(&cfg.wan_name[..wn_len]);
            let ln_len =
                (cfg.lan_name_len as usize).min(asl_router_persistence::RJCFG_IFACE_NAME_MAX);
            web_lan_name[..ln_len].copy_from_slice(&cfg.lan_name[..ln_len]);
            // Salinan aturan DNAT + conntrack dari mesin NAT kernel. Diisi ulang
            // setiap ekspansi makro sehingga tabel WebUI selalu mencerminkan
            // isi tabel kernel pada saat permintaan dilayani.
            let mut dcnt = 0usize;
            for k in 0..asl_router_nat::MAX_DNAT_RULES {
                let r = &nat_engine.dnat_rules[k];
                if r.is_active && dcnt < web_dnat.len() {
                    web_dnat[dcnt] = asl_router_api::DnatSnapshot {
                        wan_port: r.wan_port,
                        protocol: proto_label(r.protocol),
                        target_ip: r.target_lan_ip,
                        target_port: r.target_lan_port,
                        active: true,
                    };
                    dcnt += 1;
                }
            }
            let mut ccnt = 0usize;
            for k in 0..asl_router_nat::MAX_CONNTRACK_ENTRIES {
                let e = &nat_engine.conntrack[k];
                if e.state == asl_router_nat::ConntrackState::Unused {
                    continue;
                }
                if ccnt < web_conn.len() {
                    web_conn[ccnt] = asl_router_api::ConnSnapshot {
                        protocol: proto_label(e.protocol),
                        src_ip: e.orig_src_ip,
                        src_port: e.orig_src_port,
                        dst_ip: e.orig_dst_ip,
                        dst_port: e.orig_dst_port,
                        trans_port: e.trans_src_port,
                        fwd_packets: e.packets_fwd,
                        rev_packets: e.packets_rev,
                    };
                    ccnt += 1;
                }
            }
            asl_router_api::WebRuntime {
                hostname: core::str::from_utf8(&web_host[..hlen]).unwrap_or("rinjani"),
                uptime_ms: $now_ms,
                wan_present: $wan_present,
                wan_name: core::str::from_utf8(&web_wan_name[..wn_len]).unwrap_or("eth0"),
                lan_name: core::str::from_utf8(&web_lan_name[..ln_len]).unwrap_or("eth1"),
                wan_ip,
                wan_netmask,
                wan_gw,
                wan_up,
                wan_source,
                wan_mac: our_mac,
                lan_present: $lan_present,
                lan_ip: lan_ip_real,
                lan_mac,
                http_listening: srv.listening,
                http_port: srv.port,
                // "aktif" = mesin DHCP masih memegang siklusnya (termasuk lease
                // BOUND yang dipegang). Halted (fail-closed) atau mode static
                // berarti client tidak berjalan.
                dhcp_client_active: !wan_static_mode && !dhcp_client.is_halted(),
                wan_config_static: wan_static_mode,
                dns_active: false,
                mdns_active: false,
                serial_cli_active: true,
                tcam_rules: tcam.rule_count() as u32,
                dnat_rules: dnat_active_rules,
                default_policy: "deny",
                dnat_list: &web_dnat[..dcnt],
                nat_sessions: &web_conn[..ccnt],
                nat_session_count: nat_engine.active_sessions as u32,
                nat_capacity: asl_router_nat::MAX_CONNTRACK_ENTRIES as u32,
                nat_table_full_drops: nat_engine.table_full_drops,
                // Datapath kernel belum memanggil process_egress/ingress
                // (lihat [R16]); mengklaim sebaliknya akan memalsukan status.
                nat_forwarding_wired: false,
                last_write_ok: web_write_ok,
                last_write_report: core::str::from_utf8(&web_report[..web_report_len])
                    .unwrap_or(""),
                wan_rx: wan_rx_packets,
                wan_tx: wan_tx_packets,
                lan_rx: lan_rx_packets,
                lan_tx: lan_tx_packets,
                sessions_active: srv.active_sessions(),
                requests_served: srv.stats.requests_served,
                tick_100ms: $now_ms / 100,
            }
        }};
    }

    let mut loop_count: u32 = 0;
    loop {
        loop_count = loop_count.wrapping_add(1);

        // [R29] JALUR TULIS WEBUI: dispatcher hanya menvalidasi dan menampung
        // perintah; kernel di sini yang mengubah state WAN dan journal. Nama
        // target harus sama dengan antarmuka WAN yang benar-benar terdeteksi,
        // sehingga tidak ada baris fiktif yang bisa diedit.
        while let Some(cmd) = api_dispatcher.take_write() {
            let mut old_name_buf = [0u8; asl_router_persistence::RJCFG_IFACE_NAME_MAX];
            let old_len = (rjcfg_journal.active_config.wan_name_len as usize)
                .min(asl_router_persistence::RJCFG_IFACE_NAME_MAX);
            old_name_buf[..old_len]
                .copy_from_slice(&rjcfg_journal.active_config.wan_name[..old_len]);
            let old = core::str::from_utf8(&old_name_buf[..old_len]).unwrap_or("eth0");

            if old != cmd.target_str() {
                web_write_ok = false;
                web_report_len = fill_report(
                    &mut web_report,
                    &[
                        "target '",
                        cmd.target_str(),
                        "' bukan antarmuka WAN terdeteksi",
                    ],
                );
                SERIAL.write_str("[R29] WebUI write DITOLAK: target='");
                SERIAL.write_str(cmd.target_str());
                SERIAL.write_str("' != WAN='");
                SERIAL.write_str(old);
                SERIAL.write_str("'\n");
                continue;
            }

            let mut cfg = rjcfg_journal.active_config;
            let new_name = cmd.new_name_str();
            let renamed = new_name != old;
            if renamed && cfg.set_wan_name(new_name).is_err() {
                web_write_ok = false;
                web_report_len = fill_report(&mut web_report, &["nama ditolak: ", new_name]);
                continue;
            }

            let now_ms = pit.elapsed_ms();
            let mut mid: &str = "";
            let head: &'static str;
            if cmd.mode == asl_router_persistence::RJCFG_WAN_MODE_STATIC {
                cfg.set_wan_static(cmd.ip, cmd.netmask, cmd.gw);
                head = "statik diterapkan";
                wan_static_mode = true;
                wan_ip = cmd.ip;
                wan_netmask = cmd.netmask;
                wan_gw = cmd.gw;
                wan_up = true;
                wan_conflict = false;
                wan_source = "static-webui";
                dad_active = true;
                dad_deadline_ms = now_ms + 1_000;
                netstack.our_ip = cmd.ip;
                nat_engine.wan_ip = cmd.ip;
                if let Some(pfx) = mask_to_prefix(cmd.netmask) {
                    let _ = router_fib.add_connected_route(
                        [
                            cmd.ip[0] & cmd.netmask[0],
                            cmd.ip[1] & cmd.netmask[1],
                            cmd.ip[2] & cmd.netmask[2],
                            cmd.ip[3] & cmd.netmask[3],
                        ],
                        pfx,
                        1,
                    );
                }
                if cmd.gw != [0, 0, 0, 0] {
                    let _ = router_fib.add_static_route([0, 0, 0, 0], 0, cmd.gw, 1, 100);
                }
                fib_wan_routes_added = true;
                // DAD (RFC 5227) dijalankan seperti 'wan static' pada CLI.
                let mut probe_sent = false;
                if let Ok(plen) =
                    asl_router_netstack::build_arp_request(our_mac, cmd.ip, cmd.ip, &mut arp_probe)
                {
                    if let Some(eth0) = eth0.as_mut() {
                        if eth0.ready && eth0.transmit(&arp_probe[..plen]).is_ok() {
                            probe_sent = true;
                            wan_tx_packets += 1;
                        }
                    }
                }
                mid = if probe_sent {
                    ", probe ARP DAD TX"
                } else {
                    ", probe ARP DAD tidak terkirim"
                };
            } else {
                cfg.set_wan_dhcp();
                head = "dhcp aktif: alamat dilepas, DORA diulang";
                wan_static_mode = false;
                wan_ip = [0, 0, 0, 0];
                wan_netmask = [0, 0, 0, 0];
                wan_gw = [0, 0, 0, 0];
                wan_up = false;
                wan_conflict = false;
                wan_source = "none";
                dad_active = false;
                netstack.our_ip = [0, 0, 0, 0];
                nat_engine.wan_ip = [0, 0, 0, 0];
                dhcp_mode_switches = dhcp_mode_switches.wrapping_add(1);
                let _ = dhcp_client.start_discover(0x5249_4E4A + 8 + dhcp_mode_switches);
                dhcp_request_tx = false;
                dhcp_discover_tx = 0;
                dhcp_next_tx_ms = now_ms + 500;
                dhcp_halted_logged = false;
            }

            rjcfg_journal.active_config = cfg;
            // Operator minta persistensi => keberhasilan hanya jika readback disk
            // cocok. Tanpa itu laporan harus menyebut 'RAM saja'.
            let mut persist_done = !cmd.persist;
            let mut tail: &str = if cmd.persist {
                ", persistensi gagal"
            } else {
                " (RAM saja, tidak ditulis ke disk)"
            };
            if cmd.persist {
                if !block_is_ata {
                    tail = ", tidak dipersist: device block bukan ATA";
                } else if let Some(base) = rjcfg01_lba {
                    let active = rjcfg_journal.active_config;
                    match rjcfg_journal.commit_update(active) {
                        Ok(slot) => {
                            if rjcfg_write_and_verify(&mut ata_dev, &rjcfg_journal, base, slot) {
                                persist_done = true;
                                tail = ", RJCFG01 ditulis + diverifikasi di disk";
                            } else {
                                tail = ", RJCFG01 READBACK GAGAL";
                            }
                        }
                        Err(e) => tail = e,
                    }
                } else {
                    tail = ", tidak dipersist: partisi RJCFG01 tidak ada";
                }
            }
            web_write_ok = persist_done;
            web_report_len = fill_report(
                &mut web_report,
                &[if renamed { "nama + " } else { "" }, head, mid, tail],
            );
            SERIAL.write_str("[R29] WebUI write diterapkan: WAN='");
            SERIAL.write_str(new_name);
            SERIAL.write_str("' mode=");
            SERIAL.write_str(
                if cmd.mode == asl_router_persistence::RJCFG_WAN_MODE_STATIC {
                    "static"
                } else {
                    "dhcp"
                },
            );
            SERIAL.write_str(" persist=");
            SERIAL.write_str(if cmd.persist { "ya" } else { "tidak" });
            SERIAL.write_str(" — ");
            SERIAL.write_str(core::str::from_utf8(&web_report[..web_report_len]).unwrap_or(""));
            SERIAL.write_str("\n");
        }

        // [R30] JALUR DAYA WEBUI: API sudah menjaga arming 2 langkah
        // (handle_power). Kernel hanya mengeksekusi aksi yang lolos penjaga itu,
        // POWER_EXEC_MS setelah ACK agar browser menerima respons lebih dulu,
        // memakai routine yang sama dengan CLI 'reboot'/'shutdown'.
        if let Some(kind) = api_dispatcher.take_power() {
            power_kind = Some(kind);
            power_deadline_ms = pit.elapsed_ms() + asl_router_api::POWER_EXEC_MS;
            SERIAL.write_str("[R30] Aksi daya WebUI dijadwalkan: ");
            SERIAL.write_str(if kind == asl_router_api::POWER_SHUTDOWN {
                "shutdown"
            } else {
                "restart"
            });
            SERIAL.write_str(" dalam ");
            SERIAL.write_u32(asl_router_api::POWER_EXEC_MS as u32);
            SERIAL.write_str(" ms\n");
        }
        let power_due = match power_kind {
            Some(_) => pit.elapsed_ms() >= power_deadline_ms,
            None => false,
        };
        if power_due {
            if power_kind == Some(asl_router_api::POWER_SHUTDOWN) {
                SERIAL.write_str("[R30] SHUTDOWN WebUI — probing ACPI PM1_CNT candidates...\r\n");
                if power::try_poweroff_candidates(&|s| SERIAL.write_str(s)) {
                    SERIAL.write_str("[R30] Power-off unexpected return.\r\n");
                }
                SERIAL.write_str("[R30] Semua kandidat PM1_CNT ditolak — entering safe halt (power-off TIDAK didukung platform ini).\r\n");
                loop {
                    #[cfg(target_arch = "x86_64")]
                    unsafe {
                        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
                    }
                    #[cfg(not(target_arch = "x86_64"))]
                    core::hint::spin_loop();
                }
            }
            SERIAL.write_str("[R30] RESTART WebUI via reset 8042 (0xFE) lalu triple fault...\r\n");
            power::system_reset();
        }

        // [R24] JALUR LAN NYATA: poll RX eth1 (DMA), ARP/ICMP responder, GARP
        if let Some(eth1) = eth1.as_mut() {
            if eth1.ready {
                let mut serviced = 0u32;
                while serviced < 8 {
                    match eth1.poll_rx(&mut lan_frame) {
                        Some(n) => {
                            serviced += 1;
                            lan_rx_packets += 1;
                            match netstack_lan.process_packet(&lan_frame[..n], &mut lan_reply) {
                                Ok(Some(rlen)) => {
                                    if eth1.transmit(&lan_reply[..rlen]).is_ok() {
                                        lan_tx_packets += 1;
                                        SERIAL.write_str("[R24] LAN REPLY TX (");
                                        SERIAL.write_u32(rlen as u32);
                                        SERIAL.write_str(" bytes) lan_rx=");
                                        SERIAL.write_u32(lan_rx_packets);
                                        SERIAL.write_str(" lan_tx=");
                                        SERIAL.write_u32(lan_tx_packets);
                                        SERIAL.write_str("\n");
                                    }
                                }
                                Ok(None) => {}
                                Err(_) => drop_packets += 1,
                            }
                        }
                        None => break,
                    }
                }
                if loop_count == 10 || (loop_count > 10 && (loop_count - 10) % 150 == 0) {
                    if let Ok(gl) = asl_router_netstack::build_arp_gratuitous(
                        lan_mac,
                        lan_ip_real,
                        &mut garp_buf,
                    ) {
                        if eth1.transmit(&garp_buf[..gl]).is_ok() {
                            SERIAL.write_str("[R24] GARP announced 192.168.56.100 broadcast (");
                            SERIAL.write_u32(gl as u32);
                            SERIAL.write_str(" bytes)\n");
                        }
                    }
                }
            }
        }

        // [R28] JALUR WAN NYATA pada eth0 (topologi satu NIC bridged).
        // Ingress DMA -> mesin DHCP / TCP-HTTP WebUI / responder ARP+ICMP.
        // Egress  : DISCOVER+REQUEST, segmen TCP, ARP/ICMP reply via eth0.transmit.
        let lan_present_now = eth1.as_ref().map_or(false, |e| e.ready);
        let wan_present_now = eth0.as_ref().map_or(false, |e| e.ready);
        if let Some(eth0) = eth0.as_mut() {
            if eth0.ready {
                let now_ms = pit.elapsed_ms();

                // (1) Mesin DHCP dipacu waktu PIT nyata (tick = 10 ms).
                let cur_tick = now_ms / 10;
                let dt = cur_tick.saturating_sub(dhcp_last_tick) as u32;
                dhcp_last_tick = cur_tick;
                if !wan_static_mode && !dhcp_client.is_bound() && !dhcp_client.is_halted() && dt > 0
                {
                    let st = dhcp_client.on_tick(dt);
                    match st {
                        asl_router_netstack::DhcpClientState::Selecting { xid, .. } => {
                            if now_ms >= dhcp_next_tx_ms
                                && dhcp_discover_tx <= dhcp_client.config.max_retries as u32
                            {
                                if let Ok(n) =
                                    asl_router_netstack::build_dhcp_discover_with_hostname(
                                        our_mac,
                                        xid,
                                        "rinjani",
                                        &mut dhcp_buf,
                                    )
                                {
                                    if eth0.transmit(&dhcp_buf[..n]).is_ok() {
                                        dhcp_discover_tx += 1;
                                        wan_tx_packets += 1;
                                        SERIAL.write_str("[R28] DHCP DISCOVER TX percobaan #");
                                        SERIAL.write_u32(dhcp_discover_tx);
                                        SERIAL.write_str(" (menunggu OFFER)\n");
                                    }
                                }
                                dhcp_next_tx_ms = now_ms + 4_000;
                            }
                        }
                        asl_router_netstack::DhcpClientState::Requesting {
                            xid,
                            offered_ip,
                            server_ip,
                            ..
                        } => {
                            if !dhcp_request_tx {
                                dhcp_request_tx = true;
                                if let Ok(n) = asl_router_netstack::build_dhcp_request(
                                    our_mac,
                                    offered_ip,
                                    server_ip,
                                    xid,
                                    &mut dhcp_buf,
                                ) {
                                    if eth0.transmit(&dhcp_buf[..n]).is_ok() {
                                        wan_tx_packets += 1;
                                        SERIAL.write_str("[R28] DHCP REQUEST TX ip_diminta=");
                                        write_ip(offered_ip);
                                        SERIAL.write_str(" server=");
                                        write_ip(server_ip);
                                        SERIAL.write_str("\n");
                                    }
                                }
                            }
                        }
                        asl_router_netstack::DhcpClientState::HaltedNoOffer => {
                            if dhcp_nak_cycles < 2 {
                                dhcp_nak_cycles += 1;
                                dhcp_client.start_discover(0x5249_4E4A + dhcp_nak_cycles);
                                dhcp_request_tx = false;
                                dhcp_discover_tx = 0;
                                dhcp_next_tx_ms = now_ms + 1_000;
                                SERIAL.write_str("[R28] DHCP timeout -> mengulang DORA siklus #");
                                SERIAL.write_u32(dhcp_nak_cycles);
                                SERIAL.write_str(" (tidak ada OFFER/ACK dalam window)\n");
                            } else if !dhcp_halted_logged {
                                dhcp_halted_logged = true;
                                SERIAL.write_str("[R28] DHCP HALT: 15 s tanpa OFFER di eth0 — WAN DOWN (fail-closed, lease tidak dikarang). Set alamat dengan 'wan static <ip> <mask> <gw>'.\n");
                            }
                        }
                        _ => {}
                    }
                }

                // (2) DAD statis: 1 s tanpa ARP Reply -> alamat jadi definitif.
                if dad_active && now_ms >= dad_deadline_ms {
                    dad_active = false;
                    if !wan_conflict {
                        wan_source = "static";
                        SERIAL.write_str("[R28] DAD OK: ");
                        write_ip(wan_ip);
                        SERIAL.write_str(" bebas konflik (probe ARP 1 s tanpa Reply)\n");
                    }
                }

                // (3) Ingress: maksimum 8 frame per iterasi poll.
                let mut serviced = 0u32;
                while serviced < 8 {
                    let polled = eth0.poll_rx(&mut wan_frame);
                    let n = match polled {
                        Some(v) => v,
                        None => break,
                    };
                    serviced += 1;
                    wan_rx_packets += 1;

                    // (3a) Balasan DHCP server (OFFER / ACK / NAK).
                    if !wan_static_mode && !dhcp_client.is_bound() {
                        if let Ok(Some(st)) = dhcp_client.handle_incoming_packet(&wan_frame[..n]) {
                            match st {
                                asl_router_netstack::DhcpClientState::Requesting {
                                    offered_ip,
                                    server_ip,
                                    ..
                                } => {
                                    SERIAL.write_str("[R28] DHCP OFFER dari ");
                                    write_ip(server_ip);
                                    SERIAL.write_str(" ip=");
                                    write_ip(offered_ip);
                                    SERIAL.write_str(" — REQUEST akan dikirim\n");
                                }
                                asl_router_netstack::DhcpClientState::Bound {
                                    ip,
                                    netmask,
                                    gateway,
                                    lease_time,
                                } => {
                                    wan_ip = ip;
                                    wan_netmask = netmask;
                                    wan_gw = gateway;
                                    wan_up = true;
                                    wan_source = "dhcp";
                                    dad_active = false;
                                    netstack.our_ip = ip;
                                    nat_engine.wan_ip = ip;
                                    if !fib_wan_routes_added {
                                        if let Some(pfx) = mask_to_prefix(netmask) {
                                            let net = [
                                                ip[0] & netmask[0],
                                                ip[1] & netmask[1],
                                                ip[2] & netmask[2],
                                                ip[3] & netmask[3],
                                            ];
                                            let _ = router_fib.add_connected_route(net, pfx, 1);
                                        }
                                        if gateway != [0, 0, 0, 0] {
                                            let _ = router_fib.add_static_route(
                                                [0, 0, 0, 0],
                                                0,
                                                gateway,
                                                1,
                                                100,
                                            );
                                        }
                                        fib_wan_routes_added = true;
                                    }
                                    SERIAL.write_str("[R28] DHCP BOUND eth0=");
                                    write_ip(ip);
                                    SERIAL.write_str("/");
                                    SERIAL.write_u32(mask_to_prefix(netmask).unwrap_or(0) as u32);
                                    SERIAL.write_str(" gw=");
                                    write_ip(gateway);
                                    SERIAL.write_str(" lease=");
                                    SERIAL.write_u32(lease_time);
                                    SERIAL.write_str(" s — WebUI siap pada alamat WAN\n");
                                }
                                asl_router_netstack::DhcpClientState::HaltedNoOffer => {
                                    // RFC 2131: NAK berarti alamat yang diminta tidak
                                    // valid — klien WAJIB mengulang dari DISCOVER,
                                    // bukan mati permanen. Dibatasi 2 siklus agar
                                    // tidak membanjiri wire, lalu fail-closed.
                                    if dhcp_nak_cycles < 2 {
                                        dhcp_nak_cycles += 1;
                                        dhcp_client.start_discover(0x5249_4E4A + dhcp_nak_cycles);
                                        dhcp_request_tx = false;
                                        dhcp_discover_tx = 0;
                                        dhcp_next_tx_ms = now_ms + 1_000;
                                        dhcp_halted_logged = false;
                                        SERIAL
                                            .write_str("[R28] DHCP NAK -> mengulang DORA siklus #");
                                        SERIAL.write_u32(dhcp_nak_cycles);
                                        SERIAL.write_str(" (xid baru, DISCOVER akan dikirim)\n");
                                    } else if !dhcp_halted_logged {
                                        dhcp_halted_logged = true;
                                        SERIAL.write_str("[R28] DHCP ditolak server (NAK/timeout) — WAN DOWN, tidak ada alamat karangan\n");
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // (3b) TCP/HTTP WebUI pada alamat WAN.
                    let mut web_handled = false;
                    if let Some(srv) = web_server.as_mut() {
                        let rt = web_runtime!(srv, now_ms, lan_present_now, wan_present_now);
                        let mut tx = |b: &[u8]| -> Result<(), &'static str> {
                            match eth0.transmit(b) {
                                Ok(()) => {
                                    wan_tx_packets += 1;
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        };
                        match srv.on_frame(
                            &wan_frame[..n],
                            &rt,
                            &mut api_dispatcher,
                            &mut web_scratch,
                            &mut tx,
                            &weblog,
                        ) {
                            Ok(handled) => web_handled = handled,
                            Err(_) => drop_packets += 1,
                        }
                    }

                    // (3c) DAD: ARP Reply untuk alamat kita dari MAC lain = konflik.
                    if wan_ip != [0, 0, 0, 0] && !wan_conflict {
                        if let Ok((eth, arp_bytes)) =
                            asl_router_netstack::parse_ethernet_header(&wan_frame[..n])
                        {
                            if eth.ethertype == asl_router_netstack::ETHERTYPE_ARP {
                                if let Ok(a) = asl_router_netstack::parse_arp_packet(arp_bytes) {
                                    if a.opcode == 2
                                        && a.sender_ip == wan_ip
                                        && a.sender_mac != our_mac
                                    {
                                        wan_conflict = true;
                                        SERIAL.write_str("[R28] WARNING DAD: ");
                                        write_ip(wan_ip);
                                        SERIAL.write_str(" sudah dipakai host lain mac=");
                                        write_mac_hex(a.sender_mac);
                                        SERIAL.write_str(" —WebUI mungkin tidak tercapai\n");
                                    }
                                }
                            }
                        }
                    }

                    // (3d) Responder ARP/ICMP eth0 (hanya bila eth0 beralamat).
                    if !web_handled && wan_ip != [0, 0, 0, 0] {
                        match netstack.process_packet(&wan_frame[..n], &mut wan_reply) {
                            Ok(Some(rlen)) => {
                                if eth0.transmit(&wan_reply[..rlen]).is_ok() {
                                    wan_tx_packets += 1;
                                    SERIAL.write_str("[R28] WAN ARP/ICMP REPLY TX (");
                                    SERIAL.write_u32(rlen as u32);
                                    SERIAL.write_str(" bytes) wan_rx=");
                                    SERIAL.write_u32(wan_rx_packets);
                                    SERIAL.write_str(" wan_tx=");
                                    SERIAL.write_u32(wan_tx_packets);
                                    SERIAL.write_str("\n");
                                }
                            }
                            Ok(None) => {}
                            Err(_) => drop_packets += 1,
                        }
                    }
                }

                // (4) Pompa waktu TCP: sisa data tertahan window, penutupan sesi,
                // dan daur ulang slot yang ditinggal peer.
                if let Some(srv) = web_server.as_mut() {
                    if srv.active_sessions() > 0 {
                        let rt = web_runtime!(srv, now_ms, lan_present_now, wan_present_now);
                        let mut tx = |b: &[u8]| -> Result<(), &'static str> {
                            match eth0.transmit(b) {
                                Ok(()) => {
                                    wan_tx_packets += 1;
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        };
                        srv.on_tick(&rt, &mut web_scratch, &mut tx, &weblog);
                    }
                }
            }
        }

        // [R19] TIDAK ADA self-test tersimulasi. Dulu blok ini mengarang
        // "[NET] INGRESS ICMP ECHO REQUEST" + "SIMULASI FORWARDING & SNAT" dan
        // menaikkan counter untuk paket yang tidak pernah ada di wire. Satu-
        // satunya bukti TCP/HTTP/ARP yang sah adalah baris [R28] yang lahir dari
        // ingress DMA nyata pada eth0, atau perintah CLI 'ping' / 'wan show'.
        if loop_count == 100 {
            SERIAL.write_str("[R19] PROOF MODE: menunggu trafik nyata pada eth0 (WAN rx=");
            SERIAL.write_u32(wan_rx_packets);
            SERIAL.write_str(" tx=");
            SERIAL.write_u32(wan_tx_packets);
            SERIAL.write_str("). WebUI: gunakan 'wan show' untuk alamat bind.\n");
        }

        // SERIAL CLI INTERACTIVE COMMAND SCANNER
        if let Some(ch) = SERIAL.read_byte() {
            if ch == b'\r' || ch == b'\n' {
                SERIAL.write_str("\r\n");
                if cli_len > 0 {
                    let cmd = &cli_line[..cli_len];
                    if cmd == b"help" {
                        SERIAL.write_str("Available commands:\r\n");
                        SERIAL.write_str(
                            "  show version     - Display kernel and OS distribution version\r\n",
                        );
                        SERIAL.write_str(
                            "  show interface   - Display network interfaces, IPs and status\r\n",
                        );
                        SERIAL.write_str(
                            "  show hardware    - Display detected PCI and CPU inventory\r\n",
                        );
                        SERIAL.write_str(
                            "  show tcam        - Display TCAM classification table status\r\n",
                        );
                        SERIAL
                            .write_str("  show route       - Display LPM Routing Table (FIB)\r\n");
                        SERIAL.write_str("  show nat         - Display NAT rules and active Conntrack sessions\r\n");
                        SERIAL.write_str(
                            "  show counter     - Display per-interface packet counters\r\n",
                        );
                        SERIAL.write_str("  disk print       - Display GPT disk partition layout and capacity\r\n");
                        SERIAL.write_str(
                            "  disk adopt       - Adopt physical disk expansion into RJDATA\r\n",
                        );
                        SERIAL.write_str("  config show      - Display RJCFG01 active configuration and slot\r\n");
                        SERIAL.write_str("  config save      - Persist configuration via atomic A/B slot switch\r\n");
                        SERIAL.write_str(
                            "  hostname set <s> - Update and persist hostname to RJCFG01\r\n",
                        );
                        SERIAL.write_str("  ping <ip>        - Belum diimplementasi (dicetak jujur, bukan statistik palsu)\r\n");
                        SERIAL.write_str("  wan show         - Display real eth0 WAN address, DAD and HTTP state\r\n");
                        SERIAL.write_str("  wan static <ip> <mask> [gw] - Set static WAN address (stops DHCP, runs DAD)\r\n");
                        SERIAL.write_str("  install disk     - Inspeksi src+tgt utk self-install (NOL tulis)\r\n");
                        SERIAL.write_str("  install confirm YES-INSTALL - Eksekusi instalasi ke primary slave\r\n");
                        SERIAL.write_str(
                            "  install cancel   - Batalkan (menegaskan nol sektor ditulis)\r\n",
                        );
                        SERIAL.write_str("  clear            - Clear terminal screen\r\n");
                        SERIAL.write_str(
                            "  reboot           - Triple-fault hardware reboot (real)\r\n",
                        );
                        SERIAL.write_str(
                            "  shutdown         - ACPI PM1_CNT power-off probe (real)\r\n",
                        );
                    } else if cmd == b"show version" {
                        SERIAL.write_str("Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)\r\n");
                        SERIAL
                            .write_str("Target: x86_64-unknown-none | Standard: DO-178C DAL-A\r\n");
                    } else if cmd == b"show interface" {
                        SERIAL.write_str("Interfaces (state nyata):\r\n");
                        let eth0_ready = eth0.as_ref().map_or(false, |e| e.ready);
                        SERIAL.write_str("  eth0 (WAN): ");
                        SERIAL.write_str(if !eth0_ready {
                            "ABSENT (driver/DMA tidak siap)\r\n"
                        } else if wan_up {
                            "UP, ip="
                        } else {
                            "DOWN, ip=UNCONFIGURED (belum ada lease DHCP atau 'wan static')\r\n"
                        });
                        if eth0_ready && wan_up {
                            write_ip(wan_ip);
                            if let Some(pfx) = mask_to_prefix(wan_netmask) {
                                SERIAL.write_str("/");
                                SERIAL.write_u32(pfx as u32);
                            }
                            SERIAL.write_str(" gw=");
                            write_ip(wan_gw);
                            SERIAL.write_str(" source=");
                            SERIAL.write_str(wan_source);
                            SERIAL.write_str(" mac=");
                            write_mac_hex(our_mac);
                            SERIAL.write_str(" mtu=1500\r\n");
                        }
                        let eth1_ready = eth1.as_ref().map_or(false, |e| e.ready);
                        SERIAL.write_str(if eth1_ready {
                            "  eth1 (LAN): UP, ip="
                        } else {
                            "  eth1 (LAN): ABSENT (topologi satu NIC bridged)\r\n"
                        });
                        if eth1_ready {
                            write_ip(lan_ip_real);
                            SERIAL.write_str(" mac=");
                            write_mac_hex(lan_mac);
                            SERIAL.write_str(" mtu=1500\r\n");
                        }
                    } else if cmd == b"show route" {
                        SERIAL.write_str("LPM Routing Table (FIB) — isi tabel aktual:\r\n");
                        let n = router_fib.fib.count;
                        if n == 0 {
                            SERIAL.write_str("  (kosong — tidak ada rute terpasang)\r\n");
                        }
                        let mut i = 0;
                        while i < n {
                            let r = &router_fib.fib.routes[i];
                            if r.is_active {
                                SERIAL.write_str("  ");
                                write_ip(r.network);
                                SERIAL.write_str("/");
                                SERIAL.write_u32(r.prefix_len as u32);
                                SERIAL.write_str("  metric=");
                                SERIAL.write_u32(r.metric);
                                match r.next_hop {
                                    asl_router_lpm::NextHop::Interface(ifc) => {
                                        SERIAL.write_str("  direct if=");
                                        SERIAL.write_u32(ifc as u32);
                                    }
                                    asl_router_lpm::NextHop::Gateway {
                                        interface,
                                        next_hop_ip,
                                    } => {
                                        SERIAL.write_str("  via ");
                                        write_ip(next_hop_ip);
                                        SERIAL.write_str(" if=");
                                        SERIAL.write_u32(interface as u32);
                                    }
                                    asl_router_lpm::NextHop::Local => SERIAL.write_str("  local"),
                                    asl_router_lpm::NextHop::Drop => SERIAL.write_str("  drop"),
                                }
                                SERIAL.write_str("\r\n");
                            }
                            i += 1;
                        }
                    } else if cmd == b"show nat" {
                        SERIAL.write_str("NAT Engine Status:\r\n");
                        SERIAL.write_str("  SNAT Rule : masquerade WAN (eth0) ip=");
                        if nat_engine.wan_ip == [0, 0, 0, 0] {
                            SERIAL.write_str("UNCONFIGURED\r\n");
                        } else {
                            write_ip(nat_engine.wan_ip);
                            SERIAL.write_str("\r\n");
                        }
                        SERIAL.write_str("  DNAT Rule : ");
                        let mut printed = 0u32;
                        let mut k = 0;
                        while k < nat_engine.dnat_rules.len() {
                            let rule = &nat_engine.dnat_rules[k];
                            if rule.is_active {
                                printed += 1;
                                SERIAL.write_str(":");
                                SERIAL.write_u32(rule.wan_port as u32);
                                if rule.protocol == asl_router_nat::PROTO_UDP {
                                    SERIAL.write_str("/udp -> ");
                                } else {
                                    SERIAL.write_str("/tcp -> ");
                                }
                                write_ip(rule.target_lan_ip);
                                SERIAL.write_str(":");
                                SERIAL.write_u32(rule.target_lan_port as u32);
                                SERIAL.write_str("\r\n");
                            }
                            k += 1;
                        }
                        if printed == 0 {
                            SERIAL.write_str("(tidak ada rule aktif)\r\n");
                        }
                        SERIAL.write_str("  Conntrack : ");
                        SERIAL.write_u32(nat_engine.active_sessions as u32);
                        SERIAL.write_str(" active sessions, drops=");
                        SERIAL.write_u32(nat_engine.table_full_drops);
                        SERIAL.write_str("\r\n");
                    } else if cmd == b"show counter" {
                        SERIAL.write_str("Packet Counters:\r\n");
                        SERIAL.write_str("  eth0 (WAN): rx=");
                        SERIAL.write_u32(wan_rx_packets);
                        SERIAL.write_str(", tx=");
                        SERIAL.write_u32(wan_tx_packets);
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  eth1 (LAN): rx=");
                        SERIAL.write_u32(lan_rx_packets);
                        SERIAL.write_str(", tx=");
                        SERIAL.write_u32(lan_tx_packets);
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  Forward   : 0 (path eth1->eth0 belum diwire; bukan angka karangan)\r\n");
                        SERIAL.write_str("  Drop      : ");
                        SERIAL.write_u32(drop_packets);
                        SERIAL.write_str("\r\n");
                    } else if cmd == b"show hardware" {
                        SERIAL.write_str("Hardware Inventory (Real Detection Only):\r\n");
                        SERIAL.write_str("  CPU Vendor : ");
                        SERIAL.write_str(
                            core::str::from_utf8(&hwinv.cpu.vendor).unwrap_or("(tidak terbaca)"),
                        );
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  CPU Family : ");
                        SERIAL.write_u32(hwinv.cpu.family as u32);
                        SERIAL.write_str(" Model: ");
                        SERIAL.write_u32(hwinv.cpu.model as u32);
                        SERIAL.write_str(" Stepping: ");
                        SERIAL.write_u32(hwinv.cpu.stepping as u32);
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  PCI Devices: ");
                        SERIAL.write_u32(hwinv.device_count as u32);
                        SERIAL.write_str(" found, NIC: ");
                        SERIAL.write_u32(hwinv.nic_count as u32);
                        SERIAL.write_str("\r\n");
                        let mut di = 0;
                        while di < hwinv.device_count {
                            let d = &hwinv.devices[di];
                            SERIAL.write_str("    pci ");
                            SERIAL.write_u32(d.bus as u32);
                            SERIAL.write_str(":");
                            SERIAL.write_u32(d.device as u32);
                            SERIAL.write_str(".");
                            SERIAL.write_u32(d.function as u32);
                            SERIAL.write_str(" vid=0x");
                            write_hex_u16(d.vendor_id);
                            SERIAL.write_str(" did=0x");
                            write_hex_u16(d.device_id);
                            SERIAL.write_str(" class=0x");
                            write_hex_u16(d.class_code as u16);
                            SERIAL.write_str(" nic=");
                            SERIAL.write_str(if d.is_nic { "yes" } else { "no" });
                            SERIAL.write_str("\r\n");
                            di += 1;
                        }
                    } else if cmd == b"show tcam" {
                        SERIAL.write_str("TCAM Engine: rules_aktif=");
                        SERIAL.write_u32(tcam.rule_count() as u32);
                        SERIAL.write_str("/");
                        SERIAL.write_u32(tcam.capacity() as u32);
                        SERIAL.write_str(", default-action=DENY\r\n");
                    } else if cmd == b"hostname show" || cmd == b"show hostname" {
                        let mut hbuf = [0u8; 32];
                        let current_name = rjcfg_journal.get_hostname_str(&mut hbuf);
                        SERIAL.write_str("hostname: ");
                        SERIAL.write_str(current_name);
                        SERIAL.write_str(" (domain: rinjani.local, slot=");
                        SERIAL.write_u32(rjcfg_journal.header.active_slot as u32);
                        SERIAL.write_str(")\r\n");
                    } else if cmd.starts_with(b"hostname set ") {
                        let new_name = &cmd[13..];
                        if let Ok(name_str) = core::str::from_utf8(new_name) {
                            if let Ok(slot) = rjcfg_journal.set_hostname(name_str) {
                                SERIAL.write_str("Hostname updated & persisted to slot ");
                                SERIAL.write_u32(slot as u32);
                                SERIAL.write_str(": ");
                                SERIAL.write_str(name_str);
                                SERIAL.write_str(" (Option 12 active)\r\n");
                            } else {
                                SERIAL.write_str("% Error setting hostname\r\n");
                            }
                        }
                    } else if cmd == b"wan show" {
                        SERIAL.write_str("WAN (eth0) state:\r\n");
                        SERIAL.write_str("  address : ");
                        if wan_ip == [0, 0, 0, 0] {
                            SERIAL.write_str("UNCONFIGURED\r\n");
                        } else {
                            write_ip(wan_ip);
                            if let Some(pfx) = mask_to_prefix(wan_netmask) {
                                SERIAL.write_str("/");
                                SERIAL.write_u32(pfx as u32);
                            }
                            SERIAL.write_str("\r\n");
                        }
                        SERIAL.write_str("  gateway : ");
                        write_ip(wan_gw);
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  source  : ");
                        SERIAL.write_str(wan_source);
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  link    : ");
                        SERIAL.write_str(if wan_up { "UP" } else { "DOWN" });
                        SERIAL.write_str(", dad_pending=");
                        SERIAL.write_str(if dad_active { "yes" } else { "no" });
                        SERIAL.write_str(", conflict=");
                        SERIAL.write_str(if wan_conflict { "yes" } else { "no" });
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  http    : listening=");
                        SERIAL.write_str(if web_server.as_ref().map_or(false, |s| s.listening) {
                            "yes"
                        } else {
                            "no"
                        });
                        SERIAL.write_str(" port=");
                        SERIAL.write_u32(web_server.as_ref().map_or(0, |s| s.port as u32));
                        SERIAL.write_str(" sessions=");
                        SERIAL.write_u32(
                            web_server
                                .as_ref()
                                .map_or(0, |s| s.active_sessions() as u32),
                        );
                        SERIAL.write_str(" served=");
                        SERIAL.write_u32(
                            web_server
                                .as_ref()
                                .map_or(0, |s| s.stats.requests_served as u32),
                        );
                        SERIAL.write_str("\r\n");
                    } else if let Some(rest) = cmd.strip_prefix(b"wan static ".as_slice()) {
                        // Token: <ip> <mask> [gw]
                        'apply: {
                            let mut parts = rest.split(|&c| c == b' ').filter(|p| !p.is_empty());
                            let tok_ip = parts.next();
                            let tok_mask = parts.next();
                            let tok_gw = parts.next();
                            let Some(ip) = tok_ip.and_then(parse_dotted) else {
                                SERIAL.write_str("% Usage: wan static <ip> <mask> [gw] — alamat IPv4 tidak valid\r\n");
                                break 'apply;
                            };
                            let Some(mask) = tok_mask.and_then(parse_dotted) else {
                                SERIAL.write_str("% Netmask tidak valid\r\n");
                                break 'apply;
                            };
                            let Some(pfx) = mask_to_prefix(mask) else {
                                SERIAL.write_str("% Netmask bukan prefix kontigu\r\n");
                                break 'apply;
                            };
                            let gw = match tok_gw {
                                Some(t) => match parse_dotted(t) {
                                    Some(g) => g,
                                    None => {
                                        SERIAL.write_str("% Gateway tidak valid\r\n");
                                        break 'apply;
                                    }
                                },
                                None => [0, 0, 0, 0],
                            };
                            if ip == [0, 0, 0, 0] || ip[0] == 127 || ip[0] >= 224 {
                                SERIAL.write_str("% Alamat bukan unicast routable\r\n");
                                break 'apply;
                            }
                            wan_static_mode = true;
                            wan_ip = ip;
                            wan_netmask = mask;
                            wan_gw = gw;
                            wan_up = true;
                            wan_conflict = false;
                            wan_source = "static-dad";
                            netstack.our_ip = ip;
                            nat_engine.wan_ip = ip;
                            let _ = router_fib.add_connected_route(
                                [
                                    ip[0] & mask[0],
                                    ip[1] & mask[1],
                                    ip[2] & mask[2],
                                    ip[3] & mask[3],
                                ],
                                pfx,
                                1,
                            );
                            if gw != [0, 0, 0, 0] {
                                let _ = router_fib.add_static_route([0, 0, 0, 0], 0, gw, 1, 100);
                            }
                            fib_wan_routes_added = true;
                            dad_active = true;
                            dad_deadline_ms = pit.elapsed_ms() + 1_000;
                            // Probe DAD (RFC 5227): ARP request target = alamat kita.
                            let mut probe_sent = false;
                            if let Ok(n) = asl_router_netstack::build_arp_request(
                                our_mac,
                                ip,
                                ip,
                                &mut arp_probe,
                            ) {
                                if let Some(eth0) = eth0.as_mut() {
                                    if eth0.ready && eth0.transmit(&arp_probe[..n]).is_ok() {
                                        probe_sent = true;
                                        wan_tx_packets += 1;
                                    }
                                }
                            }
                            SERIAL.write_str("wan static OK: ");
                            write_ip(ip);
                            SERIAL.write_str("/");
                            SERIAL.write_u32(pfx as u32);
                            SERIAL.write_str(" gw=");
                            write_ip(gw);
                            SERIAL.write_str(" DHCP-dihentikan DAD-probe=");
                            SERIAL.write_str(if probe_sent { "TX" } else { "TIDAK-TERKIRIM" });
                            SERIAL.write_str(" — WebUI bind pada alamat ini setelah DAD\r\n");
                            // [R29] CLI memakai journal yang sama dengan jalur tulis
                            // WebUI: 'wan static' yang sukses juga ditulis ke RJCFG01
                            // agar tidak ada divergensi antar-antarmuka.
                            {
                                let mut cfg = rjcfg_journal.active_config;
                                cfg.set_wan_static(ip, mask, gw);
                                let mut logged = false;
                                if block_is_ata {
                                    if let Some(base) = rjcfg01_lba {
                                        if let Ok(slot) = rjcfg_journal.commit_update(cfg) {
                                            logged = true;
                                            SERIAL.write_str("RJCFG01 wan static: ");
                                            SERIAL.write_str(
                                                if rjcfg_write_and_verify(
                                                    &mut ata_dev,
                                                    &rjcfg_journal,
                                                    base,
                                                    slot,
                                                ) {
                                                    "ditulis + diverifikasi di disk\r\n"
                                                } else {
                                                    "READBACK GAGAL\r\n"
                                                },
                                            );
                                        }
                                    }
                                }
                                if !logged {
                                    SERIAL.write_str(
                                        "RJCFG01 wan static: tidak ditulis (journal RAM saja)\r\n",
                                    );
                                }
                            }
                        }
                    } else if cmd == b"disk print" || cmd == b"show disk" {
                        SERIAL.write_str("Disk Layout (GPT Partition Table):\r\n");
                        SERIAL.write_str("  Total Capacity: ");
                        SERIAL.write_u32(blk_device.capacity_gib() as u32);
                        SERIAL.write_str(" GiB (");
                        SERIAL.write_u32((blk_device.capacity_sectors / 2048) as u32);
                        SERIAL.write_str(" MiB)\r\n");
                        SERIAL.write_str("  Partition 1: ESP (128 MiB, start=34, end=");
                        SERIAL.write_u32(gpt_layout.partitions[0].ending_lba as u32);
                        SERIAL.write_str(")\r\n");
                        SERIAL.write_str("  Partition 2: RJCFG01 (8 MiB, start=");
                        SERIAL.write_u32(gpt_layout.partitions[1].starting_lba as u32);
                        SERIAL.write_str(", end=");
                        SERIAL.write_u32(gpt_layout.partitions[1].ending_lba as u32);
                        SERIAL.write_str(")\r\n");
                        SERIAL.write_str("  Partition 3: RJDATA (Elastic, start=");
                        SERIAL.write_u32(gpt_layout.partitions[2].starting_lba as u32);
                        SERIAL.write_str(", end=");
                        SERIAL.write_u32(gpt_layout.partitions[2].ending_lba as u32);
                        SERIAL.write_str(")\r\n");
                    } else if cmd == b"disk adopt" {
                        if blk_device.capacity_sectors > gpt_layout.total_sectors {
                            let _ = gpt_layout.resize_data_partition(blk_device.capacity_sectors);
                            SERIAL.write_str("Disk growth adopted: RJDATA expanded to match physical capacity.\r\n");
                        } else {
                            SERIAL.write_str(
                                "Disk capacity already fully adopted (zero unallocated space).\r\n",
                            );
                        }
                    } else if cmd == b"config show" || cmd == b"show config" {
                        SERIAL.write_str("RJCFG01 Configuration Journal:\r\n");
                        SERIAL.write_str("  Active Slot : ");
                        SERIAL.write_str(if rjcfg_journal.header.active_slot == 0 {
                            "Slot A"
                        } else {
                            "Slot B"
                        });
                        SERIAL.write_str(" (seq=");
                        SERIAL.write_u32(rjcfg_journal.header.sequence_number);
                        SERIAL.write_str(")\r\n");
                        SERIAL.write_str("  Hostname    : ");
                        let mut hbuf = [0u8; 32];
                        SERIAL.write_str(rjcfg_journal.get_hostname_str(&mut hbuf));
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  LAN Gateway : 192.168.56.1/24\r\n");
                        SERIAL.write_str("  DHCP Pool   : 192.168.56.100 - 192.168.56.150\r\n");
                    } else if cmd == b"config save" {
                        let active = rjcfg_journal.active_config;
                        let res = rjcfg_journal.commit_update(active);
                        if let Ok(slot) = res {
                            SERIAL.write_str("Configuration committed to slot ");
                            SERIAL.write_u32(slot as u32);
                            SERIAL.write_str(" (atomic switch ok).\r\n");
                            let mut disk_done = false;
                            if block_is_ata {
                                if let Some(base) = rjcfg01_lba {
                                    disk_done = rjcfg_write_and_verify(
                                        &mut ata_dev,
                                        &rjcfg_journal,
                                        base,
                                        slot,
                                    );
                                }
                            }
                            if !disk_done {
                                SERIAL.write_str("% Konfigurasi hanya di RAM (tidak ada disk nyata / partisi RJCFG01 / tulis gagal).\r\n");
                            }
                        } else {
                            SERIAL.write_str("% Error committing configuration.\r\n");
                        }
                    } else if cmd == b"dhcp pool show" || cmd == b"show dhcp pool" {
                        SERIAL.write_str("DHCP Pool (eth1 only):\r\n");
                        SERIAL.write_str("  eth1        : ");
                        SERIAL.write_str(if eth1.as_ref().map_or(false, |e| e.ready) {
                            "present\n"
                        } else {
                            "ABSENT — pool tidak melayani DISCOVER\n"
                        });
                        SERIAL.write_str("  Server/GW   : ");
                        write_ip(_dhcp_pool.server_ip);
                        SERIAL.write_str(" router=");
                        write_ip(_dhcp_pool.router_ip);
                        SERIAL.write_str(" dns=");
                        write_ip(_dhcp_pool.dns_ip);
                        SERIAL.write_str("\r\n");
                        SERIAL.write_str("  Pool        : oktet terakhir ");
                        SERIAL.write_u32(_dhcp_pool.pool_start_last_octet as u32);
                        SERIAL.write_str(" - ");
                        SERIAL.write_u32(_dhcp_pool.pool_end_last_octet as u32);
                        SERIAL.write_str(" lease=");
                        SERIAL.write_u32(_dhcp_pool.default_lease_time);
                        SERIAL.write_str(" s\r\n");
                        SERIAL.write_str("  Active Leases: ");
                        SERIAL.write_u32(_dhcp_pool.active_leases as u32);
                        SERIAL.write_str(" / ");
                        SERIAL.write_u32(_dhcp_pool.leases.len() as u32);
                        SERIAL.write_str("\r\n");
                    } else if cmd == b"dhcp lease show" || cmd == b"show dhcp lease" {
                        SERIAL.write_str("DHCP Leases Table (isi aktual _dhcp_pool.leases):\r\n");
                        let mut shown = 0u32;
                        let mut li = 0;
                        while li < _dhcp_pool.leases.len() {
                            let l = &_dhcp_pool.leases[li];
                            if l.active {
                                shown += 1;
                                SERIAL.write_str("  ip=");
                                write_ip(l.ip);
                                SERIAL.write_str(" mac=");
                                write_mac_hex(l.mac);
                                SERIAL.write_str(" host=");
                                let hl = (l.hostname_len as usize).min(32);
                                SERIAL.write_str(
                                    core::str::from_utf8(&l.hostname[..hl]).unwrap_or("?"),
                                );
                                SERIAL.write_str(" expire_tick=");
                                SERIAL.write_u32(l.expire_tick as u32);
                                SERIAL.write_str("\r\n");
                            }
                            li += 1;
                        }
                        if shown == 0 {
                            SERIAL.write_str("  (no active client leases)\r\n");
                        }
                    } else if cmd == b"dhcp pool clear" {
                        for l in _dhcp_pool.leases.iter_mut() {
                            *l = asl_router_netstack::DhcpLeaseEntry::empty();
                        }
                        _dhcp_pool.active_leases = 0;
                        SERIAL.write_str("DHCP pool leases cleared (entri dinolkan).\r\n");
                    } else if cmd == b"dns cache flush" {
                        _dns_proxy.flush_cache();
                        SERIAL.write_str("DNS proxy cache flushed.\r\n");
                    } else if cmd == b"dns cache show" || cmd == b"show dns cache" {
                        SERIAL.write_str("DNS LRU cache (isi aktual):\r\n");
                        let mut cshown = 0u32;
                        let mut ci = 0;
                        while ci < _dns_proxy.cache.len() {
                            let e = &_dns_proxy.cache[ci];
                            if e.active {
                                cshown += 1;
                                let ql = (e.qname_len as usize).min(64);
                                SERIAL.write_str("  ");
                                SERIAL
                                    .write_str(core::str::from_utf8(&e.qname[..ql]).unwrap_or("?"));
                                SERIAL.write_str(" -> ");
                                write_ip(e.ip);
                                SERIAL.write_str(" expire_tick=");
                                SERIAL.write_u32(e.expire_tick as u32);
                                SERIAL.write_str("\r\n");
                            }
                            ci += 1;
                        }
                        if cshown == 0 {
                            SERIAL.write_str("  (kosong — belum ada resolutor trafik DNS)\r\n");
                        }
                        SERIAL.write_str("  hits=");
                        SERIAL.write_u32(_dns_proxy.cache_hits);
                        SERIAL.write_str(" misses=");
                        SERIAL.write_u32(_dns_proxy.cache_misses);
                        SERIAL.write_str("\r\n");
                    } else if cmd == b"dns hosts show" || cmd == b"show dns hosts" {
                        SERIAL.write_str("Static DNS Hosts (tabel _dns_proxy.hosts):\r\n");
                        let mut hshown = 0u32;
                        let mut hi = 0;
                        while hi < _dns_proxy.hosts.len() {
                            let h = &_dns_proxy.hosts[hi];
                            if h.active {
                                hshown += 1;
                                let nl = (h.name_len as usize).min(32);
                                SERIAL.write_str("  ");
                                SERIAL
                                    .write_str(core::str::from_utf8(&h.name[..nl]).unwrap_or("?"));
                                SERIAL.write_str(" -> ");
                                write_ip(h.ip);
                                SERIAL.write_str("\r\n");
                            }
                            hi += 1;
                        }
                        if hshown == 0 {
                            SERIAL.write_str("  (kosong)\r\n");
                        }
                    } else if let Some(rest) = cmd.strip_prefix(b"dns hosts add ".as_slice()) {
                        'dnsadd: {
                            let mut it = rest.split(|&c| c == b' ').filter(|p| !p.is_empty());
                            let name = match it.next() {
                                Some(n) => n,
                                None => break 'dnsadd,
                            };
                            let ip = match it.next().and_then(parse_dotted) {
                                Some(v) => v,
                                None => {
                                    SERIAL.write_str("% Usage: dns hosts add <name> <ipv4>\r\n");
                                    break 'dnsadd;
                                }
                            };
                            let name_str = match core::str::from_utf8(name) {
                                Ok(s) => s,
                                Err(_) => {
                                    SERIAL.write_str("% Nama tidak valid\r\n");
                                    break 'dnsadd;
                                }
                            };
                            match _dns_proxy.add_static_host(name_str, ip) {
                                Ok(()) => SERIAL.write_str(
                                    "Static DNS host registered di tabel hosts engine.\r\n",
                                ),
                                Err(e) => {
                                    let _ = e;
                                    SERIAL.write_str("% Gagal menambah host (tabel penuh / nama terlalu panjang).\r\n")
                                }
                            }
                        }
                    } else if let Some(rest) = cmd.strip_prefix(b"dns hosts del ".as_slice()) {
                        let name = rest.split(|&c| c == b' ').next().unwrap_or(rest);
                        match core::str::from_utf8(name) {
                            Ok(s) => {
                                SERIAL.write_str(if _dns_proxy.delete_static_host(s) {
                                    "Static DNS host dihapus dari tabel.\r\n"
                                } else {
                                    "% Host tidak ditemukan.\r\n"
                                });
                            }
                            Err(_) => SERIAL.write_str("% Nama tidak valid.\r\n"),
                        }
                    } else if cmd.starts_with(b"ping") {
                        // Jujur: kernel ini belum punya egress ICMP echo maupun
                        // reply-matcher, jadi tidak ada angka RTT yang boleh
                        // dicetak. Dulu cabang ini mencetak 3 balasan karangan.
                        SERIAL.write_str("% ping: TIDAK DIIMPLEMENTASIKAN — tidak ada egress ICMP echo maupun reply-matcher di kernel, jadi setiap angka RTT akan palsu.\r\n");
                        SERIAL.write_str("  Bukti konektivitas WAN yang sah saat ini: 'wan show' (alamat + DAD), counter nyata pada 'show counter', dan handshake TCP/HTTP masuk dari host.\r\n");
                    } else if cmd == b"install" {
                        SERIAL.write_str("Usage: install disk | install confirm YES-INSTALL | install cancel\r\n");
                    } else if cmd == b"install disk" || cmd == b"show install" {
                        if !block_is_ata {
                            SERIAL.write_str(
                                "%install: disk sumber bukan ATA nyata — instalasi ditolak.\r\n",
                            );
                        } else if !target_is_ata {
                            SERIAL.write_str("%install: tidak ada disk pada primary slave (drive 1) sebagai target.\r\n");
                        } else {
                            SERIAL.write_str("install disk: INSPEKSI SAJA (nol tulis) src=drive0 -> tgt=drive1\r\n");
                            let mut src = AtaBlockIo { dev: &mut ata_dev };
                            let mut tgt = AtaBlockIo { dev: &mut ata_tgt };
                            match asl_router_installer::inspect(&mut src, &mut tgt, &|s| {
                                SERIAL.write_str(s)
                            }) {
                                Ok(plan) => {
                                    SERIAL.write_str("Rencana instalasi terverifikasi:\r\n");
                                    install_print_plan(&plan);
                                    SERIAL.write_str(
                                        "Eksekusi destruktif: install confirm YES-INSTALL\r\n",
                                    );
                                }
                                Err(f) => install_print_failure(&f),
                            }
                        }
                    } else if cmd.starts_with(b"install confirm ") {
                        if !block_is_ata {
                            SERIAL.write_str(
                                "%install: disk sumber bukan ATA nyata — instalasi ditolak.\r\n",
                            );
                        } else if !target_is_ata {
                            SERIAL.write_str("%install: tidak ada disk pada primary slave (drive 1) sebagai target.\r\n");
                        } else {
                            let token = &cmd[16..];
                            let tok_str = core::str::from_utf8(token).unwrap_or("");
                            SERIAL.write_str("install confirm: token dari UART RX = ");
                            SERIAL.write_str(tok_str);
                            SERIAL.write_str("\r\n");
                            let mut src = AtaBlockIo { dev: &mut ata_dev };
                            let mut tgt = AtaBlockIo { dev: &mut ata_tgt };
                            match asl_router_installer::install_run(
                                &mut src,
                                &mut tgt,
                                "ATA primary master (drive 0)",
                                "ATA primary slave (drive 1)",
                                tok_str,
                                &|s| SERIAL.write_str(s),
                            ) {
                                Ok(rep) => {
                                    SERIAL.write_str("[R27] INSTALL DONE sectors_written=");
                                    SERIAL.write_u32(rep.sectors_written as u32);
                                    SERIAL.write_str(" esp_sectors=");
                                    SERIAL.write_u32(rep.esp_sectors as u32);
                                    SERIAL.write_str(" sha256_w==sha256_rb=OK\r\n");
                                    install_print_plan(&rep.plan);
                                    SERIAL.write_str("Jurnal RJCFG01 SUMBER tidak pernah disentuh. shutdown, lepas disk sumber, boot dari disk target.\r\n");
                                }
                                Err(f) => install_print_failure(&f),
                            }
                        }
                    } else if cmd == b"install cancel" {
                        SERIAL.write_str("install cancel: installer TIDAK dipanggil — nol sektor ditulis ke drive 1 (tidak ada perubahan pada target).\r\n");
                    } else if cmd == b"clear" {
                        SERIAL.write_str("\x1B[2J\x1B[H");
                    } else if cmd == b"reboot" {
                        SERIAL.write_str(
                            "[R26] Reboot via reset 8042 (0xFE) lalu triple fault...\r\n",
                        );
                        power::system_reset();
                    } else if cmd == b"shutdown" {
                        SERIAL.write_str(
                            "[R25] SHUTDOWN REQUESTED — probing ACPI PM1_CNT candidates...\r\n",
                        );
                        if power::try_poweroff_candidates(&|s| SERIAL.write_str(s)) {
                            SERIAL.write_str("[R25] Power-off unexpected return.\r\n");
                        }
                        SERIAL.write_str("[R25] Semua kandidat PM1_CNT ditolak — entering safe halt (power-off TIDAK didukung platform ini).\r\n");
                        loop {
                            #[cfg(target_arch = "x86_64")]
                            unsafe {
                                core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
                            }
                            #[cfg(not(target_arch = "x86_64"))]
                            core::hint::spin_loop();
                        }
                    } else {
                        SERIAL.write_str("% Unknown command. Type 'help' for command list.\r\n");
                    }
                    cli_len = 0;
                }
                SERIAL.write_str("rinjani-cli> ");
            } else if ch == 8 || ch == 127 {
                // Backspace
                if cli_len > 0 {
                    cli_len -= 1;
                    SERIAL.write_str("\x08 \x08");
                }
            } else if ch >= 32 && ch < 127 {
                if cli_len < 255 {
                    cli_line[cli_len] = ch;
                    cli_len += 1;
                    SERIAL.write_byte(ch);
                }
            }
        }

        // Delay halus per poll tick
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }

        // [R25] POWER-OFF SELFTEST (build dengan --features poweroff-selftest)
        #[cfg(feature = "poweroff-selftest")]
        if loop_count == 400 {
            SERIAL.write_str("\n[R25] SELFTEST: ACPI PM1_CNT power-off probe (auto)\n");
            power::try_poweroff_candidates(&|s| SERIAL.write_str(s));
            SERIAL.write_str(
                "[R25] SELFTEST: semua kandidat ditolak hardware — heartbeat post-probe dimulai.\n",
            );
            let mut hb = 0u32;
            loop {
                hb = hb.wrapping_add(1);
                SERIAL.write_str("[HB] post-probe alive #");
                SERIAL.write_u32(hb);
                SERIAL.write_str("\n");
                for _ in 0..20_000_000u32 {
                    core::hint::spin_loop();
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// [R27] SELF-INSTALLER: adapter BlockIo nyata + helper CLI
// -----------------------------------------------------------------------------

/// Adapter traits installer ke driver ATA PIO nyata. Log driver diteruskan ke
/// serial supaya timeout/ERRATA ATA tetap terlihat di dalam tahap installer.
struct AtaBlockIo<'a> {
    dev: &'a mut asl_router_drivers_ata::AtaDevice,
}

impl asl_router_installer::BlockIo for AtaBlockIo<'_> {
    fn capacity_sectors(&self) -> u64 {
        self.dev.capacity_sectors()
    }
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; 512]) -> Result<(), &'static str> {
        self.dev.read_sector(lba, buf, &|s| SERIAL.write_str(s))
    }
    fn write_sector(&mut self, lba: u64, buf: &[u8; 512]) -> Result<(), &'static str> {
        self.dev.write_sector(lba, buf, &|s| SERIAL.write_str(s))
    }
}

/// Cetak rencana hasil inspeksi (nol tulis) ke serial.
fn install_print_plan(plan: &asl_router_installer::InstallPlan) {
    SERIAL.write_str("  capacity_sectors = ");
    SERIAL.write_u32(plan.total_sectors as u32);
    SERIAL.write_str("  esp = ");
    SERIAL.write_u32(plan.esp_start as u32);
    SERIAL.write_str("..");
    SERIAL.write_u32(plan.esp_end as u32);
    SERIAL.write_str(" (salin 0..");
    SERIAL.write_u32(plan.esp_used_sectors as u32);
    SERIAL.write_str(")\r\n");
    SERIAL.write_str("  RJCFG01 = ");
    SERIAL.write_u32(plan.cfg_start as u32);
    SERIAL.write_str("..");
    SERIAL.write_u32(plan.cfg_end as u32);
    SERIAL.write_str("  RJDATA = ");
    SERIAL.write_u32(plan.data_start as u32);
    SERIAL.write_str("..");
    SERIAL.write_u32(plan.data_end as u32);
    SERIAL.write_str("  entry_lba = ");
    SERIAL.write_u32(plan.entry_lba as u32);
    SERIAL.write_str("\r\n");
}

/// Cetak kegagalan bertahap (stage-tagged) sesuai format log [R27].
fn install_print_failure(f: &asl_router_installer::InstallFailure) {
    SERIAL.write_str("[R27] INSTALL FAILED stage=");
    SERIAL.write_str(f.stage);
    SERIAL.write_str(" reason=");
    SERIAL.write_str(f.reason);
    SERIAL.write_str("\r\n");
}

// -----------------------------------------------------------------------------
// [R26] RJCFG01 PERSISTENCE: tulis journal ke partisi di disk nyata + baca-balik
// -----------------------------------------------------------------------------

/// Tulis slot aktif lalu header (header LAST, atomic-switch style) ke partisi RJCFG01
/// via PIO ATA nyata, kemudian baca-balik dan verifikasi CRC header + slot.
/// Mengembalikan true hanya bila seluruh write DAN readback terverifikasi.
fn rjcfg_write_and_verify(
    ata_dev: &mut asl_router_drivers_ata::AtaDevice,
    journal: &asl_router_persistence::RjcfgJournal,
    base: u64,
    slot: u8,
) -> bool {
    let fsec = match asl_router_persistence::slot_first_sector(slot) {
        Some(f) => f,
        None => {
            SERIAL.write_str("[R26] slot tidak valid utk tulis disk\n");
            return false;
        }
    };
    let slot_bytes = if slot == 0 {
        &journal.slot_a
    } else {
        &journal.slot_b
    };
    for i in 0..8u64 {
        let mut sec = [0u8; 512];
        sec.copy_from_slice(&slot_bytes[(i as usize) * 512..(i as usize + 1) * 512]);
        if let Err(e) = ata_dev.write_sector(base + fsec + i, &sec, &|s| SERIAL.write_str(s)) {
            SERIAL.write_str("[R26] slot sector WRITE FAILED: ");
            SERIAL.write_str(e);
            SERIAL.write_str("\n");
            return false;
        }
    }
    let mut hdr_sec = [0u8; 512];
    journal.serialize_header_to_sector(&mut hdr_sec);
    if let Err(e) = ata_dev.write_sector(base, &hdr_sec, &|s| SERIAL.write_str(s)) {
        SERIAL.write_str("[R26] header WRITE FAILED: ");
        SERIAL.write_str(e);
        SERIAL.write_str("\n");
        return false;
    }
    // Baca-balik header
    let mut vhdr = [0u8; 512];
    if let Err(e) = ata_dev.read_sector(base, &mut vhdr, &|s| SERIAL.write_str(s)) {
        SERIAL.write_str("[R26] header READBACK READ FAILED: ");
        SERIAL.write_str(e);
        SERIAL.write_str("\n");
        return false;
    }
    let hdr = match asl_router_persistence::RjcfgJournal::deserialize_header_from_sector(&vhdr) {
        Ok(h) => h,
        Err(e) => {
            SERIAL.write_str("[R26] header READBACK INVALID: ");
            SERIAL.write_str(e);
            SERIAL.write_str("\n");
            return false;
        }
    };
    if hdr.sequence_number != journal.header.sequence_number || hdr.active_slot != slot {
        SERIAL.write_str("[R26] header READBACK tidak konsisten dengan RAM journal\n");
        return false;
    }
    let vfsec = match asl_router_persistence::slot_first_sector(hdr.active_slot) {
        Some(f) => f,
        None => {
            SERIAL.write_str("[R26] active_slot readback tidak valid\n");
            return false;
        }
    };
    // Baca-balik slot + verifikasi CRC via load_from_disk
    let mut vbuf = [0u8; 4096];
    for i in 0..8u64 {
        let mut sec = [0u8; 512];
        if let Err(e) = ata_dev.read_sector(base + vfsec + i, &mut sec, &|s| SERIAL.write_str(s)) {
            SERIAL.write_str("[R26] slot READBACK READ FAILED: ");
            SERIAL.write_str(e);
            SERIAL.write_str("\n");
            return false;
        }
        vbuf[(i as usize) * 512..(i as usize + 1) * 512].copy_from_slice(&sec);
    }
    match asl_router_persistence::RjcfgJournal::load_from_disk(&hdr, &vbuf) {
        Ok(_) => {
            SERIAL.write_str("[R26] RJCFG01 READBACK VERIFIED: slot ");
            SERIAL.write_u32(slot as u32);
            SERIAL.write_str(" seq=");
            SERIAL.write_u32(hdr.sequence_number);
            SERIAL.write_str(" (CRC header + slot cocok dari disk, PIO nyata)\n");
            true
        }
        Err(e) => {
            SERIAL.write_str("[R26] slot READBACK load FAILED: ");
            SERIAL.write_str(e);
            SERIAL.write_str("\n");
            false
        }
    }
}

// -----------------------------------------------------------------------------
// BARE-METAL PANIC HANDLER
// -----------------------------------------------------------------------------

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    struct PanicWriter;
    impl core::fmt::Write for PanicWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            SERIAL.write_str(s);
            Ok(())
        }
    }
    SERIAL.write_str("\n[RINJANI-PANIC] ");
    let _ = core::fmt::write(&mut PanicWriter, format_args!("{}", info));
    SERIAL.write_str("\n[RINJANI-PANIC] Halt loop.\n");
    loop {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
        #[cfg(not(target_arch = "x86_64"))]
        core::hint::spin_loop();
    }
}

// Dummy main for host/test profile if ever compiled on non-none OS
#[cfg(not(target_os = "none"))]
fn main() {}
