//! Open Core Router (OpenCR) — asl-router-nat
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Network Address Translation (NAT) Engine & Connection Tracker (Conntrack)
//!
//! Features:
//! - SNAT Masquerade (LAN Egress -> WAN)
//! - DNAT Port Forwarding (WAN Ingress -> LAN Host)
//! - Heapless Conntrack Table (TCP/UDP/ICMP 5-tuple + timeout jiffies)
//! - Bounded capacity & polite failure counters (fail-safe, no crash)

#![no_std]

pub const MAX_CONNTRACK_ENTRIES: usize = 128;
pub const MAX_DNAT_RULES: usize = 16;

pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

pub const DEFAULT_UDP_TIMEOUT_TICKS: u32 = 3000; // ~30 detik
pub const DEFAULT_TCP_TIMEOUT_TICKS: u32 = 12000; // ~120 detik
pub const DEFAULT_ICMP_TIMEOUT_TICKS: u32 = 1000; // ~10 detik

/// Status sesi pada tabel conntrack
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConntrackState {
    Unused,
    New,
    Established,
    Closing,
}

/// Satu entri pelacakan koneksi (5-Tuple Original + 5-Tuple Translated)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConntrackEntry {
    // Arah Asli (Inside -> Outside)
    pub orig_src_ip: [u8; 4],
    pub orig_dst_ip: [u8; 4],
    pub orig_src_port: u16,
    pub orig_dst_port: u16,
    pub protocol: u8,

    // Arah Terjemahan (Masquerade / Port-forward)
    pub trans_src_ip: [u8; 4],
    pub trans_dst_ip: [u8; 4],
    pub trans_src_port: u16,
    pub trans_dst_port: u16,

    pub state: ConntrackState,
    pub expiry_tick: u32,
    pub packets_fwd: u32,
    pub packets_rev: u32,
}

impl ConntrackEntry {
    pub const fn empty() -> Self {
        Self {
            orig_src_ip: [0; 4],
            orig_dst_ip: [0; 4],
            orig_src_port: 0,
            orig_dst_port: 0,
            protocol: 0,
            trans_src_ip: [0; 4],
            trans_dst_ip: [0; 4],
            trans_src_port: 0,
            trans_dst_port: 0,
            state: ConntrackState::Unused,
            expiry_tick: 0,
            packets_fwd: 0,
            packets_rev: 0,
        }
    }
}

/// Aturan Port-Forwarding (DNAT)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnatRule {
    pub wan_ip: [u8; 4],
    pub wan_port: u16,
    pub protocol: u8,
    pub target_lan_ip: [u8; 4],
    pub target_lan_port: u16,
    pub is_active: bool,
}

impl DnatRule {
    pub const fn empty() -> Self {
        Self {
            wan_ip: [0; 4],
            wan_port: 0,
            protocol: 0,
            target_lan_ip: [0; 4],
            target_lan_port: 0,
            is_active: false,
        }
    }
}

/// Hasil evaluasi translasi NAT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatAction {
    Untouched,
    TranslateSnat { new_src_ip: [u8; 4], new_src_port: u16 },
    TranslateDnat { new_dst_ip: [u8; 4], new_dst_port: u16 },
    DropTableFull,
}

/// Mesin Utama NAT & Conntrack
pub struct NatEngine {
    pub wan_ip: [u8; 4],
    pub conntrack: [ConntrackEntry; MAX_CONNTRACK_ENTRIES],
    pub dnat_rules: [DnatRule; MAX_DNAT_RULES],
    pub next_ephemeral_port: u16,
    pub active_sessions: usize,
    pub table_full_drops: u32,
    pub current_tick: u32,
}

impl NatEngine {
    pub const fn new(wan_ip: [u8; 4]) -> Self {
        Self {
            wan_ip,
            conntrack: [ConntrackEntry::empty(); MAX_CONNTRACK_ENTRIES],
            dnat_rules: [DnatRule::empty(); MAX_DNAT_RULES],
            next_ephemeral_port: 40000,
            active_sessions: 0,
            table_full_drops: 0,
            current_tick: 0,
        }
    }

    pub fn set_wan_ip(&mut self, wan_ip: [u8; 4]) {
        self.wan_ip = wan_ip;
    }

    pub fn update_clock(&mut self, current_tick: u32) {
        self.current_tick = current_tick;
        self.evict_expired_entries();
    }

    /// Bersihkan entri conntrack yang telah kadaluarsa
    pub fn evict_expired_entries(&mut self) {
        for i in 0..MAX_CONNTRACK_ENTRIES {
            if self.conntrack[i].state != ConntrackState::Unused && self.current_tick >= self.conntrack[i].expiry_tick {
                self.conntrack[i] = ConntrackEntry::empty();
                if self.active_sessions > 0 {
                    self.active_sessions -= 1;
                }
            }
        }
    }

    /// Daftarkan aturan DNAT (Port Forwarding)
    pub fn add_dnat_rule(
        &mut self,
        wan_port: u16,
        protocol: u8,
        target_lan_ip: [u8; 4],
        target_lan_port: u16,
    ) -> Result<(), &'static str> {
        for i in 0..MAX_DNAT_RULES {
            if !self.dnat_rules[i].is_active {
                self.dnat_rules[i] = DnatRule {
                    wan_ip: self.wan_ip,
                    wan_port,
                    protocol,
                    target_lan_ip,
                    target_lan_port,
                    is_active: true,
                };
                return Ok(());
            }
        }
        Err("DNAT rule capacity full")
    }

    /// Evaluasi dan terjemahkan paket Egress (LAN -> WAN Masquerade SNAT)
    pub fn process_egress(
        &mut self,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        src_port: u16,
        dst_port: u16,
        protocol: u8,
    ) -> NatAction {
        // 1. Cek sesi conntrack yang sudah ada
        for i in 0..MAX_CONNTRACK_ENTRIES {
            let entry = &mut self.conntrack[i];
            if entry.state != ConntrackState::Unused
                && entry.protocol == protocol
                && entry.orig_src_ip == src_ip
                && entry.orig_dst_ip == dst_ip
                && entry.orig_src_port == src_port
                && entry.orig_dst_port == dst_port
            {
                entry.packets_fwd = entry.packets_fwd.wrapping_add(1);
                entry.expiry_tick = self.current_tick + Self::timeout_for_proto(protocol);
                return NatAction::TranslateSnat {
                    new_src_ip: entry.trans_src_ip,
                    new_src_port: entry.trans_src_port,
                };
            }
        }

        // 2. Buat sesi SNAT baru jika belum ada
        let alloc_port = self.allocate_ephemeral_port();
        let timeout = Self::timeout_for_proto(protocol);

        for i in 0..MAX_CONNTRACK_ENTRIES {
            if self.conntrack[i].state == ConntrackState::Unused {
                self.conntrack[i] = ConntrackEntry {
                    orig_src_ip: src_ip,
                    orig_dst_ip: dst_ip,
                    orig_src_port: src_port,
                    orig_dst_port: dst_port,
                    protocol,
                    trans_src_ip: self.wan_ip,
                    trans_dst_ip: dst_ip,
                    trans_src_port: alloc_port,
                    trans_dst_port: dst_port,
                    state: ConntrackState::Established,
                    expiry_tick: self.current_tick + timeout,
                    packets_fwd: 1,
                    packets_rev: 0,
                };
                self.active_sessions += 1;
                return NatAction::TranslateSnat {
                    new_src_ip: self.wan_ip,
                    new_src_port: alloc_port,
                };
            }
        }

        self.table_full_drops = self.table_full_drops.wrapping_add(1);
        NatAction::DropTableFull
    }

    /// Evaluasi dan terjemahkan paket Ingress (WAN -> LAN Reverse Path atau DNAT)
    pub fn process_ingress(
        &mut self,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        src_port: u16,
        dst_port: u16,
        protocol: u8,
    ) -> NatAction {
        // 1. Cek apakah ini reverse path dari koneksi SNAT aktif
        for i in 0..MAX_CONNTRACK_ENTRIES {
            let entry = &mut self.conntrack[i];
            if entry.state != ConntrackState::Unused
                && entry.protocol == protocol
                && entry.trans_dst_ip == src_ip
                && entry.trans_src_port == dst_port
            {
                entry.packets_rev = entry.packets_rev.wrapping_add(1);
                entry.expiry_tick = self.current_tick + Self::timeout_for_proto(protocol);
                return NatAction::TranslateDnat {
                    new_dst_ip: entry.orig_src_ip,
                    new_dst_port: entry.orig_src_port,
                };
            }
        }

        // 2. Cek apakah mencocoki aturan DNAT (Port Forwarding)
        for i in 0..MAX_DNAT_RULES {
            let rule = &self.dnat_rules[i];
            if rule.is_active
                && rule.protocol == protocol
                && (rule.wan_ip == dst_ip || dst_ip == self.wan_ip)
                && rule.wan_port == dst_port
            {
                // Catat di conntrack untuk return path
                let timeout = Self::timeout_for_proto(protocol);
                for c in 0..MAX_CONNTRACK_ENTRIES {
                    if self.conntrack[c].state == ConntrackState::Unused {
                        self.conntrack[c] = ConntrackEntry {
                            orig_src_ip: src_ip,
                            orig_dst_ip: dst_ip,
                            orig_src_port: src_port,
                            orig_dst_port: dst_port,
                            protocol,
                            trans_src_ip: src_ip,
                            trans_dst_ip: rule.target_lan_ip,
                            trans_src_port: src_port,
                            trans_dst_port: rule.target_lan_port,
                            state: ConntrackState::Established,
                            expiry_tick: self.current_tick + timeout,
                            packets_fwd: 1,
                            packets_rev: 0,
                        };
                        self.active_sessions += 1;
                        break;
                    }
                }

                return NatAction::TranslateDnat {
                    new_dst_ip: rule.target_lan_ip,
                    new_dst_port: rule.target_lan_port,
                };
            }
        }

        NatAction::Untouched
    }

    fn allocate_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port >= 65000 {
            40000
        } else {
            self.next_ephemeral_port + 1
        };
        port
    }

    fn timeout_for_proto(proto: u8) -> u32 {
        match proto {
            PROTO_TCP => DEFAULT_TCP_TIMEOUT_TICKS,
            PROTO_UDP => DEFAULT_UDP_TIMEOUT_TICKS,
            PROTO_ICMP => DEFAULT_ICMP_TIMEOUT_TICKS,
            _ => 1000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_snat_forward_and_reverse_path() {
        let wan_ip = [192, 168, 57, 10]; // IP WAN Rinjani
        let mut nat = NatEngine::new(wan_ip);

        // Klien LAN (192.168.56.2:12345) -> Host WAN (192.168.57.1:80)
        let lan_src = [192, 168, 56, 2];
        let wan_dst = [192, 168, 57, 1];
        let act = nat.process_egress(lan_src, wan_dst, 12345, 80, PROTO_TCP);

        let trans_port = match act {
            NatAction::TranslateSnat { new_src_ip, new_src_port } => {
                assert_eq!(new_src_ip, wan_ip);
                assert!(new_src_port >= 40000);
                new_src_port
            }
            _ => panic!("Expected TranslateSnat"),
        };

        // Balasan dari WAN (192.168.57.1:80 -> WAN_IP:trans_port)
        let rev_act = nat.process_ingress(wan_dst, wan_ip, 80, trans_port, PROTO_TCP);
        match rev_act {
            NatAction::TranslateDnat { new_dst_ip, new_dst_port } => {
                assert_eq!(new_dst_ip, lan_src);
                assert_eq!(new_dst_port, 12345);
            }
            _ => panic!("Expected TranslateDnat for reverse path"),
        }
    }

    #[test]
    fn test_02_dnat_port_forward() {
        let wan_ip = [192, 168, 57, 10];
        let mut nat = NatEngine::new(wan_ip);
        // Rule: WAN UDP 7777 -> Klien LAN 192.168.56.2:7777
        nat.add_dnat_rule(7777, PROTO_UDP, [192, 168, 56, 2], 7777).unwrap();

        // Paket dari WAN Host (192.168.57.1:55555) -> Rinjani WAN (192.168.57.10:7777)
        let act = nat.process_ingress([192, 168, 57, 1], wan_ip, 55555, 7777, PROTO_UDP);
        match act {
            NatAction::TranslateDnat { new_dst_ip, new_dst_port } => {
                assert_eq!(new_dst_ip, [192, 168, 56, 2]);
                assert_eq!(new_dst_port, 7777);
            }
            _ => panic!("Expected DNAT translation"),
        }
    }

    #[test]
    fn test_03_icmp_tracking() {
        let wan_ip = [192, 168, 57, 10];
        let mut nat = NatEngine::new(wan_ip);

        // ICMP Echo LAN Klien (ID=0x1122) -> WAN
        let act = nat.process_egress([192, 168, 56, 2], [192, 168, 57, 1], 0x1122, 0, PROTO_ICMP);
        let trans_id = match act {
            NatAction::TranslateSnat { new_src_ip, new_src_port } => {
                assert_eq!(new_src_ip, wan_ip);
                new_src_port
            }
            _ => panic!("Expected ICMP SNAT"),
        };

        // Balasan ICMP Reply WAN -> Rinjani
        let rev = nat.process_ingress([192, 168, 57, 1], wan_ip, 0, trans_id, PROTO_ICMP);
        match rev {
            NatAction::TranslateDnat { new_dst_ip, new_dst_port } => {
                assert_eq!(new_dst_ip, [192, 168, 56, 2]);
                assert_eq!(new_dst_port, 0x1122);
            }
            _ => panic!("Expected ICMP Reverse DNAT"),
        }
    }

    #[test]
    fn test_04_timeout_eviction() {
        let wan_ip = [192, 168, 57, 10];
        let mut nat = NatEngine::new(wan_ip);
        nat.process_egress([192, 168, 56, 2], [192, 168, 57, 1], 1000, 80, PROTO_UDP);
        assert_eq!(nat.active_sessions, 1);

        // Majukan waktu melewati timeout UDP (3000 ticks)
        nat.update_clock(4000);
        assert_eq!(nat.active_sessions, 0);
    }

    #[test]
    fn test_05_conntrack_table_full_drop() {
        let wan_ip = [192, 168, 57, 10];
        let mut nat = NatEngine::new(wan_ip);
        for i in 0..MAX_CONNTRACK_ENTRIES {
            let act = nat.process_egress([192, 168, 56, 2], [10, 0, 0, 1], (1000 + i) as u16, 80, PROTO_TCP);
            assert!(matches!(act, NatAction::TranslateSnat { .. }));
        }
        assert_eq!(nat.active_sessions, MAX_CONNTRACK_ENTRIES);

        // Entri berikutnya harus ditolak politely dan counter drop bertambah
        let drop_act = nat.process_egress([192, 168, 56, 2], [10, 0, 0, 1], 9999, 80, PROTO_TCP);
        assert_eq!(drop_act, NatAction::DropTableFull);
        assert_eq!(nat.table_full_drops, 1);
    }

    #[test]
    fn test_06_unmatched_ingress_untouched() {
        let wan_ip = [192, 168, 57, 10];
        let mut nat = NatEngine::new(wan_ip);
        // Paket asing ke port yang tidak ada di conntrack atau DNAT
        let act = nat.process_ingress([1, 2, 3, 4], wan_ip, 1234, 9999, PROTO_TCP);
        assert_eq!(act, NatAction::Untouched);
    }

    #[test]
    fn test_07_ephemeral_port_allocation_wrap() {
        let mut nat = NatEngine::new([10, 0, 0, 1]);
        assert_eq!(nat.next_ephemeral_port, 40000);
        let p1 = nat.allocate_ephemeral_port();
        assert_eq!(p1, 40000);
        assert_eq!(nat.next_ephemeral_port, 40001);

        nat.next_ephemeral_port = 65000;
        let p_wrap = nat.allocate_ephemeral_port();
        assert_eq!(p_wrap, 65000);
        assert_eq!(nat.next_ephemeral_port, 40000);
    }

    #[test]
    fn test_08_set_wan_ip_dynamic() {
        let mut nat = NatEngine::new([10, 0, 0, 1]);
        assert_eq!(nat.wan_ip, [10, 0, 0, 1]);
        nat.set_wan_ip([192, 168, 57, 99]);
        assert_eq!(nat.wan_ip, [192, 168, 57, 99]);
    }
}
