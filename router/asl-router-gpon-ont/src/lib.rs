//! Open Core Router (OpenCR) - asl-router-gpon-ont
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";
pub const MODULE_NAME: &str = "gpon-ont";

pub fn module_status() -> &'static str {
    OS_MARKER
}

pub fn module_name() -> &'static str {
    MODULE_NAME
}

pub fn is_operational() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_marker() {
        assert_eq!(module_status(), "RINJANI_1_0_3");
    }

    #[test]
    fn test_module_name() {
        assert_eq!(module_name(), "gpon-ont");
    }

    #[test]
    fn test_module_operational() {
        assert!(is_operational());
    }
}