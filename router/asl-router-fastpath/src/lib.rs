//! Open Core Router (OpenCR) — asl-router-fastpath
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA

#![no_std]

pub mod ring;

pub use ring::{DmaAddr, DescriptorRing, RxDesc16, RxDesc32, IommuMapper, dma_cache_clean_inval, CACHELINE_SIZE};

pub const OS_MARKER: &str = "RINJANI_1_0_3";
pub const MODULE_NAME: &str = "fastpath";

pub fn module_status() -> &'static str {
    OS_MARKER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_marker() {
        assert_eq!(module_status(), "RINJANI_1_0_3");
    }

    #[test]
    fn test_align_01_cacheline_alignment() {
        assert_eq!(core::mem::align_of::<DescriptorRing<RxDesc16, 16>>(), 64);
        assert_eq!(core::mem::align_of::<DescriptorRing<RxDesc32, 16>>(), 64);
    }

    #[test]
    fn test_dma_addr_01_zone_safety() {
        let addr_low = DmaAddr::new(0x2000_0000);
        let addr_high = DmaAddr::new(0x1_2000_0000);
        assert!(addr_low.is_zone_dma32());
        assert!(!addr_high.is_zone_dma32());
    }

    #[test]
    fn test_ring_push_pop_fifo() {
        let mut ring: DescriptorRing<RxDesc16, 8> = DescriptorRing::new(RxDesc16::empty());
        assert!(ring.is_empty());

        let mut desc = RxDesc16::empty();
        desc.buffer_addr = 0x1000_2000;
        desc.length = 1514;
        ring.push(desc).expect("Push should succeed");
        assert!(!ring.is_empty());

        let popped = ring.pop().expect("Pop should succeed");
        assert_eq!(popped.buffer_addr, 0x1000_2000);
        assert_eq!(popped.length, 1514);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_ring_saturation_full() {
        let mut ring: DescriptorRing<RxDesc16, 4> = DescriptorRing::new(RxDesc16::empty());
        let desc = RxDesc16::empty();
        assert!(ring.push(desc).is_ok());
        assert!(ring.push(desc).is_ok());
        assert!(ring.push(desc).is_ok());
        // Slot ke-4 membuat ring penuh
        assert!(ring.push(desc).is_err());
    }

    #[test]
    fn test_dma_cache_clean_inval_call() {
        let addr = DmaAddr::new(0x3000_0000);
        dma_cache_clean_inval(addr, 2048);
    }
}