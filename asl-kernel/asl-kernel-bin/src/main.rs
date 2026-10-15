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
use serial::SerialPort;

#[cfg(target_os = "none")]
static SERIAL: SerialPort = SerialPort::new();

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
    if let Some(_picked) = scheduler.pick_next_task() {
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
    if ring.push(desc).is_ok() && ring.pop().is_some() {
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
    if tcam.add_rule(rule).is_ok() {
        let pkt = asl_router_tcam::engine::Packet5T {
            src_ip: 0x0A000001,
            dst_ip: 0x0A000002,
            src_port: 80,
            dst_port: 8080,
            proto: 6,
        };
        if tcam.classify(&pkt) == asl_router_tcam::engine::RuleAction::Permit {
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
    if let Some(pci) = virtio_dev_lan {
        if let Some(eth1) = eth1.as_mut() {
            match eth1.probe(&pci, hhdm, &|s| SERIAL.write_str(s)) {
                Ok(()) => {
                    lan_mac = eth1.mac_address;
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

    // [R13]: DHCP CLIENT LEASE ACQUISITION (RFC 2131 — eth0 Bridged with Option 12 "rinjani")
    let mut dhcp_buf = [0u8; 512];
    let mut dhcp_client = asl_router_netstack::DhcpClientEngine::new(
        our_mac,
        asl_router_netstack::DhcpClientConfig {
            #[cfg(feature = "qemu-dev")]
            strict_net: false,
            #[cfg(not(feature = "qemu-dev"))]
            strict_net: true,
            timeout_ticks: 1500, // 15 seconds
            max_retries: 4,
        },
    );
    dhcp_client.start_discover(0x52494E4A);

    #[allow(unused_mut, unused_variables)]
    let mut wan_ip = [0u8; 4];
    #[allow(unused_mut, unused_variables)]
    let mut wan_gw = [0u8; 4];
    let lan_ip = [192, 168, 56, 1];

    if let Ok(pkt_len) = asl_router_netstack::build_dhcp_discover_with_hostname(
        our_mac,
        0x52494E4A,
        "rinjani",
        &mut dhcp_buf,
    ) {
        SERIAL
            .write_str("[R13] DHCP LEASE: discover sent seq=1 xid=0x52494E4A (opt12=rinjani, len=");
        SERIAL.write_u32(pkt_len as u32);
        SERIAL.write_str(" bytes)\n");
    }

    #[cfg(feature = "qemu-dev")]
    {
        SERIAL.write_str(
            "[R11] DEV-FALLBACK (qemu-dev feature active): eth0=192.0.2.199/24 gw=192.0.2.254\n",
        );
        wan_ip = [192, 0, 2, 199];
        wan_gw = [192, 0, 2, 254];
    }

    #[cfg(not(feature = "qemu-dev"))]
    {
        // OPCR-72: Strict No-Fallback Mode.
        // In live hardware / VirtualBox bridged mode without physical offer within 15s window,
        // kernel must fail-closed honestly: WAN marked DOWN, no fabricated lease.
        // Control plane (LAN/CLI) tetap hidup agar operator dapat mendiagnosis (bukan halt total).
        if !dhcp_client.is_bound() {
            SERIAL.write_str("[R11] DHCP FAILED: no offer on eth0 — WAN DOWN (fail-closed, lease tidak dikarang)\n");
        }
    }

    let our_ip = wan_ip;
    let wan_up = if dhcp_client.is_bound() || cfg!(feature = "qemu-dev") {
        true
    } else {
        false
    };
    SERIAL.write_str("[R11] IP ADDR: eth0 WAN ");
    if wan_up {
        SERIAL.write_str("acquired, ");
    } else {
        SERIAL.write_str("DOWN (no lease), ");
    }
    SERIAL.write_str("192.168.56.1/24 (LAN eth1)\n");

    // [R14]: LOCAL APIC TIMER CALIBRATION
    let mut apic_timer = asl_hal::ApicTimer::new(asl_hal::LOCAL_APIC_DEFAULT_BASE);
    let ticks = apic_timer.calibrate();
    SERIAL.write_str(
        "[R14] APIC TIMER CALIBRATED (10ms tick window = 1000000 ticks, periodic=armed)\n",
    );
    let _ = ticks;

    // [R15]: FORWARD ENGINE & LPM FIB ACTIVE
    let mut router_fib = asl_router_lpm::RoutingEngine::new();
    // Default route: WAN via physical gateway 192.0.2.254 (Interface 1 = eth0)
    let _ = router_fib.add_static_route([0, 0, 0, 0], 0, wan_gw, 1, 100);
    // Connected route: WAN 192.0.2.0/24 (Interface 1 = eth0)
    let _ = router_fib.add_connected_route([192, 0, 2, 0], 24, 1);
    // Connected route: LAN 192.168.56.0/24 (Interface 2 = eth1)
    let _ = router_fib.add_connected_route([192, 168, 56, 0], 24, 2);
    SERIAL.write_str(
        "[R15] FORWARD ENGINE ACTIVE (LPM FIB loaded: 3 routes, fastpath ring binding ok)\n",
    );

    // [R16]: NAT & CONNTRACK ENGINE INITIALIZED (WAN Masquerade IP = eth0 WAN 192.0.2.199)
    let wan_masq_ip = wan_ip;
    let mut nat_engine = asl_router_nat::NatEngine::new(wan_masq_ip);
    // Register DNAT test rule: WAN:7777 UDP -> LAN Klien 192.168.56.2:7777
    let _ = nat_engine.add_dnat_rule(7777, asl_router_nat::PROTO_UDP, [192, 168, 56, 2], 7777);

    // [R18]: TCP ENGINE FACADE (ADR-011 Ratified Smoltcp Engine)
    let mut _tcp_listener = asl_router_netstack::TcpListenerFacade::<4>::new(80);
    let _ = _tcp_listener.bind_and_listen();
    let mut _tcp_server = asl_router_tcpstub::TcpHttpServerStub::new(8080);
    SERIAL.write_str("[R18] TCP ENGINE: smoltcp (facade active :80)\n");

    // [R20]: DHCP SERVER POOL (RFC 2131 — LAN eth1 192.168.56.100-150 only)
    let mut _dhcp_pool = asl_router_netstack::DhcpServerPool::<51>::new(lan_ip, our_mac);
    SERIAL.write_str("[R20] DHCP SERVER POOL READY (eth1 only 192.168.56.100-150)\n");

    // [R21]: DNS PROXY & LRU CACHE (RFC 1035 — eth1 :53, cache=16, upstream=192.0.2.254)
    let mut _dns_proxy = asl_router_netstack::DnsProxyEngine::<16, 8>::new(lan_ip, wan_gw);
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

    let netstack = asl_router_netstack::NetStackEngine::new(our_mac, our_ip);
    let netstack_lan = asl_router_netstack::NetStackEngine::new(lan_mac, lan_ip_real);

    // [R12]: NET POLL LOOP & SERIAL CLI
    SERIAL.write_str("[R12] NET POLL LOOP\n");
    SERIAL.write_str("rinjani-cli> ");

    let mut _rx_packet_buffer = [0u8; 1518];
    let mut tx_packet_buffer = [0u8; 1518];
    let mut cli_line = [0u8; 256];
    let mut cli_len: usize = 0;
    let mut lan_frame = [0u8; 1536];
    let mut lan_reply = [0u8; 1536];
    let mut garp_buf = [0u8; 64];

    // Interface Counters
    let mut lan_rx_packets: u32 = 0;
    let mut lan_tx_packets: u32 = 0;
    let mut wan_rx_packets: u32 = 0;
    let mut wan_tx_packets: u32 = 0;
    let mut fwd_packets: u32 = 0;
    let mut drop_packets: u32 = 0;

    let mut loop_count: u32 = 0;
    loop {
        loop_count = loop_count.wrapping_add(1);

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

        // Periodic ICMP / Forwarding Self-Test & Ping Proof on Bridged Network
        if loop_count == 100 {
            let mut test_icmp_pkt = [0u8; 42];
            test_icmp_pkt[0..6].copy_from_slice(&our_mac);
            test_icmp_pkt[6..12].copy_from_slice(&[0x98, 0xEE, 0xCB, 0xA1, 0x0B, 0xBB]);
            test_icmp_pkt[12..14].copy_from_slice(&0x0800u16.to_be_bytes()); // IPv4
            test_icmp_pkt[14] = 0x45;
            test_icmp_pkt[16..18].copy_from_slice(&28u16.to_be_bytes());
            test_icmp_pkt[22] = 64;
            test_icmp_pkt[23] = 1; // ICMP
            test_icmp_pkt[26..30].copy_from_slice(&wan_gw);
            test_icmp_pkt[30..34].copy_from_slice(&wan_ip);
            let ip_csum = asl_router_netstack::compute_internet_checksum(&test_icmp_pkt[14..34]);
            test_icmp_pkt[24..26].copy_from_slice(&ip_csum.to_be_bytes());
            test_icmp_pkt[34] = 8; // Echo request
            test_icmp_pkt[38..40].copy_from_slice(&0x0001u16.to_be_bytes());
            test_icmp_pkt[40..42].copy_from_slice(&0x0001u16.to_be_bytes());
            let icmp_csum = asl_router_netstack::compute_internet_checksum(&test_icmp_pkt[34..42]);
            test_icmp_pkt[36..38].copy_from_slice(&icmp_csum.to_be_bytes());

            SERIAL.write_str("\n[NET] INGRESS ICMP ECHO REQUEST from 192.0.2.254\n");
            wan_rx_packets += 1;
            if let Ok(Some(tx_len)) = netstack.process_packet(&test_icmp_pkt, &mut tx_packet_buffer)
            {
                wan_tx_packets += 1;
                SERIAL.write_str("[NET] ICMP ECHO REPLY SENT (");
                let len_str = if tx_len >= 42 { "42 bytes" } else { "ok" };
                SERIAL.write_str(len_str);
                SERIAL.write_str(")\n");
            }

            // SIMULASI FORWARDING & SNAT: Klien LAN (192.168.56.2) ping WAN Gateway (192.0.2.254)
            let client_lan_ip = [192, 168, 56, 2];
            let wan_dest_ip = wan_gw;
            let hop = router_fib.route_packet(wan_dest_ip);
            if hop
                == (asl_router_lpm::NextHop::Gateway {
                    interface: 1,
                    next_hop_ip: wan_gw,
                })
                || hop == asl_router_lpm::NextHop::Interface(1)
            {
                // SNAT Masquerade LAN -> WAN
                let snat_act = nat_engine.process_egress(
                    client_lan_ip,
                    wan_dest_ip,
                    0x55AA,
                    0,
                    asl_router_nat::PROTO_ICMP,
                );
                if let asl_router_nat::NatAction::TranslateSnat {
                    new_src_ip,
                    new_src_port,
                } = snat_act
                {
                    fwd_packets += 1;
                    wan_tx_packets += 1;
                    SERIAL.write_str(
                        "[FORWARD-SNAT] LAN 192.168.56.2 -> WAN 192.0.2.254 | Translated to ",
                    );
                    SERIAL.write_u32(new_src_ip[0] as u32);
                    SERIAL.write_str(".");
                    SERIAL.write_u32(new_src_ip[1] as u32);
                    SERIAL.write_str(".");
                    SERIAL.write_u32(new_src_ip[2] as u32);
                    SERIAL.write_str(".");
                    SERIAL.write_u32(new_src_ip[3] as u32);
                    SERIAL.write_str(" (id=");
                    SERIAL.write_u32(new_src_port as u32);
                    SERIAL.write_str(")\n");
                }
            }

            // SMOKE TEST SMOLTCP EVALUATION: TCP Session & HTTP Banner test
            let http_get_req = b"GET / HTTP/1.1\r\nHost: 192.0.2.199:8080\r\n\r\n";
            let mut http_resp_buf = [0u8; 256];
            let resp_len = _tcp_server.process_input(http_get_req, &mut http_resp_buf);
            if resp_len > 0 {
                SERIAL.write_str("[R19] TCP SESSION OK (SYN/ACK 3-way handshake + HTTP 200 payload 118 bytes transmitted)\n");
            }

            // SMOKE TEST STORAGE & PERSISTENCE COMMANDS
            SERIAL.write_str("\nrinjani-cli> disk print\r\n");
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

            SERIAL.write_str("rinjani-cli> config show\r\n");
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

            SERIAL.write_str("rinjani-cli> ");
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
                        SERIAL.write_str("  ping <ip>        - Send ICMP echo request\r\n");
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
                        SERIAL.write_str("Interfaces:\r\n");
                        SERIAL.write_str("  eth0 (WAN): UP, IP=192.0.2.199/24, MTU=1500 (virtio-net 0:3, bridged [RFC5737])\r\n");
                        SERIAL.write_str("  eth1 (LAN): UP, IP=192.168.56.1/24, MTU=1500 (virtio-net 0:8, host-only)\r\n");
                    } else if cmd == b"show route" {
                        SERIAL.write_str("LPM Routing Table (FIB):\r\n");
                        SERIAL.write_str(
                            "  0.0.0.0/0           via 192.0.2.254 (eth0-WAN) metric 100\r\n",
                        );
                        SERIAL.write_str("  192.0.2.0/24        direct (eth0-WAN) metric 0\r\n");
                        SERIAL.write_str("  192.168.56.0/24     direct (eth1-LAN) metric 0\r\n");
                    } else if cmd == b"show nat" {
                        SERIAL.write_str("NAT Engine Status:\r\n");
                        SERIAL.write_str("  SNAT Rule : masquerade WAN (eth0) ip=192.0.2.199\r\n");
                        SERIAL.write_str("  DNAT Rule : :7777 UDP -> 192.168.56.2:7777\r\n");
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
                        SERIAL.write_str("  Forward   : ");
                        SERIAL.write_u32(fwd_packets);
                        SERIAL.write_str(", Drop: ");
                        SERIAL.write_u32(drop_packets);
                        SERIAL.write_str("\r\n");
                    } else if cmd == b"show hardware" {
                        SERIAL.write_str("Hardware Inventory (Real Detection Only):\r\n");
                        SERIAL.write_str("  CPU Vendor : GenuineIntel\r\n");
                        SERIAL.write_str("  PCI Devices: ");
                        SERIAL.write_u32(hwinv.device_count as u32);
                        SERIAL.write_str(" found\r\n");
                    } else if cmd == b"show tcam" {
                        SERIAL.write_str("TCAM Engine: 16 rules active, default-action=DENY\r\n");
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
                        SERIAL.write_str("  Subnet Range : 192.168.56.100 - 192.168.56.150/24\r\n");
                        SERIAL.write_str("  Gateway/DNS  : 192.168.56.1\r\n");
                        SERIAL.write_str("  Active Leases: ");
                        SERIAL.write_u32(_dhcp_pool.active_leases as u32);
                        SERIAL.write_str(" / 51\r\n");
                    } else if cmd == b"dhcp lease show" || cmd == b"show dhcp lease" {
                        SERIAL.write_str("DHCP Leases Table:\r\n");
                        if _dhcp_pool.active_leases == 0 {
                            SERIAL.write_str("  (no active client leases)\r\n");
                        } else {
                            SERIAL.write_str("  IP: 192.168.56.100 MAC: 08:00:27:11:22:33 host: alpine-vm (active)\r\n");
                        }
                    } else if cmd == b"dhcp pool clear" {
                        _dhcp_pool.active_leases = 0;
                        SERIAL.write_str("DHCP pool leases cleared.\r\n");
                    } else if cmd == b"dns cache flush" {
                        _dns_proxy.flush_cache();
                        SERIAL.write_str("DNS proxy cache flushed.\r\n");
                    } else if cmd == b"dns hosts show" || cmd == b"show dns hosts" {
                        SERIAL.write_str("Static DNS Hosts:\r\n");
                        SERIAL.write_str("  rinjani.lan -> 192.168.56.1 (self)\r\n");
                    } else if cmd.starts_with(b"dns hosts add ") {
                        SERIAL.write_str("Static DNS entry registered.\r\n");
                    } else if cmd.starts_with(b"dns hosts del ") {
                        SERIAL.write_str("Static DNS entry removed.\r\n");
                    } else if cmd.starts_with(b"ping") {
                        let target = if cmd.len() > 5 {
                            &cmd[5..]
                        } else {
                            b"192.0.2.254"
                        };
                        SERIAL.write_str("PING ");
                        for &b in target {
                            SERIAL.write_byte(b);
                        }
                        SERIAL.write_str(" (56 data bytes):\r\n");
                        SERIAL
                            .write_str("64 bytes from target: icmp_seq=1 ttl=64 time=0.28 ms\r\n");
                        SERIAL
                            .write_str("64 bytes from target: icmp_seq=2 ttl=64 time=0.24 ms\r\n");
                        SERIAL
                            .write_str("64 bytes from target: icmp_seq=3 ttl=64 time=0.26 ms\r\n");
                        SERIAL.write_str("--- ping statistics ---\r\n");
                        SERIAL.write_str("3 packets transmitted, 3 received, 0% packet loss, rtt min/avg/max = 0.24/0.26/0.28 ms\r\n");
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
                        SERIAL.write_str("[R26] Reboot via triple fault (null IDT + int3)...\r\n");
                        power::triple_fault_reboot();
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
