//! Open Core Router (OpenCR) — asl-router-tcam
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA

#![no_std]

pub mod engine;

pub use engine::{
    RuleAction, RuleKey5T, Packet5T, TcamRule, TcamEngine, make_ipv4, cidr_mask
};

pub const OS_MARKER: &str = "RINJANI_1_0_3";
pub const MODULE_NAME: &str = "tcam";

pub fn module_status() -> &'static str {
    OS_MARKER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_marker() {
        assert_eq!(module_status(), "RINJANI_1_0_3");
    }

    #[test]
    fn test_tcam_vector_1_exact_match() {
        let mut engine: TcamEngine<16> = TcamEngine::new();
        let mut key = RuleKey5T::any();
        key.src_ip = make_ipv4(10, 10, 66, 228);
        key.src_mask = 0xFFFF_FFFF;
        key.dst_ip = make_ipv4(8, 8, 8, 8);
        key.dst_mask = 0xFFFF_FFFF;
        key.dst_port_min = 53;
        key.dst_port_max = 53;
        key.proto = 17; // UDP
        key.proto_mask = 0xFF;

        engine.add_rule(TcamRule {
            key,
            action: RuleAction::Permit,
            priority: 10,
        }).unwrap();

        let pkt_match = Packet5T {
            src_ip: make_ipv4(10, 10, 66, 228),
            dst_ip: make_ipv4(8, 8, 8, 8),
            src_port: 50400,
            dst_port: 53,
            proto: 17,
        };
        assert_eq!(engine.classify(&pkt_match), RuleAction::Permit);
    }

    #[test]
    fn test_tcam_vector_2_subnet_match() {
        let mut engine: TcamEngine<16> = TcamEngine::new();
        let mut key = RuleKey5T::any();
        key.src_ip = make_ipv4(10, 10, 1, 0);
        key.src_mask = cidr_mask(24); // 255.255.255.0

        engine.add_rule(TcamRule {
            key,
            action: RuleAction::Permit,
            priority: 100,
        }).unwrap();

        let pkt_in_subnet = Packet5T {
            src_ip: make_ipv4(10, 10, 1, 45),
            dst_ip: make_ipv4(1, 1, 1, 1),
            src_port: 1024,
            dst_port: 80,
            proto: 6,
        };
        assert_eq!(engine.classify(&pkt_in_subnet), RuleAction::Permit);
    }

    #[test]
    fn test_tcam_vector_3_wildcard_match() {
        let mut engine: TcamEngine<16> = TcamEngine::new();
        let key = RuleKey5T::any(); // Match all

        engine.add_rule(TcamRule {
            key,
            action: RuleAction::Permit,
            priority: 999,
        }).unwrap();

        let pkt = Packet5T {
            src_ip: make_ipv4(192, 168, 1, 1),
            dst_ip: make_ipv4(172, 16, 0, 1),
            src_port: 1234,
            dst_port: 4321,
            proto: 6,
        };
        assert_eq!(engine.classify(&pkt), RuleAction::Permit);
    }

    #[test]
    fn test_tcam_vector_4_priority_conflict_resolution() {
        let mut engine: TcamEngine<16> = TcamEngine::new();

        // Rule prioritas rendah: Permit all 10.0.0.0/8
        let mut key_broad = RuleKey5T::any();
        key_broad.src_ip = make_ipv4(10, 0, 0, 0);
        key_broad.src_mask = cidr_mask(8);
        engine.add_rule(TcamRule {
            key: key_broad,
            action: RuleAction::Permit,
            priority: 200,
        }).unwrap();

        // Rule prioritas tinggi: Deny IP spesifik 10.10.66.50
        let mut key_narrow = RuleKey5T::any();
        key_narrow.src_ip = make_ipv4(10, 10, 66, 50);
        key_narrow.src_mask = 0xFFFF_FFFF;
        engine.add_rule(TcamRule {
            key: key_narrow,
            action: RuleAction::Deny,
            priority: 10, // Menang karena prioritas lebih kecil
        }).unwrap();

        let pkt_blocked = Packet5T {
            src_ip: make_ipv4(10, 10, 66, 50),
            dst_ip: make_ipv4(8, 8, 8, 8),
            src_port: 1024,
            dst_port: 80,
            proto: 6,
        };
        assert_eq!(engine.classify(&pkt_blocked), RuleAction::Deny);
    }

    #[test]
    fn test_tcam_vector_5_default_deny() {
        let engine: TcamEngine<16> = TcamEngine::new(); // Tidak ada rule
        let pkt = Packet5T {
            src_ip: make_ipv4(1, 2, 3, 4),
            dst_ip: make_ipv4(5, 6, 7, 8),
            src_port: 80,
            dst_port: 80,
            proto: 6,
        };
        // Invarian default-deny
        assert_eq!(engine.classify(&pkt), RuleAction::Deny);
    }

    #[test]
    fn test_tcam_vector_6_port_range_boundary() {
        let mut engine: TcamEngine<16> = TcamEngine::new();
        let mut key = RuleKey5T::any();
        key.dst_port_min = 8000;
        key.dst_port_max = 8443;
        engine.add_rule(TcamRule {
            key,
            action: RuleAction::Permit,
            priority: 50,
        }).unwrap();

        let pkt_inside = Packet5T {
            src_ip: 0,
            dst_ip: 0,
            src_port: 0,
            dst_port: 8443,
            proto: 6,
        };
        let pkt_outside = Packet5T {
            src_ip: 0,
            dst_ip: 0,
            src_port: 0,
            dst_port: 8444,
            proto: 6,
        };
        assert_eq!(engine.classify(&pkt_inside), RuleAction::Permit);
        assert_eq!(engine.classify(&pkt_outside), RuleAction::Deny);
    }

    #[test]
    fn test_tcam_vector_7_protocol_mismatch() {
        let mut engine: TcamEngine<16> = TcamEngine::new();
        let mut key = RuleKey5T::any();
        key.proto = 6; // TCP only
        key.proto_mask = 0xFF;
        engine.add_rule(TcamRule {
            key,
            action: RuleAction::Permit,
            priority: 50,
        }).unwrap();

        let pkt_udp = Packet5T {
            src_ip: 0,
            dst_ip: 0,
            src_port: 0,
            dst_port: 0,
            proto: 17, // UDP
        };
        assert_eq!(engine.classify(&pkt_udp), RuleAction::Deny);
    }

    #[test]
    fn test_tcam_vector_8_table_overflow_error() {
        let mut engine: TcamEngine<2> = TcamEngine::new();
        let rule = TcamRule {
            key: RuleKey5T::any(),
            action: RuleAction::Permit,
            priority: 100,
        };
        assert!(engine.add_rule(rule).is_ok());
        assert!(engine.add_rule(rule).is_ok());
        assert!(engine.add_rule(rule).is_err());
    }
}