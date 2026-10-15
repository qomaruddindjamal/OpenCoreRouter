//! Open Core Router (OpenCR) — asl-router-lpm
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Longest Prefix Match (LPM) Trie & Forwarding Information Base (FIB)
//!
//! MANDATE: Heapless (#![no_std]), no alloc, bounded memory, deterministic lookup.
//! RIB -> FIB synchronization interface.

#![no_std]

pub const MAX_ROUTES: usize = 64;

/// Target antarmuka keluaran paket
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextHop {
    Drop,
    Local,
    Interface(u8), // ID interface: 0 = Loopback/Host, 1 = NIC0 (LAN), 2 = NIC1 (WAN)
    Gateway { interface: u8, next_hop_ip: [u8; 4] },
}

/// Struktur satu entri rute dalam tabel FIB
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteEntry {
    pub network: [u8; 4],
    pub prefix_len: u8,
    pub next_hop: NextHop,
    pub metric: u32,
    pub is_active: bool,
}

impl RouteEntry {
    pub const fn empty() -> Self {
        Self {
            network: [0; 4],
            prefix_len: 0,
            next_hop: NextHop::Drop,
            metric: 0,
            is_active: false,
        }
    }
}

/// Mesin Longest Prefix Match (LPM) berbasis Fixed-Capacity Table
pub struct LpmTable {
    pub routes: [RouteEntry; MAX_ROUTES],
    pub count: usize,
}

impl LpmTable {
    pub const fn new() -> Self {
        Self {
            routes: [RouteEntry::empty(); MAX_ROUTES],
            count: 0,
        }
    }

    /// Menghitung subnet mask 32-bit dari panjang prefiks CIDR (0..32)
    pub fn prefix_to_mask(prefix_len: u8) -> u32 {
        if prefix_len == 0 {
            0
        } else if prefix_len >= 32 {
            0xFFFF_FFFF
        } else {
            !((1u32 << (32 - prefix_len)) - 1)
        }
    }

    /// Konversi IPv4 4-byte slice ke u32 Big-Endian
    pub fn ip_to_u32(ip: [u8; 4]) -> u32 {
        u32::from_be_bytes(ip)
    }

    /// Memeriksa apakah IP target cocok dengan network prefix yang diberikan
    pub fn matches(target_ip: [u8; 4], network: [u8; 4], prefix_len: u8) -> bool {
        let mask = Self::prefix_to_mask(prefix_len);
        let target = Self::ip_to_u32(target_ip);
        let net = Self::ip_to_u32(network);
        (target & mask) == (net & mask)
    }

    /// Insert atau Update rute ke tabel (Single-Writer semantics)
    pub fn insert(
        &mut self,
        network: [u8; 4],
        prefix_len: u8,
        next_hop: NextHop,
        metric: u32,
    ) -> Result<(), &'static str> {
        let prefix = if prefix_len > 32 { 32 } else { prefix_len };
        // Normalisasi network sesuai mask
        let mask = Self::prefix_to_mask(prefix);
        let norm_net = (Self::ip_to_u32(network) & mask).to_be_bytes();

        // 1. Cek apakah route sudah ada (Update existing)
        for i in 0..self.count {
            if self.routes[i].is_active
                && self.routes[i].network == norm_net
                && self.routes[i].prefix_len == prefix
            {
                self.routes[i].next_hop = next_hop;
                self.routes[i].metric = metric;
                return Ok(());
            }
        }

        // 2. Tambah rute baru jika kapasitas mencukupi
        if self.count >= MAX_ROUTES {
            return Err("LPM table capacity full");
        }

        self.routes[self.count] = RouteEntry {
            network: norm_net,
            prefix_len: prefix,
            next_hop,
            metric,
            is_active: true,
        };
        self.count += 1;
        Ok(())
    }

    /// Hapus rute dari tabel
    pub fn remove(&mut self, network: [u8; 4], prefix_len: u8) -> bool {
        let prefix = if prefix_len > 32 { 32 } else { prefix_len };
        let mask = Self::prefix_to_mask(prefix);
        let norm_net = (Self::ip_to_u32(network) & mask).to_be_bytes();

        for i in 0..self.count {
            if self.routes[i].is_active
                && self.routes[i].network == norm_net
                && self.routes[i].prefix_len == prefix
            {
                // Swap remove
                self.routes[i] = self.routes[self.count - 1];
                self.routes[self.count - 1] = RouteEntry::empty();
                self.count -= 1;
                return true;
            }
        }
        false
    }

    /// Lookup Longest Prefix Match (LPM): Prefiks terpanjang menang; jika sama, metrik terendah menang.
    pub fn lookup(&self, target_ip: [u8; 4]) -> Option<NextHop> {
        let mut best_match: Option<NextHop> = None;
        let mut best_prefix: i32 = -1;
        let mut best_metric: u32 = u32::MAX;

        for i in 0..self.count {
            let entry = &self.routes[i];
            if entry.is_active && Self::matches(target_ip, entry.network, entry.prefix_len) {
                let p_len = entry.prefix_len as i32;
                if p_len > best_prefix || (p_len == best_prefix && entry.metric < best_metric) {
                    best_prefix = p_len;
                    best_metric = entry.metric;
                    best_match = Some(entry.next_hop);
                }
            }
        }

        best_match
    }
}

/// Routing Information Base (RIB) & FIB Synchronization Manager
pub struct RoutingEngine {
    pub fib: LpmTable,
}

impl RoutingEngine {
    pub const fn new() -> Self {
        Self {
            fib: LpmTable::new(),
        }
    }

    pub fn set_default_route(&mut self, next_hop: NextHop) -> Result<(), &'static str> {
        self.fib.insert([0, 0, 0, 0], 0, next_hop, 100)
    }

    pub fn add_connected_route(
        &mut self,
        network: [u8; 4],
        prefix_len: u8,
        interface: u8,
    ) -> Result<(), &'static str> {
        self.fib
            .insert(network, prefix_len, NextHop::Interface(interface), 0)
    }

    pub fn add_static_route(
        &mut self,
        network: [u8; 4],
        prefix_len: u8,
        gateway: [u8; 4],
        interface: u8,
        metric: u32,
    ) -> Result<(), &'static str> {
        self.fib.insert(
            network,
            prefix_len,
            NextHop::Gateway {
                interface,
                next_hop_ip: gateway,
            },
            metric,
        )
    }

    pub fn route_packet(&self, dst_ip: [u8; 4]) -> NextHop {
        self.fib.lookup(dst_ip).unwrap_or(NextHop::Drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_exact_and_default_match() {
        let mut lpm = LpmTable::new();
        // Default route 0.0.0.0/0 -> WAN (Interface 2)
        lpm.insert([0, 0, 0, 0], 0, NextHop::Interface(2), 100)
            .unwrap();
        // Connected LAN 192.168.56.0/24 -> LAN (Interface 1)
        lpm.insert([192, 168, 56, 0], 24, NextHop::Interface(1), 10)
            .unwrap();

        // 192.168.56.100 harus match ke LAN (Interface 1) karena prefix /24 > /0
        let res_lan = lpm.lookup([192, 168, 56, 100]);
        assert_eq!(res_lan, Some(NextHop::Interface(1)));

        // 8.8.8.8 harus match ke WAN (Interface 2) via default route
        let res_wan = lpm.lookup([8, 8, 8, 8]);
        assert_eq!(res_wan, Some(NextHop::Interface(2)));
    }

    #[test]
    fn test_02_longest_prefix_wins() {
        let mut lpm = LpmTable::new();
        // Subnet besar /16 -> Interface 1
        lpm.insert([10, 0, 0, 0], 16, NextHop::Interface(1), 20)
            .unwrap();
        // Subnet kecil /24 -> Interface 2 (lebih spesifik)
        lpm.insert([10, 0, 2, 0], 24, NextHop::Interface(2), 10)
            .unwrap();
        // Host route /32 -> Local
        lpm.insert([10, 0, 2, 15], 32, NextHop::Local, 5).unwrap();

        assert_eq!(lpm.lookup([10, 0, 5, 1]), Some(NextHop::Interface(1)));
        assert_eq!(lpm.lookup([10, 0, 2, 99]), Some(NextHop::Interface(2)));
        assert_eq!(lpm.lookup([10, 0, 2, 15]), Some(NextHop::Local));
    }

    #[test]
    fn test_03_route_update_existing() {
        let mut lpm = LpmTable::new();
        lpm.insert([10, 0, 0, 0], 24, NextHop::Interface(1), 10)
            .unwrap();
        assert_eq!(lpm.count, 1);
        assert_eq!(lpm.lookup([10, 0, 0, 5]), Some(NextHop::Interface(1)));

        // Update ke Interface 3
        lpm.insert([10, 0, 0, 0], 24, NextHop::Interface(3), 5)
            .unwrap();
        assert_eq!(lpm.count, 1); // Kapasitas tidak bertambah
        assert_eq!(lpm.lookup([10, 0, 0, 5]), Some(NextHop::Interface(3)));
    }

    #[test]
    fn test_04_route_remove() {
        let mut lpm = LpmTable::new();
        lpm.insert([192, 168, 1, 0], 24, NextHop::Interface(1), 10)
            .unwrap();
        assert_eq!(lpm.lookup([192, 168, 1, 50]), Some(NextHop::Interface(1)));

        let removed = lpm.remove([192, 168, 1, 0], 24);
        assert!(removed);
        assert_eq!(lpm.count, 0);
        assert_eq!(lpm.lookup([192, 168, 1, 50]), None);
    }

    #[test]
    fn test_05_table_overflow_error() {
        let mut lpm = LpmTable::new();
        for i in 0..MAX_ROUTES {
            lpm.insert([10, i as u8, 0, 0], 24, NextHop::Interface(1), 10)
                .unwrap();
        }
        assert_eq!(lpm.count, MAX_ROUTES);
        // Entri ke-(MAX_ROUTES+1) harus ditolak politely
        let err = lpm.insert([10, 255, 0, 0], 24, NextHop::Interface(1), 10);
        assert!(err.is_err());
    }

    #[test]
    fn test_06_routing_engine_integration() {
        let mut re = RoutingEngine::new();
        re.set_default_route(NextHop::Interface(2)).unwrap();
        re.add_connected_route([192, 168, 56, 0], 24, 1).unwrap();

        assert_eq!(re.route_packet([192, 168, 56, 1]), NextHop::Interface(1));
        assert_eq!(re.route_packet([1, 1, 1, 1]), NextHop::Interface(2));
    }

    #[test]
    fn test_07_prefix_to_mask_boundaries() {
        assert_eq!(LpmTable::prefix_to_mask(0), 0);
        assert_eq!(LpmTable::prefix_to_mask(8), 0xFF00_0000);
        assert_eq!(LpmTable::prefix_to_mask(16), 0xFFFF_0000);
        assert_eq!(LpmTable::prefix_to_mask(24), 0xFFFF_FF00);
        assert_eq!(LpmTable::prefix_to_mask(32), 0xFFFF_FFFF);
    }

    #[test]
    fn test_08_metric_priority_on_tie() {
        let mut lpm = LpmTable::new();
        // Dua default route, satu metrik 200, satu metrik 50
        lpm.insert([0, 0, 0, 0], 0, NextHop::Interface(1), 200)
            .unwrap();
        // Rute spesifik dengan prefix sama namun beda network/metrik
        lpm.insert([10, 10, 0, 0], 16, NextHop::Interface(2), 100)
            .unwrap();
        lpm.insert([10, 10, 0, 0], 16, NextHop::Interface(3), 50)
            .unwrap(); // Update metrik lebih rendah

        assert_eq!(lpm.lookup([10, 10, 1, 1]), Some(NextHop::Interface(3)));
    }

    #[test]
    fn test_09_forward_multihop_gateway() {
        let mut re = RoutingEngine::new();
        // Route default ke Gateway 192.168.57.1 di interface WAN (2)
        assert!(re
            .add_static_route([0, 0, 0, 0], 0, [192, 168, 57, 1], 2, 100)
            .is_ok());
        // Route langsung ke LAN (1)
        assert!(re.add_connected_route([192, 168, 56, 0], 24, 1).is_ok());

        assert_eq!(
            re.route_packet([8, 8, 8, 8]),
            NextHop::Gateway {
                interface: 2,
                next_hop_ip: [192, 168, 57, 1]
            }
        );
        assert_eq!(re.route_packet([192, 168, 56, 50]), NextHop::Interface(1));
    }

    #[test]
    fn test_10_drop_unreachable_when_no_default_route() {
        let mut re = RoutingEngine::new();
        assert!(re.add_connected_route([10, 0, 0, 0], 8, 1).is_ok());
        assert_eq!(re.route_packet([192, 168, 1, 1]), NextHop::Drop);
    }
}
