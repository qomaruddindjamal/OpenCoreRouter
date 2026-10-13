//! ASL Core Kernel — asl-kernel-bin
//! Version: v1.0.0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Kernel Top-Level Orchestration & Integration Binary

#![no_std]

pub const KERNEL_VERSION: &str = "v1.0.0";
pub const DISTRO_CODENAME: &str = "RINJANI 1.0.0";

pub fn kernel_init(boot_info: &asl_bootloader::BootInfo) -> Result<asl_kernel_core::KernelExecutive, &'static str> {
    if !boot_info.validate() {
        return Err("Invalid bootloader info");
    }

    let mut executive = asl_kernel_core::KernelExecutive::new();
    let partition_control = asl_kernel_core::PartitionDescriptor::new(1, 10_000, 2_000);
    executive.register_partition(partition_control)?;

    Ok(executive)
}

pub fn kernel_boot_banner() -> (&'static str, &'static str) {
    (
        "[R1] RINJANI 1.0.0 — Open Core Router",
        "[R2] Powered by ASL Core Kernel v1.0.0",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_init_flow() {
        let boot = asl_bootloader::BootInfo::new(0x1000_0000, 0x2000_0000, 2);
        let exec = kernel_init(&boot).expect("Kernel init must succeed");
        assert_eq!(exec.version_info(), "ASL_CORE_KERNEL_v1_0_0");
    }

    #[test]
    fn test_kernel_init_invalid_boot_fails() {
        let invalid_boot = asl_bootloader::BootInfo::new(0, 0, 0);
        assert!(kernel_init(&invalid_boot).is_err());
    }

    #[test]
    fn test_kernel_handover_marker() {
        let (r1, r2) = kernel_boot_banner();
        assert_eq!(r1, "[R1] RINJANI 1.0.0 — Open Core Router");
        assert_eq!(r2, "[R2] Powered by ASL Core Kernel v1.0.0");
    }
}
