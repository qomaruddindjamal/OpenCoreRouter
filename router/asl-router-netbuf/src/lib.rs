//! Open Core Router (OpenCR) — asl-router-netbuf
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Static Zero-Allocation Network Packet Buffer Pool

#![no_std]

pub const NETBUF_SIZE: usize = 2048;

/// NetBuffer mandiri dengan memori statis terikat
#[derive(Clone, Copy)]
pub struct NetBuffer {
    pub id: u16,
    pub length: u16,
    pub data: [u8; NETBUF_SIZE],
}

impl NetBuffer {
    pub const fn empty(id: u16) -> Self {
        Self {
            id,
            length: 0,
            data: [0u8; NETBUF_SIZE],
        }
    }

    pub fn reset(&mut self) {
        self.length = 0;
    }
}

/// Static Power-of-Two Buffer Pool
pub struct NetBufferPool<const CAPACITY: usize> {
    buffers: [NetBuffer; CAPACITY],
    in_use: [bool; CAPACITY],
}

impl<const CAPACITY: usize> NetBufferPool<CAPACITY> {
    pub const fn new() -> Self {
        // Inisialisasi heapless tanpa unwrap
        let buffers = [const { NetBuffer::empty(0) }; CAPACITY];
        let in_use = [false; CAPACITY];
        Self { buffers, in_use }
    }

    pub fn acquire(&mut self) -> Option<&mut NetBuffer> {
        let mut i = 0;
        while i < CAPACITY {
            if !self.in_use[i] {
                self.in_use[i] = true;
                self.buffers[i].id = i as u16;
                self.buffers[i].length = 0;
                return Some(&mut self.buffers[i]);
            }
            i += 1;
        }
        None
    }

    pub fn release(&mut self, id: u16) -> Result<(), &'static str> {
        let idx = id as usize;
        if idx >= CAPACITY {
            return Err("NetBuffer ID out of bounds");
        }
        if !self.in_use[idx] {
            return Err("Double-free guard: NetBuffer already free");
        }
        self.in_use[idx] = false;
        self.buffers[idx].length = 0;
        Ok(())
    }

    pub fn available_count(&self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < CAPACITY {
            if !self.in_use[i] {
                count += 1;
            }
            i += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_acquire_and_release() {
        let mut pool = NetBufferPool::<4>::new();
        assert_eq!(pool.available_count(), 4);

        let buf = pool.acquire().expect("Must acquire buf 0");
        assert_eq!(buf.id, 0);
        assert_eq!(pool.available_count(), 3);

        assert!(pool.release(0).is_ok());
        assert_eq!(pool.available_count(), 4);
    }

    #[test]
    fn test_buffer_pool_exhaustion() {
        let mut pool = NetBufferPool::<2>::new();
        let _b0 = pool.acquire().unwrap();
        let _b1 = pool.acquire().unwrap();
        assert_eq!(pool.available_count(), 0);
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn test_buffer_pool_double_free_guard() {
        let mut pool = NetBufferPool::<2>::new();
        let _b0 = pool.acquire().unwrap();
        assert!(pool.release(0).is_ok());
        assert!(pool.release(0).is_err()); // Double free guard
    }
}
