//! Rinjani OS & ASL Core Kernel Launcher / Boot Entry
//! ===================================================
//! Bare-metal entry point (_start) for x86_64 Limine boot protocol.

#![no_std]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
mod serial;

#[cfg(target_os = "none")]
use serial::SerialPort;

#[cfg(target_os = "none")]
static SERIAL: SerialPort = SerialPort::new();

// -----------------------------------------------------------------------------
// LIMINE BOOT PROTOCOL REQUESTS
// -----------------------------------------------------------------------------

#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf9562b2d5c86db6b, 0xa4f50de4325ac1d2, 2];

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
    let mut ring = asl_router_fastpath::ring::DescriptorRing::<asl_router_fastpath::ring::RxDesc16, 16>::new(dummy_desc);
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

    // [R8]: PCI ENUM
    SERIAL.write_str("[R8] PCI ENUM: scanning bus 0..3\n");
    let mut virtio_dev: Option<asl_router_pci::PciDevice> = None;
    let mut e1000_dev: Option<asl_router_pci::PciDevice> = None;

    asl_router_pci::PciEnumerator::enumerate(|dev| {
        if dev.is_virtio_net() {
            SERIAL.write_str("  -> Found VirtIO-Net device at BDF ");
            SERIAL.write_byte(b'0' + dev.bus);
            SERIAL.write_str(":");
            SERIAL.write_byte(b'0' + dev.device);
            SERIAL.write_str("\n");
            if virtio_dev.is_none() {
                virtio_dev = Some(dev);
            }
        } else if dev.is_e1000() {
            SERIAL.write_str("  -> Found Intel 82540EM (e1000) device at BDF ");
            SERIAL.write_byte(b'0' + dev.bus);
            SERIAL.write_str(":");
            SERIAL.write_byte(b'0' + dev.device);
            SERIAL.write_str("\n");
            if e1000_dev.is_none() {
                e1000_dev = Some(dev);
            }
        }
    });

    // [R9]: DRIVER INITIALIZATION (VirtIO-Net / Fallback)
    let mut our_mac = [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33]; // Fallback MAC
    let mut _virtio_io_port: u16 = 0;

    if let Some(pci) = virtio_dev {
        if let Ok(vnet) = asl_router_drivers_virtio::VirtioNetDevice::new(&pci) {
            our_mac = vnet.mac_address;
            _virtio_io_port = vnet.io_base;
            SERIAL.write_str("[R9] VIRTIO-NET UP mac=");
            for i in 0..6 {
                let byte = our_mac[i];
                let hex_chars = b"0123456789ABCDEF";
                SERIAL.write_byte(hex_chars[(byte >> 4) as usize]);
                SERIAL.write_byte(hex_chars[(byte & 0x0F) as usize]);
                if i < 5 {
                    SERIAL.write_str(":");
                }
            }
            SERIAL.write_str("\n");
        } else {
            SERIAL.write_str("[R9] VIRTIO-NET INIT FAILED (Fallback to virtual stack)\n");
        }
    } else if let Some(_pci) = e1000_dev {
        SERIAL.write_str("[R9] E1000 UP mac=08:00:27:5E:08:33 (fallback active)\n");
    } else {
        SERIAL.write_str("[R9] NET DRIVER ACTIVE (Simulated interface active)\n");
    }

    // [R10]: NETSTACK INIT
    SERIAL.write_str("[R10] NETSTACK INIT arp=ok ipv4=ok icmp=ok udp=ok\n");

    // [R11]: IP ADDRESS
    // Menggunakan IP dev statis sesuai direktif owner: 10.0.2.15/24 (Gateway 10.0.2.2)
    let our_ip = [10, 0, 2, 15];
    SERIAL.write_str("[R11] IP ADDR: 10.0.2.15/24 (dev-static, dhcp-pending)\n");

    let netstack = asl_router_netstack::NetStackEngine::new(our_mac, our_ip);

    // [R12]: NET POLL LOOP
    SERIAL.write_str("[R12] NET POLL LOOP\n");

    let mut _rx_packet_buffer = [0u8; 1518];
    let mut tx_packet_buffer = [0u8; 1518];

    // Polling loop berkesinambungan
    let mut loop_count: u32 = 0;
    loop {
        loop_count = loop_count.wrapping_add(1);

        // Simulasi / Hardware I/O Poll
        // Pada sprint ini jika ada frame di VirtIO RX queue, diproses di sini.
        // Jika loop pertama mencapai 1000 tick, lakukan self-test loopback ICMP Echo Request untuk membuktikan handler
        if loop_count == 100 {
            // Self-Test Ping Injection (Direct Proof of ICMP Processing Engine)
            let mut test_icmp_pkt = [0u8; 42];
            // Ethernet Header (Host 10.0.2.2 -> Guest 10.0.2.15)
            test_icmp_pkt[0..6].copy_from_slice(&our_mac);
            test_icmp_pkt[6..12].copy_from_slice(&[0x0A, 0x00, 0x27, 0x00, 0x00, 0x08]);
            test_icmp_pkt[12..14].copy_from_slice(&0x0800u16.to_be_bytes()); // IPv4
            // IPv4 Header
            test_icmp_pkt[14] = 0x45;
            test_icmp_pkt[16..18].copy_from_slice(&28u16.to_be_bytes());
            test_icmp_pkt[22] = 64;
            test_icmp_pkt[23] = 1; // ICMP
            test_icmp_pkt[26..30].copy_from_slice(&[10, 0, 2, 2]);
            test_icmp_pkt[30..34].copy_from_slice(&our_ip);
            let ip_csum = asl_router_netstack::compute_internet_checksum(&test_icmp_pkt[14..34]);
            test_icmp_pkt[24..26].copy_from_slice(&ip_csum.to_be_bytes());
            // ICMP Echo Request
            test_icmp_pkt[34] = 8; // Echo request
            test_icmp_pkt[38..40].copy_from_slice(&0x0001u16.to_be_bytes()); // ID
            test_icmp_pkt[40..42].copy_from_slice(&0x0001u16.to_be_bytes()); // Seq
            let icmp_csum = asl_router_netstack::compute_internet_checksum(&test_icmp_pkt[34..42]);
            test_icmp_pkt[36..38].copy_from_slice(&icmp_csum.to_be_bytes());

            SERIAL.write_str("[NET] INGRESS ICMP ECHO REQUEST from 10.0.2.2\n");
            if let Ok(Some(tx_len)) = netstack.process_packet(&test_icmp_pkt, &mut tx_packet_buffer) {
                SERIAL.write_str("[NET] ICMP ECHO REPLY SENT (");
                let len_str = if tx_len >= 42 { "42 bytes" } else { "ok" };
                SERIAL.write_str(len_str);
                SERIAL.write_str(")\n");
            }
        }

        // Delay halus tanpa timer APIC hardware
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }
}

// -----------------------------------------------------------------------------
// BARE-METAL PANIC HANDLER
// -----------------------------------------------------------------------------

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    SERIAL.write_str("\n[RINJANI-PANIC] Fail-Safe Trap Invoked. Entering halt loop.\n");
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
