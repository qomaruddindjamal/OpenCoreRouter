//! Open Core Router (OpenCR) — asl-router-persistence
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! RJCFG01 Configuration Journal Subsystem with Dual-Slot A/B Atomic Switch
//! Supports hostname, static DNS hosts, DHCP pool configuration, and interface profiles.

#![no_std]

use asl_router_gpt::compute_crc32;

pub const RJCFG_MAGIC: [u8; 8] = *b"RJCFG01\0";
pub const RJCFG_HEADER_SIZE: usize = 32;
pub const RJCFG_SLOT_SIZE: usize = 4096; // 4 KiB per slot

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RjcfgHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub active_slot: u8, // 0 = Slot A, 1 = Slot B
    pub reserved: [u8; 3],
    pub sequence_number: u32,
    pub slot_a_crc32: u32,
    pub slot_b_crc32: u32,
    pub header_crc32: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RjcfgConfigPayload {
    pub hostname: [u8; 32],
    pub hostname_len: u8,
    pub domain: [u8; 32],
    pub domain_len: u8,
    pub lan_ip: [u8; 4],
    pub dhcp_pool_start: [u8; 4],
    pub dhcp_pool_end: [u8; 4],
    pub dns_upstream: [u8; 4],
    pub static_dns_host: [u8; 32],
    pub static_dns_host_len: u8,
    pub static_dns_ip: [u8; 4],
}

impl RjcfgConfigPayload {
    pub const fn default_config() -> Self {
        let mut hostname = [0u8; 32];
        let h_str = b"rinjani";
        let mut i = 0;
        while i < h_str.len() {
            hostname[i] = h_str[i];
            i += 1;
        }

        let mut domain = [0u8; 32];
        let d_str = b"rinjani.local";
        let mut j = 0;
        while j < d_str.len() {
            domain[j] = d_str[j];
            j += 1;
        }

        let mut static_dns_host = [0u8; 32];
        let s_str = b"rinjani.lan";
        let mut k = 0;
        while k < s_str.len() {
            static_dns_host[k] = s_str[k];
            k += 1;
        }

        Self {
            hostname,
            hostname_len: h_str.len() as u8,
            domain,
            domain_len: d_str.len() as u8,
            lan_ip: [192, 168, 56, 1],
            dhcp_pool_start: [192, 168, 56, 100],
            dhcp_pool_end: [192, 168, 56, 150],
            dns_upstream: [10, 10, 66, 254],
            static_dns_host,
            static_dns_host_len: s_str.len() as u8,
            static_dns_ip: [192, 168, 56, 1],
        }
    }

    pub fn serialize(&self, out: &mut [u8; RJCFG_SLOT_SIZE]) {
        for b in out.iter_mut() {
            *b = 0;
        }
        out[0] = self.hostname_len;
        out[1..33].copy_from_slice(&self.hostname);
        out[33] = self.domain_len;
        out[34..66].copy_from_slice(&self.domain);
        out[66..70].copy_from_slice(&self.lan_ip);
        out[70..74].copy_from_slice(&self.dhcp_pool_start);
        out[74..78].copy_from_slice(&self.dhcp_pool_end);
        out[78..82].copy_from_slice(&self.dns_upstream);
        out[82] = self.static_dns_host_len;
        out[83..115].copy_from_slice(&self.static_dns_host);
        out[115..119].copy_from_slice(&self.static_dns_ip);
    }

    pub fn deserialize(buf: &[u8; RJCFG_SLOT_SIZE]) -> Result<Self, &'static str> {
        let hostname_len = buf[0];
        if hostname_len > 32 {
            return Err("Invalid hostname length");
        }
        let mut hostname = [0u8; 32];
        hostname.copy_from_slice(&buf[1..33]);

        let domain_len = buf[33];
        if domain_len > 32 {
            return Err("Invalid domain length");
        }
        let mut domain = [0u8; 32];
        domain.copy_from_slice(&buf[34..66]);

        let mut lan_ip = [0u8; 4];
        lan_ip.copy_from_slice(&buf[66..70]);

        let mut dhcp_pool_start = [0u8; 4];
        dhcp_pool_start.copy_from_slice(&buf[70..74]);

        let mut dhcp_pool_end = [0u8; 4];
        dhcp_pool_end.copy_from_slice(&buf[74..78]);

        let mut dns_upstream = [0u8; 4];
        dns_upstream.copy_from_slice(&buf[78..82]);

        let static_dns_host_len = buf[82];
        if static_dns_host_len > 32 {
            return Err("Invalid static DNS host length");
        }
        let mut static_dns_host = [0u8; 32];
        static_dns_host.copy_from_slice(&buf[83..115]);

        let mut static_dns_ip = [0u8; 4];
        static_dns_ip.copy_from_slice(&buf[115..119]);

        Ok(Self {
            hostname,
            hostname_len,
            domain,
            domain_len,
            lan_ip,
            dhcp_pool_start,
            dhcp_pool_end,
            dns_upstream,
            static_dns_host,
            static_dns_host_len,
            static_dns_ip,
        })
    }
}

/// A/B Journal Persistence Manager
pub struct RjcfgJournal {
    pub header: RjcfgHeader,
    pub slot_a: [u8; RJCFG_SLOT_SIZE],
    pub slot_b: [u8; RJCFG_SLOT_SIZE],
    pub active_config: RjcfgConfigPayload,
}

impl RjcfgJournal {
    pub fn new_default() -> Self {
        let active_config = RjcfgConfigPayload::default_config();
        let mut slot_a = [0u8; RJCFG_SLOT_SIZE];
        let slot_b = [0u8; RJCFG_SLOT_SIZE];
        active_config.serialize(&mut slot_a);

        let slot_a_crc32 = compute_crc32(&slot_a);
        let slot_b_crc32 = compute_crc32(&slot_b);

        let mut header = RjcfgHeader {
            magic: RJCFG_MAGIC,
            version: 1,
            active_slot: 0, // Slot A
            reserved: [0; 3],
            sequence_number: 1,
            slot_a_crc32,
            slot_b_crc32,
            header_crc32: 0,
        };

        let raw_hdr = unsafe {
            core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                RJCFG_HEADER_SIZE - 4, // Exclude header_crc32 itself
            )
        };
        header.header_crc32 = compute_crc32(raw_hdr);

        Self {
            header,
            slot_a,
            slot_b,
            active_config,
        }
    }

    /// Commit modified configuration atomically to the opposite slot
    pub fn commit_update(&mut self, new_config: RjcfgConfigPayload) -> Result<u8, &'static str> {
        let current_slot = self.header.active_slot;
        let target_slot = if current_slot == 0 { 1 } else { 0 };

        let mut temp_slot = [0u8; RJCFG_SLOT_SIZE];
        new_config.serialize(&mut temp_slot);
        let new_crc = compute_crc32(&temp_slot);

        if target_slot == 1 {
            self.slot_b = temp_slot;
            self.header.slot_b_crc32 = new_crc;
        } else {
            self.slot_a = temp_slot;
            self.header.slot_a_crc32 = new_crc;
        }

        self.header.sequence_number = self.header.sequence_number.wrapping_add(1);
        self.header.active_slot = target_slot;

        // Recompute header CRC
        let raw_hdr = unsafe {
            core::slice::from_raw_parts(
                &self.header as *const _ as *const u8,
                RJCFG_HEADER_SIZE - 4,
            )
        };
        self.header.header_crc32 = compute_crc32(raw_hdr);

        self.active_config = new_config;
        Ok(target_slot)
    }

    /// Update hostname in active config and commit
    pub fn set_hostname(&mut self, new_hostname: &str) -> Result<u8, &'static str> {
        if new_hostname.is_empty() || new_hostname.len() > 32 {
            return Err("Hostname must be 1..32 characters");
        }
        let mut cfg = self.active_config;
        for b in cfg.hostname.iter_mut() {
            *b = 0;
        }
        let bytes = new_hostname.as_bytes();
        cfg.hostname[..bytes.len()].copy_from_slice(bytes);
        cfg.hostname_len = bytes.len() as u8;
        self.commit_update(cfg)
    }

    pub fn get_hostname_str<'a>(&'a self, out_buf: &'a mut [u8; 32]) -> &'a str {
        let len = self.active_config.hostname_len as usize;
        out_buf[..len].copy_from_slice(&self.active_config.hostname[..len]);
        core::str::from_utf8(&out_buf[..len]).unwrap_or("rinjani")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_default_config_values() {
        let journal = RjcfgJournal::new_default();
        let mut buf = [0u8; 32];
        let name = journal.get_hostname_str(&mut buf);
        assert_eq!(name, "rinjani");
        assert_eq!(journal.active_config.lan_ip, [192, 168, 56, 1]);
        assert_eq!(journal.header.active_slot, 0);
    }

    #[test]
    fn test_02_atomic_slot_switch_a_to_b() {
        let mut journal = RjcfgJournal::new_default();
        assert_eq!(journal.header.active_slot, 0);

        let target_slot = journal.set_hostname("rinjani-router").expect("hostname set ok");
        assert_eq!(target_slot, 1);
        assert_eq!(journal.header.active_slot, 1);
        let seq = journal.header.sequence_number;
        assert_eq!(seq, 2);

        let mut buf = [0u8; 32];
        assert_eq!(journal.get_hostname_str(&mut buf), "rinjani-router");
    }

    #[test]
    fn test_03_atomic_slot_switch_b_to_a() {
        let mut journal = RjcfgJournal::new_default();
        journal.set_hostname("test-x").unwrap();
        assert_eq!(journal.header.active_slot, 1);

        let target_slot = journal.set_hostname("rinjani").unwrap();
        assert_eq!(target_slot, 0);
        assert_eq!(journal.header.active_slot, 0);
        let seq = journal.header.sequence_number;
        assert_eq!(seq, 3);
        let mut buf = [0u8; 32];
        assert_eq!(journal.get_hostname_str(&mut buf), "rinjani");
    }

    #[test]
    fn test_04_serialization_roundtrip() {
        let cfg = RjcfgConfigPayload::default_config();
        let mut slot_buf = [0u8; RJCFG_SLOT_SIZE];
        cfg.serialize(&mut slot_buf);

        let parsed = RjcfgConfigPayload::deserialize(&slot_buf).expect("deserialize ok");
        assert_eq!(parsed.hostname_len, cfg.hostname_len);
        assert_eq!(parsed.hostname, cfg.hostname);
        assert_eq!(parsed.lan_ip, cfg.lan_ip);
        assert_eq!(parsed.dns_upstream, cfg.dns_upstream);
    }

    #[test]
    fn test_05_hostname_length_bounds() {
        let mut journal = RjcfgJournal::new_default();
        assert_eq!(journal.set_hostname(""), Err("Hostname must be 1..32 characters"));
        assert_eq!(journal.set_hostname("this-hostname-is-definitely-far-too-long-for-rinjani-32-bytes"), Err("Hostname must be 1..32 characters"));
    }

    #[test]
    fn test_06_crc_recomputation_integrity() {
        let mut journal = RjcfgJournal::new_default();
        let crc_a = journal.header.slot_a_crc32;
        journal.set_hostname("new-name").unwrap();
        let crc_b = journal.header.slot_b_crc32;
        assert_ne!(crc_a, crc_b);
    }
}
