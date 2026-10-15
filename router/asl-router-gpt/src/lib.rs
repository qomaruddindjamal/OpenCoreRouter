//! Open Core Router (OpenCR) — asl-router-gpt
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! GUID Partition Table (GPT) & Protective MBR Parser / Formatter
//! Strict compliance: RFC 1071 / IEEE 802.3 CRC32, Rinjani 3-partition layout:
//!   - Partition 1: EFI System Partition (ESP) — 128 MiB (FAT32/Limine boot)
//!   - Partition 2: RJCFG01 Configuration Journal — 8 MiB (A/B persistent slot)
//!   - Partition 3: RJDATA Elastic Data Store — Remainder of disk (elastic growth)

#![no_std]

pub const SECTOR_SIZE: usize = 512;
pub const GPT_HEADER_SIGNATURE: [u8; 8] = *b"EFI PART";
pub const GPT_HEADER_REVISION_1_0: u32 = 0x00010000;
pub const GPT_HEADER_SIZE: u32 = 92;
pub const GPT_PARTITION_ENTRY_SIZE: u32 = 128;
pub const GPT_MAX_PARTITIONS: usize = 128;

// Well-known partition type GUIDs
// ESP: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
pub const GUID_ESP: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
    0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

// RJCFG01 (Rinjani Config Journal): 524A4346-4730-3100-8000-00A0C93EC93B ("RJCFG01")
pub const GUID_RINJANI_CFG: [u8; 16] = [
    0x46, 0x43, 0x4A, 0x52, 0x30, 0x47, 0x00, 0x31,
    0x80, 0x00, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

// RJDATA (Rinjani Elastic Data): 524A4441-5441-0000-8000-00A0C93EC93B ("RJDATA")
pub const GUID_RINJANI_DATA: [u8; 16] = [
    0x41, 0x44, 0x4A, 0x52, 0x41, 0x54, 0x00, 0x00,
    0x80, 0x00, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

// -----------------------------------------------------------------------------
// CRC32 UTILITY (IEEE 802.3 standard polynomial 0xEDB88320)
// -----------------------------------------------------------------------------

/// Deterministic unique partition GUID: 524A4E0<n>-0001-4E01-8000-524A4E000001
/// ("RJN" flavored; images are reproducible byte-for-byte, no randomness).
pub const fn make_unique_guid(partition_no: u8) -> [u8; 16] {
    [
        0x52, 0x4A, 0x4E, 0x30 + partition_no,
        0x00, 0x01,
        0x4E, 0x01,
        0x80, 0x00,
        0x52, 0x4A, 0x4E, 0x00, 0x00, 0x01,
    ]
}

pub fn compute_crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data)
}

/// CRC32 inkremental: `prev` = hasil (ternegasi) chunk sebelumnya; untuk chunk
/// pertama pakai 0xFFFF_FFFF. Hasil akhir sudah ternegasi — untuk meneruskan,
/// un-negasi dulu: crc32_update(!prev_state, next). Dipakai installer utk
/// memverifikasi CRC array entri GPT (16 KiB) tanpa buffer penuh di stack.
pub fn crc32_update(state: u32, data: &[u8]) -> u32 {
    let mut crc = state;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = !((crc & 1).wrapping_sub(1));
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// -----------------------------------------------------------------------------
// GPT HEADER STRUCTURE (LBA 1 / Backup LBA)
// -----------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GptHeader {
    pub signature: [u8; 8],
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub reserved: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub size_of_partition_entry: u32,
    pub partition_array_crc32: u32,
}

// -----------------------------------------------------------------------------
// GPT PARTITION ENTRY STRUCTURE (128 bytes)
// -----------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GptPartitionEntry {
    pub partition_type_guid: [u8; 16],
    pub unique_partition_guid: [u8; 16],
    pub starting_lba: u64,
    pub ending_lba: u64,
    pub attributes: u64,
    pub partition_name: [u16; 36], // UTF-16LE 36 chars
}

impl GptPartitionEntry {
    pub const fn empty() -> Self {
        Self {
            partition_type_guid: [0u8; 16],
            unique_partition_guid: [0u8; 16],
            starting_lba: 0,
            ending_lba: 0,
            attributes: 0,
            partition_name: [0u16; 36],
        }
    }

    pub fn is_used(&self) -> bool {
        self.starting_lba != 0 && self.ending_lba >= self.starting_lba
    }

    pub fn sector_count(&self) -> u64 {
        if self.is_used() {
            (self.ending_lba - self.starting_lba) + 1
        } else {
            0
        }
    }

    pub fn size_bytes(&self) -> u64 {
        self.sector_count() * (SECTOR_SIZE as u64)
    }

    pub fn set_name(&mut self, name: &str) {
        let mut idx = 0;
        for c in name.chars() {
            if idx < 35 {
                self.partition_name[idx] = c as u16;
                idx += 1;
            }
        }
        while idx < 36 {
            self.partition_name[idx] = 0;
            idx += 1;
        }
    }
}

// -----------------------------------------------------------------------------
// GPT DISK LAYOUT MANAGER
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptDiskLayout {
    pub total_sectors: u64,
    pub primary_header: GptHeader,
    pub backup_header: GptHeader,
    pub partitions: [GptPartitionEntry; 4], // 1: ESP, 2: RJCFG01, 3: RJDATA, 4: free
}

impl GptDiskLayout {
    /// Formats a clean Rinjani 3-partition GPT layout given total disk capacity in sectors
    pub fn create_default_layout(total_sectors: u64) -> Result<Self, &'static str> {
        if total_sectors < 1024 * 1024 { // Minimum ~512 MiB
            return Err("Disk too small for Rinjani GPT layout");
        }

        let first_usable = 34u64;
        let last_usable = total_sectors - 34;

        // Partition 1: ESP 128 MiB (128 * 2048 = 262,144 sectors)
        let esp_sectors = 262_144u64;
        let mut esp = GptPartitionEntry::empty();
        esp.partition_type_guid = GUID_ESP;
        esp.unique_partition_guid = make_unique_guid(1);
        esp.starting_lba = first_usable;
        esp.ending_lba = esp.starting_lba + esp_sectors - 1;
        esp.set_name("EFI System Partition");

        // Partition 2: RJCFG01 8 MiB (8 * 2048 = 16,384 sectors)
        let cfg_sectors = 16_384u64;
        let mut cfg = GptPartitionEntry::empty();
        cfg.partition_type_guid = GUID_RINJANI_CFG;
        cfg.unique_partition_guid = make_unique_guid(2);
        cfg.starting_lba = esp.ending_lba + 1;
        cfg.ending_lba = cfg.starting_lba + cfg_sectors - 1;
        cfg.set_name("RJCFG01");

        // Partition 3: RJDATA elastic remainder up to last_usable
        let mut data = GptPartitionEntry::empty();
        data.partition_type_guid = GUID_RINJANI_DATA;
        data.unique_partition_guid = make_unique_guid(3);
        data.starting_lba = cfg.ending_lba + 1;
        data.ending_lba = last_usable;
        data.set_name("RJDATA");

        let partitions = [esp, cfg, data, GptPartitionEntry::empty()];

        // Compute partition array CRC32
        let raw_parts = unsafe {
            core::slice::from_raw_parts(
                partitions.as_ptr() as *const u8,
                core::mem::size_of_val(&partitions),
            )
        };
        let parts_crc = compute_crc32(raw_parts);

        let primary_header = GptHeader {
            signature: GPT_HEADER_SIGNATURE,
            revision: GPT_HEADER_REVISION_1_0,
            header_size: GPT_HEADER_SIZE,
            header_crc32: 0,
            reserved: 0,
            current_lba: 1,
            backup_lba: total_sectors - 1,
            first_usable_lba: first_usable,
            last_usable_lba: last_usable,
            disk_guid: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00],
            partition_entry_lba: 2,
            num_partition_entries: 4,
            size_of_partition_entry: GPT_PARTITION_ENTRY_SIZE,
            partition_array_crc32: parts_crc,
        };

        let backup_header = GptHeader {
            signature: GPT_HEADER_SIGNATURE,
            revision: GPT_HEADER_REVISION_1_0,
            header_size: GPT_HEADER_SIZE,
            header_crc32: 0,
            reserved: 0,
            current_lba: total_sectors - 1,
            backup_lba: 1,
            first_usable_lba: first_usable,
            last_usable_lba: last_usable,
            disk_guid: primary_header.disk_guid,
            partition_entry_lba: total_sectors - 33,
            num_partition_entries: 4,
            size_of_partition_entry: GPT_PARTITION_ENTRY_SIZE,
            partition_array_crc32: parts_crc,
        };

        Ok(Self {
            total_sectors,
            primary_header,
            backup_header,
            partitions,
        })
    }

    /// Resize partition 3 (RJDATA) to adopt real expanded physical capacity
    pub fn resize_data_partition(&mut self, new_total_sectors: u64) -> Result<u64, &'static str> {
        if new_total_sectors <= self.total_sectors {
            return Err("New capacity is smaller than or equal to current capacity");
        }
        let new_last_usable = new_total_sectors - 34;
        self.total_sectors = new_total_sectors;
        self.primary_header.backup_lba = new_total_sectors - 1;
        self.primary_header.last_usable_lba = new_last_usable;

        self.backup_header.current_lba = new_total_sectors - 1;
        self.backup_header.last_usable_lba = new_last_usable;
        self.backup_header.partition_entry_lba = new_total_sectors - 33;

        // Resize partition 3
        self.partitions[2].ending_lba = new_last_usable;

        // Recompute partition CRC
        let raw_parts = unsafe {
            core::slice::from_raw_parts(
                self.partitions.as_ptr() as *const u8,
                core::mem::size_of_val(&self.partitions),
            )
        };
        let new_parts_crc = compute_crc32(raw_parts);
        self.primary_header.partition_array_crc32 = new_parts_crc;
        self.backup_header.partition_array_crc32 = new_parts_crc;

        Ok(self.partitions[2].sector_count())
    }
}

// -----------------------------------------------------------------------------
// GPT SERIALIZER (LBA0 protective MBR, LBA1 header, LBA2..33 entries, backup)
// -----------------------------------------------------------------------------

impl GptPartitionEntry {
    pub fn to_bytes(&self) -> [u8; GPT_PARTITION_ENTRY_SIZE as usize] {
        let mut out = [0u8; GPT_PARTITION_ENTRY_SIZE as usize];
        out[0..16].copy_from_slice(&self.partition_type_guid);
        out[16..32].copy_from_slice(&self.unique_partition_guid);
        out[32..40].copy_from_slice(&self.starting_lba.to_le_bytes());
        out[40..48].copy_from_slice(&self.ending_lba.to_le_bytes());
        out[48..56].copy_from_slice(&self.attributes.to_le_bytes());
        for (i, u) in self.partition_name.iter().enumerate() {
            out[56 + i * 2..58 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        out
    }
}

impl GptHeader {
    /// Serializes the 92-byte header, self-computing header_crc32 over the
    /// header with that field zeroed (UEFI 2.8 / GPT spec requirement).
    pub fn to_bytes(&mut self) -> [u8; GPT_HEADER_SIZE as usize] {
        self.header_crc32 = 0;
        let mut buf = [0u8; GPT_HEADER_SIZE as usize];
        buf[0..8].copy_from_slice(&self.signature);
        buf[8..12].copy_from_slice(&self.revision.to_le_bytes());
        buf[12..16].copy_from_slice(&self.header_size.to_le_bytes());
        buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // crc field placeholder
        buf[20..24].copy_from_slice(&self.reserved.to_le_bytes());
        buf[24..32].copy_from_slice(&self.current_lba.to_le_bytes());
        buf[32..40].copy_from_slice(&self.backup_lba.to_le_bytes());
        buf[40..48].copy_from_slice(&self.first_usable_lba.to_le_bytes());
        buf[48..56].copy_from_slice(&self.last_usable_lba.to_le_bytes());
        buf[56..72].copy_from_slice(&self.disk_guid);
        buf[72..80].copy_from_slice(&self.partition_entry_lba.to_le_bytes());
        buf[80..84].copy_from_slice(&self.num_partition_entries.to_le_bytes());
        buf[84..88].copy_from_slice(&self.size_of_partition_entry.to_le_bytes());
        buf[88..92].copy_from_slice(&self.partition_array_crc32.to_le_bytes());
        let crc = compute_crc32(&buf);
        self.header_crc32 = crc;
        buf[16..20].copy_from_slice(&crc.to_le_bytes());
        buf
    }
}

pub const GPT_ENTRY_ARRAY_SECTORS: usize = 32; // 128 entries x 128 bytes

impl GptDiskLayout {
    /// Builds the canonical 128-entry partition array bytes (entries beyond
    /// the declared partitions are zero-filled) and refreshes both header CRCs.
    pub fn entry_array_bytes(&mut self) -> [u8; GPT_MAX_PARTITIONS * GPT_PARTITION_ENTRY_SIZE as usize] {
        let mut arr = [0u8; GPT_MAX_PARTITIONS * GPT_PARTITION_ENTRY_SIZE as usize];
        for (i, p) in self.partitions.iter().enumerate() {
            let bytes = p.to_bytes();
            arr[i * GPT_PARTITION_ENTRY_SIZE as usize..(i + 1) * GPT_PARTITION_ENTRY_SIZE as usize]
                .copy_from_slice(&bytes);
        }
        let arr_crc = compute_crc32(&arr);
        self.primary_header.partition_array_crc32 = arr_crc;
        self.backup_header.partition_array_crc32 = arr_crc;
        self.primary_header.num_partition_entries = GPT_MAX_PARTITIONS as u32;
        self.backup_header.num_partition_entries = GPT_MAX_PARTITIONS as u32;
        arr
    }

    /// Writes the disk head: sector 0 = protective MBR, sector 1 = primary
    /// header, sectors 2..34 = full 128-entry partition array.
    /// Requires out.len() >= 34 * SECTOR_SIZE.
    pub fn write_head(&mut self, out: &mut [u8]) -> Result<(), &'static str> {
        if out.len() < 34 * SECTOR_SIZE {
            return Err("write_head buffer smaller than 34 sectors");
        }
        let mut mbr = [0u8; SECTOR_SIZE];
        build_protective_mbr(self.total_sectors, &mut mbr);
        out[0..SECTOR_SIZE].copy_from_slice(&mbr);

        let arr = self.entry_array_bytes();
        let mut hdr = self.primary_header;
        let hdr_bytes = hdr.to_bytes();
        out[SECTOR_SIZE..SECTOR_SIZE + GPT_HEADER_SIZE as usize].copy_from_slice(&hdr_bytes);
        out[2 * SECTOR_SIZE..2 * SECTOR_SIZE + arr.len()].copy_from_slice(&arr);
        Ok(())
    }

    /// Writes the disk tail: 32 sectors of backup entry array then the backup
    /// header as the final sector. Requires out.len() == last 33 sectors.
    pub fn write_backup(&mut self, out: &mut [u8]) -> Result<(), &'static str> {
        if out.len() != 33 * SECTOR_SIZE {
            return Err("write_backup buffer must be exactly 33 sectors");
        }
        let arr = self.entry_array_bytes();
        out[0..arr.len()].copy_from_slice(&arr);
        let mut hdr = self.backup_header;
        let hdr_bytes = hdr.to_bytes();
        out[32 * SECTOR_SIZE..32 * SECTOR_SIZE + GPT_HEADER_SIZE as usize].copy_from_slice(&hdr_bytes);
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// PROTECTIVE MBR GENERATOR (LBA 0)
// -----------------------------------------------------------------------------

pub fn build_protective_mbr(total_sectors: u64, out_sector: &mut [u8; SECTOR_SIZE]) {
    for b in out_sector.iter_mut() {
        *b = 0;
    }
    // Partition entry 1 at offset 446 (0x1BE)
    let p1_start = 446;
    out_sector[p1_start] = 0x00;     // Non-bootable
    out_sector[p1_start + 1] = 0x00; // Starting CHS (0/2/0)
    out_sector[p1_start + 2] = 0x02;
    out_sector[p1_start + 3] = 0x00;
    out_sector[p1_start + 4] = 0xEE; // OS Type: GPT Protective MBR (0xEE)
    out_sector[p1_start + 5] = 0xFF; // Ending CHS
    out_sector[p1_start + 6] = 0xFF;
    out_sector[p1_start + 7] = 0xFF;
    out_sector[p1_start + 8..p1_start + 12].copy_from_slice(&1u32.to_le_bytes()); // Starting LBA = 1

    let size_sectors = if total_sectors > 0xFFFF_FFFF {
        0xFFFF_FFFFu32
    } else {
        (total_sectors.saturating_sub(1)) as u32
    };
    out_sector[p1_start + 12..p1_start + 16].copy_from_slice(&size_sectors.to_le_bytes());

    // Boot signature
    out_sector[510] = 0x55;
    out_sector[511] = 0xAA;
}

// -----------------------------------------------------------------------------
// PARSER READ-ONLY: GPT header & partition entry dari byte disk nyata
// (untuk verifikasi round-trip driver block, mis. baca LBA1 via PIO ATA).
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GptHeaderInfo {
    pub entry_lba: u64,
    pub num_entries: u32,
    pub entry_size: u32,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub header_crc_ok: bool,
}

/// Parse header GPT dari satu sektor (512 B). None bila signature salah.
/// header_crc32 dihitung atas header_size (92) byte pertama dgn field crc dinolkan.
pub fn parse_gpt_header_lba1(sec: &[u8]) -> Option<GptHeaderInfo> {
    if sec.len() < 92 || &sec[0..8] != b"EFI PART" {
        return None;
    }
    let header_size = u32::from_le_bytes(sec[8..12].try_into().unwrap()) as usize;
    let stored_crc = u32::from_le_bytes(sec[16..20].try_into().unwrap());
    let first_usable_lba = u64::from_le_bytes(sec[40..48].try_into().unwrap());
    let last_usable_lba = u64::from_le_bytes(sec[48..56].try_into().unwrap());
    let entry_lba = u64::from_le_bytes(sec[72..80].try_into().unwrap());
    let num_entries = u32::from_le_bytes(sec[80..84].try_into().unwrap());
    let entry_size = u32::from_le_bytes(sec[84..88].try_into().unwrap());

    let hs = if header_size >= 92 && header_size <= sec.len() { header_size } else { 92 };
    let mut hdr_copy = [0u8; 92];
    hdr_copy[..hs.min(92)].copy_from_slice(&sec[..hs.min(92)]);
    hdr_copy[16..20].fill(0);
    let calc = compute_crc32(&hdr_copy[..hs.min(92)]);
    Some(GptHeaderInfo {
        entry_lba,
        num_entries,
        entry_size,
        first_usable_lba,
        last_usable_lba,
        header_crc_ok: calc == stored_crc,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GptEntryInfo {
    pub start_lba: u64,
    pub end_lba: u64,
    /// Nama ASCII hasil konversi UTF-16LE (di-trim, max 36).
    pub name: [u8; 36],
    pub name_len: usize,
    pub used: bool,
    pub type_guid: [u8; 16],
}

/// Parse 1 entri partisi (128 B) dari buffer array entri.
pub fn parse_gpt_entry(bytes: &[u8]) -> Option<GptEntryInfo> {
    if bytes.len() < 128 {
        return None;
    }
    let type_guid: [u8; 16] = bytes[0..16].try_into().unwrap();
    let used = type_guid != [0u8; 16];
    let start_lba = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
    let end_lba = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
    let mut name = [0u8; 36];
    let mut nlen = 0usize;
    if used {
        for i in 0..36 {
            let lo = bytes[56 + i * 2];
            let hi = bytes[56 + i * 2 + 1];
            let ch = (hi as u16) << 8 | lo as u16;
            if ch == 0 {
                break;
            }
            name[nlen] = if ch >= 0x20 && ch < 0x7F { ch as u8 } else { b'?' };
            nlen += 1;
        }
    }
    Some(GptEntryInfo { start_lba, end_lba, name, name_len: nlen, used, type_guid })
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    #[test]
    fn test_01_crc32_standard_vector() {
        let test_data = b"123456789";
        let crc = compute_crc32(test_data);
        assert_eq!(crc, 0xCBF43926);
    }

    #[test]
    fn test_02_protective_mbr_structure() {
        let mut mbr = [0u8; SECTOR_SIZE];
        build_protective_mbr(100_000, &mut mbr);
        assert_eq!(mbr[510], 0x55);
        assert_eq!(mbr[511], 0xAA);
        assert_eq!(mbr[446 + 4], 0xEE); // GPT Protective
        let start_lba = u32::from_le_bytes([mbr[446 + 8], mbr[446 + 9], mbr[446 + 10], mbr[446 + 11]]);
        assert_eq!(start_lba, 1);
    }

    #[test]
    fn test_03_default_layout_partitions() {
        let total_sectors = 41_943_040u64; // 20 GiB
        let layout = GptDiskLayout::create_default_layout(total_sectors).expect("layout ok");
        assert_eq!(layout.partitions[0].partition_type_guid, GUID_ESP);
        assert_eq!(layout.partitions[0].sector_count(), 262_144); // 128 MiB
        assert_eq!(layout.partitions[1].partition_type_guid, GUID_RINJANI_CFG);
        assert_eq!(layout.partitions[1].sector_count(), 16_384); // 8 MiB
        assert_eq!(layout.partitions[2].partition_type_guid, GUID_RINJANI_DATA);
        assert!(layout.partitions[2].sector_count() > 40_000_000);
    }

    #[test]
    fn test_04_disk_too_small_error() {
        assert_eq!(
            GptDiskLayout::create_default_layout(500_000),
            Err("Disk too small for Rinjani GPT layout")
        );
    }

    #[test]
    fn test_05_resize_data_partition_growth() {
        let initial_sectors = 41_943_040u64; // 20 GiB
        let mut layout = GptDiskLayout::create_default_layout(initial_sectors).unwrap();
        let initial_p3_size = layout.partitions[2].sector_count();

        let expanded_sectors = 83_886_080u64; // 40 GiB
        let new_p3_size = layout.resize_data_partition(expanded_sectors).expect("resize ok");
        assert!(new_p3_size > initial_p3_size);
        assert_eq!(layout.total_sectors, expanded_sectors);
        let backup_lba = layout.primary_header.backup_lba;
        assert_eq!(backup_lba, expanded_sectors - 1);
    }

    #[test]
    fn test_06_resize_shrink_returns_err() {
        let initial_sectors = 41_943_040u64;
        let mut layout = GptDiskLayout::create_default_layout(initial_sectors).unwrap();
        assert_eq!(
            layout.resize_data_partition(initial_sectors - 1000),
            Err("New capacity is smaller than or equal to current capacity")
        );
    }

    #[test]
    fn test_07_partition_name_setter() {
        let mut entry = GptPartitionEntry::empty();
        entry.set_name("RJCFG01");
        let p0 = entry.partition_name[0];
        let p1 = entry.partition_name[1];
        let p2 = entry.partition_name[2];
        let p3 = entry.partition_name[3];
        let p4 = entry.partition_name[4];
        let p5 = entry.partition_name[5];
        let p6 = entry.partition_name[6];
        let p7 = entry.partition_name[7];
        assert_eq!(p0, 'R' as u16);
        assert_eq!(p1, 'J' as u16);
        assert_eq!(p2, 'C' as u16);
        assert_eq!(p3, 'F' as u16);
        assert_eq!(p4, 'G' as u16);
        assert_eq!(p5, '0' as u16);
        assert_eq!(p6, '1' as u16);
        assert_eq!(p7, 0);
    }

    #[test]
    fn test_08_gpt_header_crc_updates() {
        let total_sectors = 41_943_040u64;
        let mut layout = GptDiskLayout::create_default_layout(total_sectors).unwrap();
        let crc1 = layout.primary_header.partition_array_crc32;
        layout.resize_data_partition(total_sectors + 1000).unwrap();
        let crc2 = layout.primary_header.partition_array_crc32;
        assert_ne!(crc1, crc2);
    }

    #[test]
    fn test_09_header_self_crc_roundtrip() {
        let total = 2_097_152u64; // 1 GiB
        let mut layout = GptDiskLayout::create_default_layout(total).unwrap();
        let _ = layout.entry_array_bytes();
        let mut hdr = layout.primary_header;
        let bytes = hdr.to_bytes();
        assert_eq!(&bytes[0..8], b"EFI PART");
        let stored = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert_eq!(stored, hdr.header_crc32);
        // Recompute: zero the crc field, CRC over 92 bytes must match
        let mut probe = bytes;
        probe[16..20].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(compute_crc32(&probe), stored);
    }

    #[test]
    fn test_10_write_head_layout() {
        let total = 2_097_152u64;
        let mut layout = GptDiskLayout::create_default_layout(total).unwrap();
        let mut head = vec![0u8; 34 * SECTOR_SIZE];
        layout.write_head(&mut head).unwrap();
        assert_eq!(head[510], 0x55);
        assert_eq!(head[511], 0xAA);
        assert_eq!(head[446 + 4], 0xEE);
        assert_eq!(&head[512..520], b"EFI PART");
        let n = u32::from_le_bytes([head[512 + 80], head[512 + 81], head[512 + 82], head[512 + 83]]);
        assert_eq!(n, 128);
        // ESP entry first bytes = GUID_ESP
        assert_eq!(&head[2 * SECTOR_SIZE..2 * SECTOR_SIZE + 16], &GUID_ESP);
    }

    #[test]
    fn test_11_write_backup_tail() {
        let total = 2_097_152u64;
        let mut layout = GptDiskLayout::create_default_layout(total).unwrap();
        let mut tail = vec![0u8; 33 * SECTOR_SIZE];
        layout.write_backup(&mut tail).unwrap();
        assert_eq!(&tail[32 * SECTOR_SIZE..32 * SECTOR_SIZE + 8], b"EFI PART");
        let cur = u64::from_le_bytes(tail[32 * SECTOR_SIZE + 24..32 * SECTOR_SIZE + 32].try_into().unwrap());
        assert_eq!(cur, total - 1);
    }

    #[test]
    fn test_12_entry_array_crc_covers_128_slots() {
        let total = 2_097_152u64;
        let mut layout = GptDiskLayout::create_default_layout(total).unwrap();
        let arr = layout.entry_array_bytes();
        assert_eq!(arr.len(), 128 * 128);
        assert_eq!(layout.primary_header.partition_array_crc32, compute_crc32(&arr));
        // Slots 4..128 must be fully zero
        for b in &arr[4 * 128..] {
            assert_eq!(*b, 0);
        }
    }

    #[test]
    fn test_13_unique_partition_guids_nonzero_and_distinct() {
        let total = 2_097_152u64;
        let layout = GptDiskLayout::create_default_layout(total).unwrap();
        for p in layout.partitions.iter().take(3) {
            assert_ne!(p.unique_partition_guid, [0u8; 16]);
        }
        assert_ne!(layout.partitions[0].unique_partition_guid, layout.partitions[1].unique_partition_guid);
        assert_ne!(layout.partitions[1].unique_partition_guid, layout.partitions[2].unique_partition_guid);
    }

    #[test]
    fn test_14_parse_gpt_header_roundtrip_from_layout() {
        let total = 4_194_304u64; // 2 GiB — kapasitas nyata vdi
        let mut layout = GptDiskLayout::create_default_layout(total).unwrap();
        let mut head = [0u8; 34 * SECTOR_SIZE];
        layout.write_head(&mut head).unwrap();
        let info = parse_gpt_header_lba1(&head[SECTOR_SIZE..2 * SECTOR_SIZE]).expect("header parse");
        assert!(info.header_crc_ok, "CRC header harus cocok dgn to_bytes()");
        assert_eq!(info.entry_lba, layout.primary_header.partition_entry_lba);
        assert_eq!(info.num_entries, 128);
        assert_eq!(info.entry_size, 128);
        assert_eq!(info.first_usable_lba, layout.primary_header.first_usable_lba);
        assert_eq!(info.last_usable_lba, layout.primary_header.last_usable_lba);
    }

    #[test]
    fn test_15_parse_gpt_entries_roundtrip() {
        let total = 4_194_304u64;
        let mut layout = GptDiskLayout::create_default_layout(total).unwrap();
        let arr = layout.entry_array_bytes();
        let e0 = parse_gpt_entry(&arr[0..128]).unwrap();
        assert!(e0.used);
        assert_eq!(&e0.name[..e0.name_len], b"EFI System Partition");
        let e1 = parse_gpt_entry(&arr[128..256]).unwrap();
        assert_eq!(&e1.name[..e1.name_len], b"RJCFG01");
        let e2 = parse_gpt_entry(&arr[256..384]).unwrap();
        assert_eq!(&e2.name[..e2.name_len], b"RJDATA");
        let e5 = parse_gpt_entry(&arr[5 * 128..6 * 128]).unwrap();
        assert!(!e5.used);
        assert_eq!(e5.start_lba, 0);
    }

    #[test]
    fn test_16_crc32_update_streaming_equals_one_shot() {
        let data: std::vec::Vec<u8> = (0..4200u32).map(|i| (i * 31 % 251) as u8).collect();
        let one_shot = compute_crc32(&data);
        // Pola streaming yang dipakai installer: internal register, negasi tiap chunk.
        let mut internal = 0xFFFF_FFFFu32;
        for chunk in data.chunks(512) {
            internal = !crc32_update(internal, chunk);
        }
        assert_eq!(crc32_update(internal, &[]), one_shot);
        // Chunk tunggal = compute_crc32 biasa.
        assert_eq!(crc32_update(0xFFFF_FFFF, &data), one_shot);
    }
}
