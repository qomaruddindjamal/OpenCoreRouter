//! Open Core Router (OpenCR) — opcr-fastpath::ring
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Lock-Free SPSC Descriptor Ring (Intel 82599 / i40e architecture)

use core::sync::atomic::{AtomicU32, Ordering};

pub const OS_MARKER: &str = "RINJANI_1_0_3";

/// Constraint ALIGN-01: Cacheline size 64 bytes.
pub const CACHELINE_SIZE: usize = 64;

/// Constraint DMA-ADDR-01: Abstract DMA physical address handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaAddr(pub u64);

impl DmaAddr {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Validasi ZoneDMA32 (<4GB) untuk arsitektur 32-bit legacy
    #[inline(always)]
    pub fn is_zone_dma32(&self) -> bool {
        self.0 <= 0xFFFF_FFFF
    }
}

/// Constraint DMA-ADDR-01: IOMMU Mapper Trait stub
pub trait IommuMapper {
    fn map_page(&mut self, virt_addr: usize, phys_addr: DmaAddr) -> Result<(), &'static str>;
    fn unmap_page(&mut self, virt_addr: usize) -> Result<(), &'static str>;
}

/// Constraint DMA-CACHE-01: HAL cache maintenance barrier
#[inline(always)]
pub fn dma_cache_clean_inval(_addr: DmaAddr, _len: usize) {
    // Di arsitektur x86_64, DMA berbasis hardware snooping koheren (no-op).
    // Untuk platform AArch64 atau MIPS non-koheren, instruksi invalidasi dimasukkan di sini.
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
}

/// Format Descriptor 16-byte (Legacy Intel 82599 style)
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxDesc16 {
    pub buffer_addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub vlan_tag: u16,
}

impl RxDesc16 {
    pub const fn empty() -> Self {
        Self {
            buffer_addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            vlan_tag: 0,
        }
    }
}

// Compile-time check ukuran RxDesc16 = 16 bytes
const _: () = assert!(core::mem::size_of::<RxDesc16>() == 16);

/// Format Descriptor 32-byte (Advanced read format i40e / E810 style)
#[repr(C, align(32))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxDesc32 {
    pub packet_buffer_addr: u64,
    pub header_buffer_addr: u64,
    pub qword1: u64,
    pub qword2: u64,
}

impl RxDesc32 {
    pub const fn empty() -> Self {
        Self {
            packet_buffer_addr: 0,
            header_buffer_addr: 0,
            qword1: 0,
            qword2: 0,
        }
    }
}

// Compile-time check ukuran RxDesc32 = 32 bytes
const _: () = assert!(core::mem::size_of::<RxDesc32>() == 32);

/// Ring Buffer SPSC dengan Kapasitas Power-of-2 dan Aligned 64-byte
#[repr(C, align(64))]
pub struct DescriptorRing<T, const N: usize> {
    descriptors: [T; N],
    head: AtomicU32,
    tail: AtomicU32,
}

impl<T: Copy, const N: usize> DescriptorRing<T, N> {
    // Compile-time constraint: Ukuran N wajib power-of-two
    pub const fn new(init_val: T) -> Self {
        assert!(N > 0 && (N & (N - 1)) == 0, "Ring capacity must be power-of-two");
        Self {
            descriptors: [init_val; N],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        let h = self.head.load(Ordering::Acquire);
        let t = self.tail.load(Ordering::Acquire);
        ((h.wrapping_add(1)) & (N as u32 - 1)) == (t & (N as u32 - 1))
    }

    pub fn push(&mut self, desc: T) -> Result<(), &'static str> {
        let h = self.head.load(Ordering::Relaxed);
        let t = self.tail.load(Ordering::Acquire);

        if ((h.wrapping_add(1)) & (N as u32 - 1)) == (t & (N as u32 - 1)) {
            return Err("Ring buffer full");
        }

        let idx = (h as usize) & (N - 1);
        self.descriptors[idx] = desc;
        self.head.store(h.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        let h = self.head.load(Ordering::Acquire);
        let t = self.tail.load(Ordering::Relaxed);

        if (h & (N as u32 - 1)) == (t & (N as u32 - 1)) {
            return None;
        }

        let idx = (t as usize) & (N - 1);
        let val = self.descriptors[idx];
        self.tail.store(t.wrapping_add(1), Ordering::Release);
        Some(val)
    }
}
