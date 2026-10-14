//! ASL Core Kernel — asl-kernel-bin
//! Version: v1.0.0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Kernel Top-Level Orchestration & Integration Binary

#![no_std]

pub const KERNEL_VERSION: &str = "v1.0.0";
pub const DISTRO_CODENAME: &str = "RINJANI 1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootConfig {
    pub strict_net: bool,
    pub dhcp_timeout_secs: u32,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            strict_net: true,
            dhcp_timeout_secs: 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpBootResult {
    Bound { ip: [u8; 4], gw: [u8; 4], mask: [u8; 4] },
    HaltedStrictNoOffer,
    DevFallback { ip: [u8; 4], gw: [u8; 4] },
}

pub fn evaluate_dhcp_outcome(
    has_offer: bool,
    offered_ip: [u8; 4],
    gateway: [u8; 4],
    mask: [u8; 4],
    cfg: BootConfig,
) -> DhcpBootResult {
    if has_offer {
        DhcpBootResult::Bound { ip: offered_ip, gw: gateway, mask }
    } else if cfg.strict_net {
        DhcpBootResult::HaltedStrictNoOffer
    } else {
        DhcpBootResult::DevFallback {
            ip: [192, 0, 2, 199],
            gw: [192, 0, 2, 254],
        }
    }
}

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

    #[test]
    fn test_strict_net_default_is_true() {
        let cfg = BootConfig::default();
        assert!(cfg.strict_net);
        assert_eq!(cfg.dhcp_timeout_secs, 15);
    }

    #[test]
    fn test_strict_net_halt_when_no_offer() {
        let cfg = BootConfig::default();
        let res = evaluate_dhcp_outcome(false, [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], cfg);
        assert_eq!(res, DhcpBootResult::HaltedStrictNoOffer);
    }

    #[test]
    fn test_strict_net_bound_when_offer_present() {
        let cfg = BootConfig::default();
        let res = evaluate_dhcp_outcome(
            true,
            [192, 0, 2, 100],
            [192, 0, 2, 254],
            [255, 255, 255, 0],
            cfg,
        );
        assert_eq!(
            res,
            DhcpBootResult::Bound {
                ip: [192, 0, 2, 100],
                gw: [192, 0, 2, 254],
                mask: [255, 255, 255, 0],
            }
        );
    }

    #[test]
    fn test_dev_fallback_when_strict_net_disabled() {
        let cfg = BootConfig { strict_net: false, dhcp_timeout_secs: 5 };
        let res = evaluate_dhcp_outcome(false, [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], cfg);
        assert_eq!(
            res,
            DhcpBootResult::DevFallback {
                ip: [192, 0, 2, 199],
                gw: [192, 0, 2, 254],
            }
        );
    }
}
