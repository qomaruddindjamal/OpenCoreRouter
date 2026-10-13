//! Open Core Router (OpenCR) — opcr-tcam::engine
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! IPv4 5-Tuple Ternary Content Addressable Memory Engine

pub const OS_MARKER: &str = "RINJANI_1_0_3";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Permit,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleKey5T {
    pub src_ip: u32,
    pub src_mask: u32,
    pub dst_ip: u32,
    pub dst_mask: u32,
    pub src_port_min: u16,
    pub src_port_max: u16,
    pub dst_port_min: u16,
    pub dst_port_max: u16,
    pub proto: u8,
    pub proto_mask: u8,
}

impl RuleKey5T {
    pub const fn any() -> Self {
        Self {
            src_ip: 0,
            src_mask: 0,
            dst_ip: 0,
            dst_mask: 0,
            src_port_min: 0,
            src_port_max: 0xFFFF,
            dst_port_min: 0,
            dst_port_max: 0xFFFF,
            proto: 0,
            proto_mask: 0,
        }
    }

    pub fn matches(&self, packet: &Packet5T) -> bool {
        if (packet.src_ip & self.src_mask) != (self.src_ip & self.src_mask) {
            return false;
        }
        if (packet.dst_ip & self.dst_mask) != (self.dst_ip & self.dst_mask) {
            return false;
        }
        if packet.src_port < self.src_port_min || packet.src_port > self.src_port_max {
            return false;
        }
        if packet.dst_port < self.dst_port_min || packet.dst_port > self.dst_port_max {
            return false;
        }
        if (packet.proto & self.proto_mask) != (self.proto & self.proto_mask) {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet5T {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcamRule {
    pub key: RuleKey5T,
    pub action: RuleAction,
    pub priority: u32, // Lower value = higher priority
}

pub struct TcamEngine<const MAX_RULES: usize> {
    rules: [Option<TcamRule>; MAX_RULES],
}

impl<const MAX_RULES: usize> TcamEngine<MAX_RULES> {
    pub const fn new() -> Self {
        Self {
            rules: [None; MAX_RULES],
        }
    }

    pub fn add_rule(&mut self, rule: TcamRule) -> Result<(), &'static str> {
        let mut i = 0;
        while i < MAX_RULES {
            if self.rules[i].is_none() {
                self.rules[i] = Some(rule);
                return Ok(());
            }
            i += 1;
        }
        Err("TCAM table full")
    }

    pub fn classify(&self, packet: &Packet5T) -> RuleAction {
        let mut best_match: Option<TcamRule> = None;
        let mut i = 0;
        while i < MAX_RULES {
            if let Some(rule) = self.rules[i] {
                if rule.key.matches(packet) {
                    match best_match {
                        None => best_match = Some(rule),
                        Some(current) => {
                            if rule.priority < current.priority {
                                best_match = Some(rule);
                            }
                        }
                    }
                }
            }
            i += 1;
        }

        match best_match {
            Some(rule) => rule.action,
            None => RuleAction::Deny, // Default-Deny Security Invariant
        }
    }
}

/// Helper untuk membuat IPv4 integer
pub const fn make_ipv4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}

/// Helper membuat subnet mask dari prefix CIDR
pub const fn cidr_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else if prefix >= 32 {
        0xFFFF_FFFF
    } else {
        !((1u32 << (32 - prefix)) - 1)
    }
}
