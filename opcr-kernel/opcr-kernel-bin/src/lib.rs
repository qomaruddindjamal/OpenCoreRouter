//! Open Core Router (OpenCR) — opcr-kernel-bin
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Kernel Top-Level Orchestration & Integration Binary

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";

pub fn kernel_init(boot_info: &opcr_bootloader::BootInfo) -> Result<opcr_core::KernelExecutive, &'static str> {
    if !boot_info.validate() {
        return Err("Invalid bootloader info");
    }

    let mut executive = opcr_core::KernelExecutive::new();
    let partition_control = opcr_core::PartitionDescriptor::new(1, 10_000, 2_000);
    executive.register_partition(partition_control)?;

    Ok(executive)
}

pub fn kernel_handover_sequence() -> &'static str {
    "R1 RINJANI HANDOVER"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_init_flow() {
        let boot = opcr_bootloader::BootInfo::new(0x1000_0000, 0x2000_0000, 2);
        let exec = kernel_init(&boot).expect("Kernel init must succeed");
        assert_eq!(exec.version_info(), "RINJANI_1_0_3");
    }

    #[test]
    fn test_kernel_init_invalid_boot_fails() {
        let invalid_boot = opcr_bootloader::BootInfo::new(0, 0, 0);
        assert!(kernel_init(&invalid_boot).is_err());
    }

    #[test]
    fn test_kernel_handover_marker() {
        assert_eq!(kernel_handover_sequence(), "R1 RINJANI HANDOVER");
    }
}
