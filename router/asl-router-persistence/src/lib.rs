//! Open Core Router (OpenCR) — asl-router-persistence
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! RJCFG01 Configuration Journal Subsystem with Dual-Slot A/B Atomic Switch
//! Supports hostname, static DNS hosts, DHCP pool configuration, WAN addressing
//! mode (DHCP vs static) and operator-changed interface names.

#![no_std]

use asl_router_gpt::compute_crc32;

pub const RJCFG_MAGIC: [u8; 8] = *b"RJCFG01\0";
pub const RJCFG_HEADER_SIZE: usize = 32;
pub const RJCFG_SLOT_SIZE: usize = 4096; // 4 KiB per slot

/// Penanda blok konfigurasi WAN lanjutan pada payload. Rekam lama (sebelum sprint
/// UI-02) hanya menulis byte 0..119 dan `serialize` selalu menolkan sisanya, jadi
/// byte 119 pada slot lama pasti 0. Nilai 0xA7 berarti blok WAN benar-benar ada.
/// Tag dipakai alih-alih menaikkan `RjcfgHeader.version` karena pembaca menolak
/// versi tidak dikenal — kenaikan versi akan membuat rekam lama ikut tertolak dan
/// hostname pengguna hilang.
pub const RJCFG_WAN_BLOCK_TAG: u8 = 0xA7;
/// Panjang maksimum nama antarmuka yang dapat ditulis WebUI/CLI.
pub const RJCFG_IFACE_NAME_MAX: usize = 16;
/// Mode WAN: alamat diperoleh lewat klien DHCP.
pub const RJCFG_WAN_MODE_DHCP: u8 = 0;
/// Mode WAN: alamat statik dipegang operator.
pub const RJCFG_WAN_MODE_STATIC: u8 = 1;

// Offset blok WAN dalam slot (byte 0..119 dipakai field warisan).
const OFF_WAN_TAG: usize = 119;
const OFF_WAN_MODE: usize = 120;
const OFF_WAN_IP: usize = 121;
const OFF_WAN_MASK: usize = 125;
const OFF_WAN_GW: usize = 129;
const OFF_WAN_NAME_LEN: usize = 133;
const OFF_WAN_NAME: usize = 134;
const OFF_LAN_NAME_LEN: usize = 150;
const OFF_LAN_NAME: usize = 151;
const OFF_WAN_END: usize = 167;
/// Penjaga regresi: ekstensi blok WAN harus tetap muat dalam slot 4096 B dan
/// batas akhir harus cocok dengan susunan offset di atas.
const _: () = assert!(OFF_WAN_END <= RJCFG_SLOT_SIZE);
const _: () = assert!(OFF_WAN_END == OFF_LAN_NAME + RJCFG_IFACE_NAME_MAX);

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
    /// Konfigurasi WAN (blok lanjutan, lihat `RJCFG_WAN_BLOCK_TAG`).
    pub wan_mode: u8,
    pub wan_ip: [u8; 4],
    pub wan_netmask: [u8; 4],
    pub wan_gw: [u8; 4],
    pub wan_name: [u8; RJCFG_IFACE_NAME_MAX],
    pub wan_name_len: u8,
    pub lan_name: [u8; RJCFG_IFACE_NAME_MAX],
    pub lan_name_len: u8,
}

/// Nama antarmuka: huruf/angka kecil, `-`, `_`, `.`; panjang 1..=16. Aturan ini
/// dipakai WebUI dan CLI agar satu sumber kebenaran untuk validasi penamaan.
pub fn iface_name_ok(s: &str) -> bool {
    if s.is_empty() || s.len() > RJCFG_IFACE_NAME_MAX {
        return false;
    }
    s.bytes().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.'
    })
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

        let mut wan_name = [0u8; RJCFG_IFACE_NAME_MAX];
        let wn_str = b"eth0";
        let mut m = 0;
        while m < wn_str.len() {
            wan_name[m] = wn_str[m];
            m += 1;
        }

        let mut lan_name = [0u8; RJCFG_IFACE_NAME_MAX];
        let ln_str = b"eth1";
        let mut p = 0;
        while p < ln_str.len() {
            lan_name[p] = ln_str[p];
            p += 1;
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
            wan_mode: RJCFG_WAN_MODE_DHCP,
            wan_ip: [0; 4],
            wan_netmask: [0; 4],
            wan_gw: [0; 4],
            wan_name,
            wan_name_len: wn_str.len() as u8,
            lan_name,
            lan_name_len: ln_str.len() as u8,
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
        // Blok WAN lanjutan (sprint UI-02): ditulis selalu, dengan tag agar
        // pembaca dapat membedakan rekam warisan (byte 119 == 0) dari rekam baru.
        out[OFF_WAN_TAG] = RJCFG_WAN_BLOCK_TAG;
        out[OFF_WAN_MODE] = self.wan_mode;
        out[OFF_WAN_IP..OFF_WAN_IP + 4].copy_from_slice(&self.wan_ip);
        out[OFF_WAN_MASK..OFF_WAN_MASK + 4].copy_from_slice(&self.wan_netmask);
        out[OFF_WAN_GW..OFF_WAN_GW + 4].copy_from_slice(&self.wan_gw);
        out[OFF_WAN_NAME_LEN] = self.wan_name_len;
        out[OFF_WAN_NAME..OFF_WAN_NAME + RJCFG_IFACE_NAME_MAX].copy_from_slice(&self.wan_name);
        out[OFF_LAN_NAME_LEN] = self.lan_name_len;
        out[OFF_LAN_NAME..OFF_LAN_NAME + RJCFG_IFACE_NAME_MAX].copy_from_slice(&self.lan_name);
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

        // Blok WAN: hanya ada bila tag tersimpan. Rekam warisan memakai default
        // (DHCP + nama eth0/eth1) sehingga pembacaan tidak pernah gagal.
        let mut wan_mode = RJCFG_WAN_MODE_DHCP;
        let mut wan_ip = [0u8; 4];
        let mut wan_netmask = [0u8; 4];
        let mut wan_gw = [0u8; 4];
        let mut wan_name = [0u8; RJCFG_IFACE_NAME_MAX];
        let mut wan_name_len = 0u8;
        let mut lan_name = [0u8; RJCFG_IFACE_NAME_MAX];
        let mut lan_name_len = 0u8;
        let mut block_present = false;
        if buf[OFF_WAN_TAG] == RJCFG_WAN_BLOCK_TAG {
            block_present = true;
            wan_mode = buf[OFF_WAN_MODE];
            if wan_mode > RJCFG_WAN_MODE_STATIC {
                return Err("Invalid WAN mode");
            }
            wan_ip.copy_from_slice(&buf[OFF_WAN_IP..OFF_WAN_IP + 4]);
            wan_netmask.copy_from_slice(&buf[OFF_WAN_MASK..OFF_WAN_MASK + 4]);
            wan_gw.copy_from_slice(&buf[OFF_WAN_GW..OFF_WAN_GW + 4]);
            wan_name_len = buf[OFF_WAN_NAME_LEN];
            if wan_name_len as usize > RJCFG_IFACE_NAME_MAX {
                return Err("Invalid WAN interface name length");
            }
            wan_name.copy_from_slice(&buf[OFF_WAN_NAME..OFF_WAN_NAME + RJCFG_IFACE_NAME_MAX]);
            lan_name_len = buf[OFF_LAN_NAME_LEN];
            if lan_name_len as usize > RJCFG_IFACE_NAME_MAX {
                return Err("Invalid LAN interface name length");
            }
            lan_name.copy_from_slice(&buf[OFF_LAN_NAME..OFF_LAN_NAME + RJCFG_IFACE_NAME_MAX]);
        }
        if !block_present || wan_name_len == 0 {
            let d = b"eth0";
            wan_name[..d.len()].copy_from_slice(d);
            wan_name_len = d.len() as u8;
        }
        if !block_present || lan_name_len == 0 {
            let d = b"eth1";
            lan_name[..d.len()].copy_from_slice(d);
            lan_name_len = d.len() as u8;
        }

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
            wan_mode,
            wan_ip,
            wan_netmask,
            wan_gw,
            wan_name,
            wan_name_len,
            lan_name,
            lan_name_len,
        })
    }

    /// Tulis alamat WAN statik ke dalam payload (belum di-commit).
    pub fn set_wan_static(&mut self, ip: [u8; 4], netmask: [u8; 4], gw: [u8; 4]) {
        self.wan_mode = RJCFG_WAN_MODE_STATIC;
        self.wan_ip = ip;
        self.wan_netmask = netmask;
        self.wan_gw = gw;
    }

    /// Kembalikan payload ke mode DHCP: alamat yang pernah statik dibersihkan
    /// supaya boot berikutnya tidak pernah mengarang alamat lama.
    pub fn set_wan_dhcp(&mut self) {
        self.wan_mode = RJCFG_WAN_MODE_DHCP;
        self.wan_ip = [0; 4];
        self.wan_netmask = [0; 4];
        self.wan_gw = [0; 4];
    }

    /// Ganti nama antarmuka sisi WAN; validasi ketat karena nama dipakai kembali
    /// sebagai pengenal pada endpoint tulis.
    pub fn set_wan_name(&mut self, name: &str) -> Result<(), &'static str> {
        if !iface_name_ok(name) {
            return Err("Interface name must be 1..=16 of [a-z0-9-_.]");
        }
        for b in self.wan_name.iter_mut() {
            *b = 0;
        }
        let bytes = name.as_bytes();
        self.wan_name[..bytes.len()].copy_from_slice(bytes);
        self.wan_name_len = bytes.len() as u8;
        Ok(())
    }

    pub fn set_lan_name(&mut self, name: &str) -> Result<(), &'static str> {
        if !iface_name_ok(name) {
            return Err("Interface name must be 1..=16 of [a-z0-9-_.]");
        }
        for b in self.lan_name.iter_mut() {
            *b = 0;
        }
        let bytes = name.as_bytes();
        self.lan_name[..bytes.len()].copy_from_slice(bytes);
        self.lan_name_len = bytes.len() as u8;
        Ok(())
    }

    /// Nama WAN sebagai `&str`; buffer dipinjam pemanggil agar tetap no_std
    /// tanpa alokasi. Salinan yang rusak UTF-8 jatuh ke default `eth0`.
    pub fn wan_name_str<'a>(&'a self, out_buf: &'a mut [u8; RJCFG_IFACE_NAME_MAX]) -> &'a str {
        let len = self.wan_name_len as usize;
        if len == 0 || len > RJCFG_IFACE_NAME_MAX {
            return "eth0";
        }
        out_buf[..len].copy_from_slice(&self.wan_name[..len]);
        core::str::from_utf8(&out_buf[..len]).unwrap_or("eth0")
    }

    pub fn lan_name_str<'a>(&'a self, out_buf: &'a mut [u8; RJCFG_IFACE_NAME_MAX]) -> &'a str {
        let len = self.lan_name_len as usize;
        if len == 0 || len > RJCFG_IFACE_NAME_MAX {
            return "eth1";
        }
        out_buf[..len].copy_from_slice(&self.lan_name[..len]);
        core::str::from_utf8(&out_buf[..len]).unwrap_or("eth1")
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

// ============================================================================
// Layout On-Disk RJCFG01 (partisi GPT di atas disk ATA, sektor 512 B)
// - Sektor 0 partisi = blok header journal
// - Slot A = sektor 8..16 (8 sektor x 512 B = 4096 B)
// - Slot B = sektor 16..24 (8 sektor x 512 B = 4096 B)
// Serialisasi header on-disk eksplisit little-endian; TIDAK ada cast pointer
// mentah pada kode baru (jalur lama new_default/commit_update tetap dipertahankan).
// ============================================================================

/// Ukuran satu sektor ATA pada layout RJCFG01.
pub const RJCFG_SECTOR_SIZE: usize = 512;
/// Nomor sektor blok header journal (sektor 0 partisi).
pub const RJCFG_HEADER_SECTOR: u64 = 0;
/// Jumlah sektor yang dihuni satu slot konfigurasi (8 x 512 B = 4096 B).
pub const RJCFG_SLOT_SECTORS: usize = 8;
/// Sektor pertama Slot A pada partisi RJCFG01.
pub const RJCFG_SLOT_A_FIRST_SECTOR: u64 = 8;
/// Sektor pertama Slot B pada partisi RJCFG01.
pub const RJCFG_SLOT_B_FIRST_SECTOR: u64 = 16;
/// Versi header on-disk yang dikenal (byte versi tunggal di offset 8).
pub const RJCFG_ONDISK_VERSION: u8 = 1;

/// Peta index slot (0 = A, 1 = B) ke nomor sektor pertama partisi RJCFG01.
pub fn slot_first_sector(slot: u8) -> Option<u64> {
    match slot {
        0 => Some(RJCFG_SLOT_A_FIRST_SECTOR),
        1 => Some(RJCFG_SLOT_B_FIRST_SECTOR),
        _ => None,
    }
}

impl RjcfgJournal {
    /// Serialisasi header journal ke satu sektor 512 B, layout little-endian:
    /// [0..8]=magic, [8]=version u8, [9]=active_slot u8, [10..14]=nol,
    /// [14..16]=nol, [16..20]=sequence_number u32 LE, [20..24]=slot_a_crc32 LE,
    /// [24..28]=slot_b_crc32 LE, [28..32]=header_crc32 LE, [32..512]=nol.
    /// header_crc32 = compute_crc32(byte 0..28) — field CRC itu sendiri dikecualikan.
    ///
    /// Catatan pemetaan: `RjcfgHeader.version` bertipe u32 sedangkan layout
    /// on-disk menyimpan versi sebagai satu byte; saat tulis hanya byte rendah
    /// yang disimpan, dan saat baca versi direkonstruksi sebagai u32 dari byte
    /// tersebut. `reserved: [u8;3]` tidak punya slot on-disk: ditulis nol dan
    /// direkonstruksi sebagai default [0;3] saat baca.
    pub fn serialize_header_to_sector(&self, out: &mut [u8; RJCFG_SECTOR_SIZE]) {
        for b in out.iter_mut() {
            *b = 0;
        }
        out[0..8].copy_from_slice(&self.header.magic);
        out[8] = self.header.version as u8;
        out[9] = self.header.active_slot;
        // Byte 10..14 dan 14..16 tetap nol (padding/reserved on-disk).
        out[16..20].copy_from_slice(&self.header.sequence_number.to_le_bytes());
        out[20..24].copy_from_slice(&self.header.slot_a_crc32.to_le_bytes());
        out[24..28].copy_from_slice(&self.header.slot_b_crc32.to_le_bytes());
        // header_crc32 dihitung atas 28 byte pertama (field CRC dikecualikan).
        let header_crc = compute_crc32(&out[0..28]);
        out[28..32].copy_from_slice(&header_crc.to_le_bytes());
    }

    /// Deskripsi header 512 B on-disk menjadi RjcfgHeader dengan validasi
    /// bertingkat: magic, versi dikenal, lalu CRC header.
    pub fn deserialize_header_from_sector(
        sec: &[u8; RJCFG_SECTOR_SIZE],
    ) -> Result<RjcfgHeader, &'static str> {
        if sec[0..8] != RJCFG_MAGIC {
            return Err("RJCFG01: magic tidak valid");
        }
        let version = sec[8];
        if version != RJCFG_ONDISK_VERSION {
            return Err("RJCFG01: versi tidak dikenal");
        }
        let stored_header_crc = u32::from_le_bytes([sec[28], sec[29], sec[30], sec[31]]);
        if compute_crc32(&sec[0..28]) != stored_header_crc {
            return Err("RJCFG01: header CRC salah");
        }
        Ok(RjcfgHeader {
            magic: RJCFG_MAGIC,
            // Versi on-disk u8 direkonstruksi menjadi u32; reserved dipulihkan
            // ke default [0;3] (tidak diwakili di layout on-disk).
            version: version as u32,
            active_slot: sec[9],
            reserved: [0u8; 3],
            sequence_number: u32::from_le_bytes([sec[16], sec[17], sec[18], sec[19]]),
            slot_a_crc32: u32::from_le_bytes([sec[20], sec[21], sec[22], sec[23]]),
            slot_b_crc32: u32::from_le_bytes([sec[24], sec[25], sec[26], sec[27]]),
            header_crc32: stored_header_crc,
        })
    }

    /// Bangun ulang journal dari disk: `slot_bytes` adalah 4096 B milik slot
    /// AKTIF hasil baca partisi. CRC slot aktif diverifikasi terhadap header,
    /// payload dideserialisasi, dan slot non-aktif diisi nol.
    pub fn load_from_disk(
        header: &RjcfgHeader,
        slot_bytes: &[u8; RJCFG_SLOT_SIZE],
    ) -> Result<Self, &'static str> {
        if header.active_slot > 1 {
            return Err("RJCFG01: active_slot tidak valid");
        }
        let expected_slot_crc = if header.active_slot == 0 {
            header.slot_a_crc32
        } else {
            header.slot_b_crc32
        };
        if compute_crc32(slot_bytes) != expected_slot_crc {
            return Err("RJCFG01: slot CRC salah");
        }
        let active_config = RjcfgConfigPayload::deserialize(slot_bytes)?;
        let (slot_a, slot_b) = if header.active_slot == 0 {
            (*slot_bytes, [0u8; RJCFG_SLOT_SIZE])
        } else {
            ([0u8; RJCFG_SLOT_SIZE], *slot_bytes)
        };
        Ok(Self {
            header: *header,
            slot_a,
            slot_b,
            active_config,
        })
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

        let target_slot = journal
            .set_hostname("rinjani-router")
            .expect("hostname set ok");
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
        assert_eq!(
            journal.set_hostname(""),
            Err("Hostname must be 1..32 characters")
        );
        assert_eq!(
            journal.set_hostname("this-hostname-is-definitely-far-too-long-for-rinjani-32-bytes"),
            Err("Hostname must be 1..32 characters")
        );
    }

    #[test]
    fn test_06_crc_recomputation_integrity() {
        let mut journal = RjcfgJournal::new_default();
        let crc_a = journal.header.slot_a_crc32;
        journal.set_hostname("new-name").unwrap();
        let crc_b = journal.header.slot_b_crc32;
        assert_ne!(crc_a, crc_b);
    }

    #[test]
    fn test_07_header_sector_roundtrip() {
        let journal = RjcfgJournal::new_default();
        let mut sector = [0u8; RJCFG_SECTOR_SIZE];
        journal.serialize_header_to_sector(&mut sector);

        let parsed =
            RjcfgJournal::deserialize_header_from_sector(&sector).expect("header sector valid");
        // Perbandingan struct utuh (hindari referensi ke field packed struct).
        assert_eq!(parsed, journal.header);
        // Byte di luar wilayah header harus nol.
        assert!(sector[32..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_08_header_magic_corruption() {
        let journal = RjcfgJournal::new_default();
        let mut sector = [0u8; RJCFG_SECTOR_SIZE];
        journal.serialize_header_to_sector(&mut sector);

        sector[2] ^= 0xFF; // flip satu byte di wilayah magic
        assert_eq!(
            RjcfgJournal::deserialize_header_from_sector(&sector),
            Err("RJCFG01: magic tidak valid")
        );
    }

    #[test]
    fn test_09_header_version_corruption() {
        let journal = RjcfgJournal::new_default();
        let mut sector = [0u8; RJCFG_SECTOR_SIZE];
        journal.serialize_header_to_sector(&mut sector);

        sector[8] = 9; // versi tidak dikenal, dicek sebelum CRC
        assert_eq!(
            RjcfgJournal::deserialize_header_from_sector(&sector),
            Err("RJCFG01: versi tidak dikenal")
        );
    }

    #[test]
    fn test_10_header_crc_corruption() {
        let journal = RjcfgJournal::new_default();
        let mut sector = [0u8; RJCFG_SECTOR_SIZE];
        journal.serialize_header_to_sector(&mut sector);

        sector[16] ^= 0x01; // byte tercakup CRC (sequence_number), magic/version utuh
        assert_eq!(
            RjcfgJournal::deserialize_header_from_sector(&sector),
            Err("RJCFG01: header CRC salah")
        );
    }

    #[test]
    fn test_11_slot_first_sector_mapping() {
        assert_eq!(slot_first_sector(0), Some(8));
        assert_eq!(slot_first_sector(1), Some(16));
        assert_eq!(slot_first_sector(2), None);
        assert_eq!(slot_first_sector(255), None);
        assert_eq!(RJCFG_SLOT_SECTORS * RJCFG_SECTOR_SIZE, RJCFG_SLOT_SIZE);
    }

    #[test]
    fn test_12_load_from_disk_happy_path() {
        let journal = RjcfgJournal::new_default();
        // new_default sudah menyimpan CRC slot_a di header dan active_slot = 0.
        let loaded = RjcfgJournal::load_from_disk(&journal.header, &journal.slot_a)
            .expect("load from disk ok");
        assert_eq!(loaded.header, journal.header);
        assert_eq!(loaded.slot_a, journal.slot_a);
        assert_eq!(loaded.slot_b, [0u8; RJCFG_SLOT_SIZE]);
        assert_eq!(loaded.active_config, journal.active_config);
    }

    #[test]
    fn test_13_load_from_disk_slot_crc_mismatch() {
        let journal = RjcfgJournal::new_default();
        let mut bad_slot = journal.slot_a;
        bad_slot[5] ^= 0xAA; // ubah payload sehingga CRC slot tidak cocok header
        let err = RjcfgJournal::load_from_disk(&journal.header, &bad_slot)
            .err()
            .expect("slot CRC harus menolak");
        assert_eq!(err, "RJCFG01: slot CRC salah");
    }

    #[test]
    fn test_14_load_from_disk_invalid_active_slot() {
        let journal = RjcfgJournal::new_default();
        let mut bad_header = journal.header;
        bad_header.active_slot = 2;
        let err = RjcfgJournal::load_from_disk(&bad_header, &journal.slot_a)
            .err()
            .expect("active_slot tidak valid harus menolak");
        assert_eq!(err, "RJCFG01: active_slot tidak valid");
    }

    #[test]
    fn test_15_wan_static_roundtrip() {
        let mut cfg = RjcfgConfigPayload::default_config();
        cfg.set_wan_static([198, 51, 100, 20], [255, 255, 255, 0], [198, 51, 100, 1]);
        cfg.set_wan_name("wan-uplink").unwrap();

        let mut slot = [0u8; RJCFG_SLOT_SIZE];
        cfg.serialize(&mut slot);
        assert_eq!(slot[OFF_WAN_TAG], RJCFG_WAN_BLOCK_TAG);

        let back = RjcfgConfigPayload::deserialize(&slot).expect("blok WAN terbaca");
        assert_eq!(back.wan_mode, RJCFG_WAN_MODE_STATIC);
        assert_eq!(back.wan_ip, [198, 51, 100, 20]);
        assert_eq!(back.wan_netmask, [255, 255, 255, 0]);
        assert_eq!(back.wan_gw, [198, 51, 100, 1]);
        let mut nb = [0u8; RJCFG_IFACE_NAME_MAX];
        assert_eq!(back.wan_name_str(&mut nb), "wan-uplink");
        assert_eq!(back, cfg);
    }

    #[test]
    fn test_16_legacy_slot_without_wan_block_keeps_defaults() {
        // Rekam pra-UI-02: byte 119..167 nol karena serialize lama menolkan slot.
        let mut slot = [0u8; RJCFG_SLOT_SIZE];
        RjcfgConfigPayload::default_config().serialize(&mut slot);
        for b in slot[OFF_WAN_TAG..OFF_WAN_END].iter_mut() {
            *b = 0;
        }
        let back = RjcfgConfigPayload::deserialize(&slot).expect("rekam warisan wajib diterima");
        let mut nb = [0u8; RJCFG_IFACE_NAME_MAX];
        assert_eq!(back.hostname_len, 7);
        assert_eq!(&back.hostname[..7], b"rinjani");
        assert_eq!(back.wan_mode, RJCFG_WAN_MODE_DHCP);
        assert_eq!(back.wan_ip, [0, 0, 0, 0]);
        assert_eq!(back.wan_name_str(&mut nb), "eth0");
        assert_eq!(back.lan_name_str(&mut nb), "eth1");
    }

    #[test]
    fn test_17_invalid_wan_mode_rejected() {
        let mut slot = [0u8; RJCFG_SLOT_SIZE];
        RjcfgConfigPayload::default_config().serialize(&mut slot);
        slot[OFF_WAN_MODE] = 2; // bukan dhcp(0) dan bukan static(1)
        assert_eq!(
            RjcfgConfigPayload::deserialize(&slot),
            Err("Invalid WAN mode")
        );
    }

    #[test]
    fn test_18_iface_name_validation() {
        assert!(iface_name_ok("eth0"));
        assert!(iface_name_ok("wan-uplink.2"));
        assert!(!iface_name_ok(""));
        assert!(!iface_name_ok("ETH0"));
        assert!(!iface_name_ok("eth 0"));
        assert!(!iface_name_ok("a;rm -rf"));
        assert!(!iface_name_ok("enam belas karakter percobaan"));
        let mut cfg = RjcfgConfigPayload::default_config();
        assert!(cfg.set_wan_name("eth;drop").is_err());
        // Nama yang ditolak tidak boleh meninggalkan sisa di payload.
        let mut nb = [0u8; RJCFG_IFACE_NAME_MAX];
        assert_eq!(cfg.wan_name_str(&mut nb), "eth0");
    }

    #[test]
    fn test_19_wan_config_survives_commit_and_reload() {
        let mut journal = RjcfgJournal::new_default();
        let mut cfg = journal.active_config;
        cfg.set_wan_static([203, 0, 113, 9], [255, 255, 255, 128], [203, 0, 113, 1]);
        cfg.set_lan_name("lan-home").unwrap();
        let slot = journal.commit_update(cfg).expect("commit ok");
        assert_eq!(slot, 1);

        let loaded = RjcfgJournal::load_from_disk(&journal.header, &journal.slot_b)
            .expect("muat ulang dari slot aktif");
        assert_eq!(loaded.active_config.wan_mode, RJCFG_WAN_MODE_STATIC);
        assert_eq!(loaded.active_config.wan_ip, [203, 0, 113, 9]);
        let mut nb = [0u8; RJCFG_IFACE_NAME_MAX];
        assert_eq!(loaded.active_config.lan_name_str(&mut nb), "lan-home");
    }

    #[test]
    fn test_20_dhcp_mode_clears_static_addresses() {
        let mut cfg = RjcfgConfigPayload::default_config();
        cfg.set_wan_static([198, 51, 100, 20], [255, 255, 255, 0], [198, 51, 100, 1]);
        cfg.set_wan_dhcp();
        assert_eq!(cfg.wan_mode, RJCFG_WAN_MODE_DHCP);
        assert_eq!(cfg.wan_ip, [0, 0, 0, 0]);
        assert_eq!(cfg.wan_gw, [0, 0, 0, 0]);
    }
}
