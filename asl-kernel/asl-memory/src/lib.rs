//! Open Core Router (OpenCR) — asl-memory
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Static Page-Table Slicing & Buddy Memory Allocator

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";
pub const PAGE_SIZE: usize = 4096;
pub const MAX_PAGES: usize = 256;

pub struct StaticPageAllocator {
    bitmap: [u64; 4], // 256 bits = 256 pages = 1 MB zone
    base_phys_addr: usize,
}

impl StaticPageAllocator {
    pub const fn new(base_phys_addr: usize) -> Self {
        Self {
            bitmap: [0; 4],
            base_phys_addr,
        }
    }

    pub fn allocate_page(&mut self) -> Option<usize> {
        let mut idx = 0;
        while idx < MAX_PAGES {
            let word_idx = idx / 64;
            let bit_idx = idx % 64;
            if (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
                self.bitmap[word_idx] |= 1u64 << bit_idx;
                return Some(self.base_phys_addr + (idx * PAGE_SIZE));
            }
            idx += 1;
        }
        None
    }

    pub fn free_page(&mut self, phys_addr: usize) -> Result<(), &'static str> {
        if phys_addr < self.base_phys_addr {
            return Err("Address below base");
        }
        let offset = phys_addr - self.base_phys_addr;
        if offset % PAGE_SIZE != 0 {
            return Err("Unaligned page address");
        }
        let idx = offset / PAGE_SIZE;
        if idx >= MAX_PAGES {
            return Err("Address out of zone bounds");
        }
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        if (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
            return Err("Double free detected");
        }
        self.bitmap[word_idx] &= !(1u64 << bit_idx);
        Ok(())
    }

    pub fn free_page_count(&self) -> usize {
        let mut free = 0;
        let mut idx = 0;
        while idx < MAX_PAGES {
            let word_idx = idx / 64;
            let bit_idx = idx % 64;
            if (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
                free += 1;
            }
            idx += 1;
        }
        free
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_page_allocator() {
        let mut alloc = StaticPageAllocator::new(0x1000_0000);
        let p0 = alloc.allocate_page().expect("Should allocate p0");
        let p1 = alloc.allocate_page().expect("Should allocate p1");
        assert_eq!(p0, 0x1000_0000);
        assert_eq!(p1, 0x1000_1000);

        alloc.free_page(p0).expect("Free should succeed");
        let p0_reused = alloc.allocate_page().expect("Should reuse p0");
        assert_eq!(p0_reused, 0x1000_0000);
    }

    #[test]
    fn test_unaligned_or_invalid_free() {
        let mut alloc = StaticPageAllocator::new(0x2000_0000);
        assert!(alloc.free_page(0x1000_0000).is_err()); // below base
        assert!(alloc.free_page(0x2000_0001).is_err()); // unaligned
        assert!(alloc.free_page(0x3000_0000).is_err()); // out of bounds
    }

    #[test]
    fn test_free_page_count_tracking() {
        let mut alloc = StaticPageAllocator::new(0x4000_0000);
        assert_eq!(alloc.free_page_count(), 256);
        let p0 = alloc.allocate_page().unwrap();
        assert_eq!(alloc.free_page_count(), 255);
        alloc.free_page(p0).unwrap();
        assert_eq!(alloc.free_page_count(), 256);
    }
}
