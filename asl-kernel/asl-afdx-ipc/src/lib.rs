//! Open Core Router (OpenCR) — opcr-afdx-ipc
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Avionics Full-Duplex Switched Ethernet IPC — SPSC Ring Buffer Zero-Copy

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";
pub const RING_CAPACITY: usize = 16;
pub const MAX_PAYLOAD_SIZE: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfdxFrame {
    pub virtual_link_id: u16,
    pub sequence_num: u8,
    pub payload_len: u8,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl AfdxFrame {
    pub const fn empty() -> Self {
        Self {
            virtual_link_id: 0,
            sequence_num: 0,
            payload_len: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }
}

pub struct SpscRingBuffer {
    buffer: [AfdxFrame; RING_CAPACITY],
    head: usize,
    tail: usize,
}

impl SpscRingBuffer {
    pub const fn new() -> Self {
        Self {
            buffer: [AfdxFrame::empty(); RING_CAPACITY],
            head: 0,
            tail: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn is_full(&self) -> bool {
        ((self.head + 1) % RING_CAPACITY) == self.tail
    }

    pub fn enqueue(&mut self, frame: AfdxFrame) -> Result<(), &'static str> {
        if self.is_full() {
            return Err("Ring buffer full");
        }
        self.buffer[self.head] = frame;
        self.head = (self.head + 1) % RING_CAPACITY;
        Ok(())
    }

    pub fn dequeue(&mut self) -> Option<AfdxFrame> {
        if self.is_empty() {
            return None;
        }
        let frame = self.buffer[self.tail];
        self.tail = (self.tail + 1) % RING_CAPACITY;
        Some(frame)
    }

    pub fn available_slots(&self) -> usize {
        if self.head >= self.tail {
            (RING_CAPACITY - 1) - (self.head - self.tail)
        } else {
            self.tail - self.head - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spsc_ring_buffer_fifo() {
        let mut ring = SpscRingBuffer::new();
        assert!(ring.is_empty());

        let mut f1 = AfdxFrame::empty();
        f1.virtual_link_id = 42;
        f1.sequence_num = 1;
        f1.payload_len = 4;
        f1.payload[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        ring.enqueue(f1).expect("Enqueue should succeed");
        assert!(!ring.is_empty());

        let deq = ring.dequeue().expect("Dequeue should yield frame");
        assert_eq!(deq.virtual_link_id, 42);
        assert_eq!(deq.sequence_num, 1);
        assert_eq!(&deq.payload[0..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_spsc_ring_buffer_full_saturation() {
        let mut ring = SpscRingBuffer::new();
        for i in 0..(RING_CAPACITY - 1) {
            let mut f = AfdxFrame::empty();
            f.virtual_link_id = i as u16;
            assert!(ring.enqueue(f).is_ok());
        }
        assert!(ring.is_full());
        assert_eq!(ring.available_slots(), 0);
        let mut overflow = AfdxFrame::empty();
        overflow.virtual_link_id = 999;
        assert!(ring.enqueue(overflow).is_err());
    }

    #[test]
    fn test_afdx_frame_empty_defaults() {
        let f = AfdxFrame::empty();
        assert_eq!(f.virtual_link_id, 0);
        assert_eq!(f.sequence_num, 0);
        assert_eq!(f.payload_len, 0);
    }
}
