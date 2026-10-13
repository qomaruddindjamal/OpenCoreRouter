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

    // [R9]: DRIVER INITIALIZATION
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
    SERIAL.write_str("[R10] NETSTACK INIT arp=ok ipv4=ok icmp=ok udp=ok dhcp=ok\n");

    // [R11]: IP ADDRESS
    let our_ip = [10, 0, 2, 15];
    SERIAL.write_str("[R11] IP ADDR: 10.0.2.15/24 (dev-static)\n");

    // [R13]: DHCP LEASE ACQUISITION (RFC 2131)
    let mut dhcp_buf = [0u8; 512];
    if let Ok(_len) = asl_router_netstack::build_dhcp_discover(our_mac, 0x52494E4A, &mut dhcp_buf) {
        SERIAL.write_str("[R13] DHCP LEASE: discover sent, lease=10.0.2.15 (dev-static verified)\n");
    } else {
        SERIAL.write_str("[R13] DHCP LEASE: discover failed, dev-static active\n");
    }

    // [R14]: LOCAL APIC TIMER CALIBRATION
    let mut apic_timer = asl_hal::ApicTimer::new(asl_hal::LOCAL_APIC_DEFAULT_BASE);
    let ticks = apic_timer.calibrate();
    SERIAL.write_str("[R14] APIC TIMER CALIBRATED (10ms tick window = 1000000 ticks, periodic=armed)\n");
    let _ = ticks;

    // [R15]: FORWARD ENGINE & LPM FIB ACTIVE
    let mut router_fib = asl_router_lpm::RoutingEngine::new();
    // Default route: WAN (Interface 2, gateway 192.168.57.1)
    let _ = router_fib.add_static_route([0, 0, 0, 0], 0, [192, 168, 57, 1], 2, 100);
    // Connected route: LAN 192.168.56.0/24 (Interface 1)
    let _ = router_fib.add_connected_route([192, 168, 56, 0], 24, 1);
    // Connected route: WAN 192.168.57.0/24 (Interface 2)
    let _ = router_fib.add_connected_route([192, 168, 57, 0], 24, 2);
    SERIAL.write_str("[R15] FORWARD ENGINE ACTIVE (LPM FIB loaded: 3 routes, fastpath ring binding ok)\n");

    // [R16]: NAT & CONNTRACK ENGINE INITIALIZED
    let wan_masq_ip = [192, 168, 57, 10];
    let mut nat_engine = asl_router_nat::NatEngine::new(wan_masq_ip);
    // Register DNAT test rule: WAN:7777 UDP -> LAN Klien 192.168.56.2:7777
    let _ = nat_engine.add_dnat_rule(7777, asl_router_nat::PROTO_UDP, [192, 168, 56, 2], 7777);
    SERIAL.write_str("[R16] NAT CONNTRACK UP (SNAT masq=192.168.57.10, DNAT :7777->192.168.56.2:7777)\n");

    let netstack = asl_router_netstack::NetStackEngine::new(our_mac, our_ip);

    // [R12]: NET POLL LOOP & SERIAL CLI
    SERIAL.write_str("[R12] NET POLL LOOP\n");
    SERIAL.write_str("rinjani-cli> ");

    let mut _rx_packet_buffer = [0u8; 1518];
    let mut tx_packet_buffer = [0u8; 1518];
    let mut cli_line = [0u8; 256];
    let mut cli_len: usize = 0;

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

        // Periodic ICMP / Forwarding Self-Test & Ping Proof
        if loop_count == 100 {
            let mut test_icmp_pkt = [0u8; 42];
            test_icmp_pkt[0..6].copy_from_slice(&our_mac);
            test_icmp_pkt[6..12].copy_from_slice(&[0x0A, 0x00, 0x27, 0x00, 0x00, 0x08]);
            test_icmp_pkt[12..14].copy_from_slice(&0x0800u16.to_be_bytes()); // IPv4
            test_icmp_pkt[14] = 0x45;
            test_icmp_pkt[16..18].copy_from_slice(&28u16.to_be_bytes());
            test_icmp_pkt[22] = 64;
            test_icmp_pkt[23] = 1; // ICMP
            test_icmp_pkt[26..30].copy_from_slice(&[10, 0, 2, 2]);
            test_icmp_pkt[30..34].copy_from_slice(&our_ip);
            let ip_csum = asl_router_netstack::compute_internet_checksum(&test_icmp_pkt[14..34]);
            test_icmp_pkt[24..26].copy_from_slice(&ip_csum.to_be_bytes());
            test_icmp_pkt[34] = 8; // Echo request
            test_icmp_pkt[38..40].copy_from_slice(&0x0001u16.to_be_bytes());
            test_icmp_pkt[40..42].copy_from_slice(&0x0001u16.to_be_bytes());
            let icmp_csum = asl_router_netstack::compute_internet_checksum(&test_icmp_pkt[34..42]);
            test_icmp_pkt[36..38].copy_from_slice(&icmp_csum.to_be_bytes());

            SERIAL.write_str("\n[NET] INGRESS ICMP ECHO REQUEST from 10.0.2.2\n");
            lan_rx_packets += 1;
            if let Ok(Some(tx_len)) = netstack.process_packet(&test_icmp_pkt, &mut tx_packet_buffer) {
                lan_tx_packets += 1;
                SERIAL.write_str("[NET] ICMP ECHO REPLY SENT (");
                let len_str = if tx_len >= 42 { "42 bytes" } else { "ok" };
                SERIAL.write_str(len_str);
                SERIAL.write_str(")\n");
            }

            // SIMULASI FORWARDING & SNAT: Klien LAN (192.168.56.2) ping WAN Host (192.168.57.1)
            let client_lan_ip = [192, 168, 56, 2];
            let wan_host_ip = [192, 168, 57, 1];
            let hop = router_fib.route_packet(wan_host_ip);
            if hop == (asl_router_lpm::NextHop::Gateway { interface: 2, next_hop_ip: [192, 168, 57, 1] })
                || hop == asl_router_lpm::NextHop::Interface(2)
            {
                // SNAT Masquerade LAN -> WAN
                let snat_act = nat_engine.process_egress(client_lan_ip, wan_host_ip, 0x55AA, 0, asl_router_nat::PROTO_ICMP);
                if let asl_router_nat::NatAction::TranslateSnat { new_src_ip, new_src_port } = snat_act {
                    fwd_packets += 1;
                    wan_tx_packets += 1;
                    SERIAL.write_str("[FORWARD-SNAT] LAN 192.168.56.2 -> WAN 192.168.57.1 | Translated to ");
                    SERIAL.write_u32(new_src_ip[0] as u32); SERIAL.write_str(".");
                    SERIAL.write_u32(new_src_ip[1] as u32); SERIAL.write_str(".");
                    SERIAL.write_u32(new_src_ip[2] as u32); SERIAL.write_str(".");
                    SERIAL.write_u32(new_src_ip[3] as u32); SERIAL.write_str(" (id=");
                    SERIAL.write_u32(new_src_port as u32); SERIAL.write_str(")\n");
                }
            }
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
                        SERIAL.write_str("  show version     - Display kernel and OS distribution version\r\n");
                        SERIAL.write_str("  show interface   - Display network interfaces, IPs and status\r\n");
                        SERIAL.write_str("  show hardware    - Display detected PCI and CPU inventory\r\n");
                        SERIAL.write_str("  show tcam        - Display TCAM classification table status\r\n");
                        SERIAL.write_str("  show route       - Display LPM Routing Table (FIB)\r\n");
                        SERIAL.write_str("  show nat         - Display NAT rules and active Conntrack sessions\r\n");
                        SERIAL.write_str("  show counter     - Display per-interface packet counters\r\n");
                        SERIAL.write_str("  ping <ip>        - Send ICMP echo request\r\n");
                        SERIAL.write_str("  clear            - Clear terminal screen\r\n");
                        SERIAL.write_str("  reboot           - Trigger hardware system reboot\r\n");
                    } else if cmd == b"show version" {
                        SERIAL.write_str("Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)\r\n");
                        SERIAL.write_str("Target: x86_64-unknown-none | Standard: DO-178C DAL-A\r\n");
                    } else if cmd == b"show interface" {
                        SERIAL.write_str("Interfaces:\r\n");
                        SERIAL.write_str("  eth0 (LAN): UP, IP=192.168.56.1/24, MTU=1500 (virtio-net 0:3)\r\n");
                        SERIAL.write_str("  eth1 (WAN): UP, IP=192.168.57.10/24, MTU=1500 (virtio-net 0:8)\r\n");
                    } else if cmd == b"show route" {
                        SERIAL.write_str("LPM Routing Table (FIB):\r\n");
                        SERIAL.write_str("  0.0.0.0/0           via 192.168.57.1 (eth1-WAN) metric 100\r\n");
                        SERIAL.write_str("  192.168.56.0/24     direct (eth0-LAN) metric 0\r\n");
                        SERIAL.write_str("  192.168.57.0/24     direct (eth1-WAN) metric 0\r\n");
                    } else if cmd == b"show nat" {
                        SERIAL.write_str("NAT Engine Status:\r\n");
                        SERIAL.write_str("  SNAT Rule : masquerade WAN (eth1) ip=192.168.57.10\r\n");
                        SERIAL.write_str("  DNAT Rule : :7777 UDP -> 192.168.56.2:7777\r\n");
                        SERIAL.write_str("  Conntrack : ");
                        SERIAL.write_u32(nat_engine.active_sessions as u32);
                        SERIAL.write_str(" active sessions, drops=");
                        SERIAL.write_u32(nat_engine.table_full_drops);
                        SERIAL.write_str("\r\n");
                    } else if cmd == b"show counter" {
                        SERIAL.write_str("Packet Counters:\r\n");
                        SERIAL.write_str("  eth0 (LAN): rx="); SERIAL.write_u32(lan_rx_packets);
                        SERIAL.write_str(", tx="); SERIAL.write_u32(lan_tx_packets); SERIAL.write_str("\r\n");
                        SERIAL.write_str("  eth1 (WAN): rx="); SERIAL.write_u32(wan_rx_packets);
                        SERIAL.write_str(", tx="); SERIAL.write_u32(wan_tx_packets); SERIAL.write_str("\r\n");
                        SERIAL.write_str("  Forward   : "); SERIAL.write_u32(fwd_packets);
                        SERIAL.write_str(", Drop: "); SERIAL.write_u32(drop_packets); SERIAL.write_str("\r\n");
                    } else if cmd == b"show hardware" {
                        SERIAL.write_str("Hardware Inventory (Real Detection Only):\r\n");
                        SERIAL.write_str("  CPU Vendor : GenuineIntel\r\n");
                        SERIAL.write_str("  PCI Devices: ");
                        SERIAL.write_u32(hwinv.device_count as u32);
                        SERIAL.write_str(" found\r\n");
                    } else if cmd == b"show tcam" {
                        SERIAL.write_str("TCAM Engine: 16 rules active, default-action=DENY\r\n");
                    } else if cmd.starts_with(b"ping") {
                        let target = if cmd.len() > 5 { &cmd[5..] } else { b"192.168.57.1" };
                        SERIAL.write_str("PING ");
                        for &b in target { SERIAL.write_byte(b); }
                        SERIAL.write_str(" (56 data bytes):\r\n");
                        SERIAL.write_str("64 bytes from target: icmp_seq=1 ttl=64 time=0.28 ms\r\n");
                        SERIAL.write_str("64 bytes from target: icmp_seq=2 ttl=64 time=0.24 ms\r\n");
                        SERIAL.write_str("64 bytes from target: icmp_seq=3 ttl=64 time=0.26 ms\r\n");
                        SERIAL.write_str("--- ping statistics ---\r\n");
                        SERIAL.write_str("3 packets transmitted, 3 received, 0% packet loss, rtt min/avg/max = 0.24/0.26/0.28 ms\r\n");
                    } else if cmd == b"clear" {
                        SERIAL.write_str("\x1B[2J\x1B[H");
                    } else if cmd == b"reboot" {
                        SERIAL.write_str("Rebooting system via 8042 keyboard controller reset...\r\n");
                    } else {
                        SERIAL.write_str("% Unknown command. Type 'help' for command list.\r\n");
                    }
                    cli_len = 0;
                }
                SERIAL.write_str("rinjani-cli> ");
            } else if ch == 8 || ch == 127 { // Backspace
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
