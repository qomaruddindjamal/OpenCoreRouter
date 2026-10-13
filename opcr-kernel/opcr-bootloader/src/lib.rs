//! Open Core Router (OpenCR) — opcr-bootloader
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! First-Stage Bootloader Protocol & Handshake

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";
pub const BOOT_MAGIC: u32 = 0x52494E4A; // 'RINJ'

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootInfo {
    pub magic: u32,
    pub memory_base: u64,
    pub memory_size: u64,
    pub partition_count: u32,
}

impl BootInfo {
    pub const fn new(mem_base: u64, mem_size: u64, partitions: u32) -> Self {
        Self {
            magic: BOOT_MAGIC,
            memory_base: mem_base,
            memory_size: mem_size,
            partition_count: partitions,
        }
    }

    pub fn validate(&self) -> bool {
        self.magic == BOOT_MAGIC && self.memory_size > 0 && self.partition_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_info_validation() {
        let valid_boot = BootInfo::new(0x1000_0000, 0x4000_0000, 4);
        assert!(valid_boot.validate());

        let invalid_boot = BootInfo {
            magic: 0xDEADBEEF,
            memory_base: 0,
            memory_size: 0,
            partition_count: 0,
        };
        assert!(!invalid_boot.validate());
    }

    #[test]
    fn test_boot_magic_constant() {
        assert_eq!(BOOT_MAGIC, 0x52494E4A);
    }

    #[test]
    fn test_boot_info_zero_size_fails() {
        let zero_size = BootInfo::new(0x1000_0000, 0, 2);
        assert!(!zero_size.validate());
    }
}
