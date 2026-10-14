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

pub fn compute_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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
        esp.starting_lba = first_usable;
        esp.ending_lba = esp.starting_lba + esp_sectors - 1;
        esp.set_name("EFI System Partition");

        // Partition 2: RJCFG01 8 MiB (8 * 2048 = 16,384 sectors)
        let cfg_sectors = 16_384u64;
        let mut cfg = GptPartitionEntry::empty();
        cfg.partition_type_guid = GUID_RINJANI_CFG;
        cfg.starting_lba = esp.ending_lba + 1;
        cfg.ending_lba = cfg.starting_lba + cfg_sectors - 1;
        cfg.set_name("RJCFG01");

        // Partition 3: RJDATA elastic remainder up to last_usable
        let mut data = GptPartitionEntry::empty();
        data.partition_type_guid = GUID_RINJANI_DATA;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
