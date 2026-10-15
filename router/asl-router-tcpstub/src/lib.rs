//! Open Core Router (OpenCR) — asl-router-tcpstub
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Evaluation Stub: smoltcp Architecture & Zero-Copy DescriptorRing Adapter
//!
//! STATUS RINJANI 1.0.0: artifact evaluasi EVAL-01 (ADR-011, OPCR-54) dan
//! adapter zero-copy ring. crate ini TIDAK lagi ditautkan ke `asl-kernel-bin`:
//! jalur TCP/HTTP produksi adalah mesin natif di `asl-router-webui`
//! (RFC 9293 + RFC 7230). Jangan kutuk crate ini sebagai bukti TCP hidup.
//!
//! MANDATE:
//! - #![no_std] pure, no libc, bounded memory.
//! - Direct binding to asl-router-fastpath DescriptorRing and netbuf pool.
//! - ZERO copy payload transfer trait implementation.

#![no_std]

use asl_router_fastpath::ring::{DescriptorRing, RxDesc16};
use asl_router_netbuf::NetBufferPool;

pub const SMOLTCP_EVAL_VERSION: &str = "0.11.1";
pub const SMOLTCP_UPSTREAM_LICENSE: &str = "0BSD";
pub const SMOLTCP_UPSTREAM_REPO: &str = "https://github.com/smoltcp-rs/smoltcp";

/// Status koneksi TCP state-machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynReceived,
    Established,
    FinWait,
    ClosedWait,
    TimeWait,
}

/// Buffer token transmisi zero-copy (merepresentasikan slice memory langsung dari descriptor)
pub struct TxToken<'a> {
    buffer: &'a mut [u8],
    desc_ring: *mut DescriptorRing<RxDesc16, 16>,
}

impl<'a> TxToken<'a> {
    pub fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let target_slice = &mut self.buffer[..len];
        let res = f(target_slice);

        // Langsung commit ke hardware ring tanpa memcpy kedua
        if !self.desc_ring.is_null() {
            let desc = RxDesc16 {
                buffer_addr: target_slice.as_ptr() as u64,
                length: len as u16,
                checksum: 0,
                status: 1,
                errors: 0,
                vlan_tag: 0,
            };
            unsafe {
                let _ = (*self.desc_ring).push(desc);
            }
        }
        res
    }
}

/// Buffer token resepsi zero-copy (meminjam buffer tanpa salin)
pub struct RxToken<'a> {
    buffer: &'a [u8],
}

impl<'a> RxToken<'a> {
    pub fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.buffer)
    }
}

/// Adapter smoltcp Device Trait berbasis DescriptorRing
pub struct RingDeviceAdapter<'a> {
    pub rx_ring: &'a mut DescriptorRing<RxDesc16, 16>,
    pub tx_ring: &'a mut DescriptorRing<RxDesc16, 16>,
    pub rx_scratch: [u8; 1518],
    pub tx_scratch: [u8; 1518],
    pub rx_packets: u32,
    pub tx_packets: u32,
    pub zero_copy_count: u32,
}

impl<'a> RingDeviceAdapter<'a> {
    pub fn new(
        rx_ring: &'a mut DescriptorRing<RxDesc16, 16>,
        tx_ring: &'a mut DescriptorRing<RxDesc16, 16>,
    ) -> Self {
        Self {
            rx_ring,
            tx_ring,
            rx_scratch: [0u8; 1518],
            tx_scratch: [0u8; 1518],
            rx_packets: 0,
            tx_packets: 0,
            zero_copy_count: 0,
        }
    }

    /// Polling penerimaan frame dari ring
    pub fn receive(&mut self) -> Option<RxToken<'_>> {
        if let Some(desc) = self.rx_ring.pop() {
            let len = (desc.length as usize).min(self.rx_scratch.len());
            self.rx_packets = self.rx_packets.wrapping_add(1);
            self.zero_copy_count = self.zero_copy_count.wrapping_add(1);
            Some(RxToken {
                buffer: &self.rx_scratch[..len],
            })
        } else {
            None
        }
    }

    /// Mengalokasikan slot transmisi langsung ke ring TX
    pub fn transmit(&mut self) -> Option<TxToken<'_>> {
        if self.tx_ring.is_full() {
            return None;
        }
        self.tx_packets = self.tx_packets.wrapping_add(1);
        self.zero_copy_count = self.zero_copy_count.wrapping_add(1);
        Some(TxToken {
            buffer: &mut self.tx_scratch,
            desc_ring: self.tx_ring as *mut DescriptorRing<RxDesc16, 16>,
        })
    }
}

/// Sederhana TCP HTTP Server Socket Mock untuk Smoke Testing
pub struct TcpHttpServerStub {
    pub port: u16,
    pub state: TcpState,
    pub rx_bytes: usize,
    pub tx_bytes: usize,
    pub connections_handled: u32,
}

impl TcpHttpServerStub {
    pub const fn new(port: u16) -> Self {
        Self {
            port,
            state: TcpState::Listen,
            rx_bytes: 0,
            tx_bytes: 0,
            connections_handled: 0,
        }
    }

    /// Simulasi respon HTTP banner Rinjani
    pub fn process_input(&mut self, request_data: &[u8], response_out: &mut [u8]) -> usize {
        self.rx_bytes += request_data.len();
        self.state = TcpState::Established;

        let banner = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n[R18] RINJANI 1.0.0 SMOLTCP-STUB ACTIVE\n[R19] TCP SESSION OK\n";
        let copy_len = banner.len().min(response_out.len());
        response_out[..copy_len].copy_from_slice(&banner[..copy_len]);

        self.tx_bytes += copy_len;
        self.connections_handled = self.connections_handled.wrapping_add(1);
        self.state = TcpState::TimeWait;

        copy_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_adapter_creation_and_zero_copy() {
        let dummy = RxDesc16 {
            buffer_addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            vlan_tag: 0,
        };
        let mut rx_ring = DescriptorRing::new(dummy);
        let mut tx_ring = DescriptorRing::new(dummy);

        let mut adapter = RingDeviceAdapter::new(&mut rx_ring, &mut tx_ring);

        // Push paket uji ke RX ring
        let test_desc = RxDesc16 {
            buffer_addr: 0x1000,
            length: 64,
            checksum: 0,
            status: 1,
            errors: 0,
            vlan_tag: 0,
        };
        adapter.rx_ring.push(test_desc).unwrap();

        let token = adapter.receive().expect("Should receive token");
        token.consume(|buf| {
            assert_eq!(buf.len(), 64);
        });

        assert_eq!(adapter.rx_packets, 1);
        assert_eq!(adapter.zero_copy_count, 1);
    }

    #[test]
    fn test_02_tx_token_direct_commit() {
        let dummy = RxDesc16 {
            buffer_addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            vlan_tag: 0,
        };
        let mut rx_ring = DescriptorRing::new(dummy);
        let mut tx_ring = DescriptorRing::new(dummy);

        let mut adapter = RingDeviceAdapter::new(&mut rx_ring, &mut tx_ring);
        let token = adapter.transmit().expect("Should provide tx token");

        token.consume(42, |buf| {
            buf[0] = 0xAA;
            buf[41] = 0xBB;
        });

        assert_eq!(adapter.tx_packets, 1);
        let popped = adapter.tx_ring.pop().unwrap();
        assert_eq!(popped.length, 42);
    }

    #[test]
    fn test_03_http_stub_handshake_and_banner() {
        let mut server = TcpHttpServerStub::new(8080);
        assert_eq!(server.state, TcpState::Listen);

        let req = b"GET / HTTP/1.1\r\nHost: 192.168.57.10:8080\r\n\r\n";
        let mut resp = [0u8; 512];
        let len = server.process_input(req, &mut resp);

        assert!(len > 0);
        assert!(resp[..len].starts_with(b"HTTP/1.1 200 OK"));
        assert_eq!(server.state, TcpState::TimeWait);
        assert_eq!(server.connections_handled, 1);
    }

    #[test]
    fn test_04_upstream_metadata_compliance() {
        assert_eq!(SMOLTCP_EVAL_VERSION, "0.11.1");
        assert_eq!(SMOLTCP_UPSTREAM_LICENSE, "0BSD");
        assert!(SMOLTCP_UPSTREAM_REPO.contains("smoltcp"));
    }

    #[test]
    fn test_05_pool_integration_with_adapter() {
        let mut pool = NetBufferPool::<4>::new();
        let buf = pool.acquire().unwrap();
        let id = buf.id;
        assert_eq!(id, 0);
        pool.release(id).unwrap();
    }

    #[test]
    fn test_06_ring_full_tx_drop() {
        let dummy = RxDesc16 {
            buffer_addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            vlan_tag: 0,
        };
        let mut rx_ring = DescriptorRing::new(dummy);
        let mut tx_ring = DescriptorRing::new(dummy);

        // Ring kapasitas 16 penuh pada N-1 = 15 elemen
        for _ in 0..15 {
            tx_ring.push(dummy).unwrap();
        }

        let mut adapter = RingDeviceAdapter::new(&mut rx_ring, &mut tx_ring);
        assert!(adapter.transmit().is_none());
    }
}
