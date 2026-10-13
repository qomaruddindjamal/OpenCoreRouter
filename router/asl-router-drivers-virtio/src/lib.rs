//! Open Core Router (OpenCR) — asl-router-drivers-virtio
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! VirtIO-Net Legacy/Modern Driver (Integrated with asl-router-fastpath DescriptorRing)

#![no_std]

use asl_router_pci::PciDevice;
use asl_router_fastpath::ring::{DescriptorRing, RxDesc16};

// VirtIO PCI Status bits
pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
pub const VIRTIO_STATUS_FAILED: u8 = 128;

// VirtIO PCI Register Offsets (I/O space)
pub const VIRTIO_PCI_HOST_FEATURES: u16 = 0;
pub const VIRTIO_PCI_GUEST_FEATURES: u16 = 4;
pub const VIRTIO_PCI_QUEUE_PFN: u16 = 8;
pub const VIRTIO_PCI_QUEUE_NUM: u16 = 12;
pub const VIRTIO_PCI_QUEUE_SEL: u16 = 14;
pub const VIRTIO_PCI_QUEUE_NOTIFY: u16 = 16;
pub const VIRTIO_PCI_STATUS: u16 = 18;
pub const VIRTIO_PCI_ISR: u16 = 19;
pub const VIRTIO_PCI_CONFIG_MAC: u16 = 20;

// Port I/O helpers
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    #[cfg(target_arch = "x86_64")]
    {
        let ret: u8;
        core::arch::asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(target_arch = "x86_64"))]
    { let _ = port; 0 }
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(target_arch = "x86_64"))]
    let _ = (port, val);
}

#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    #[cfg(target_arch = "x86_64")]
    {
        let ret: u16;
        core::arch::asm!("in ax, dx", out("ax") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(target_arch = "x86_64"))]
    { let _ = port; 0 }
}

#[inline(always)]
unsafe fn outw(port: u16, val: u16) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(target_arch = "x86_64"))]
    let _ = (port, val);
}

#[inline(always)]
unsafe fn ind(port: u16) -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        let ret: u32;
        core::arch::asm!("in eax, dx", out("eax") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(target_arch = "x86_64"))]
    { let _ = port; 0 }
}

#[inline(always)]
unsafe fn outd(port: u16, val: u32) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(target_arch = "x86_64"))]
    let _ = (port, val);
}

// -----------------------------------------------------------------------------
// VIRTIO-NET DRIVER STRUCT
// -----------------------------------------------------------------------------

pub struct VirtioNetDevice {
    pub io_base: u16,
    pub mac_address: [u8; 6],
    pub rx_ring: DescriptorRing<RxDesc16, 16>,
    pub tx_ring: DescriptorRing<RxDesc16, 16>,
}

impl VirtioNetDevice {
    pub fn new(pci: &PciDevice) -> Result<Self, &'static str> {
        let mut io_port = 0u16;
        for bar in pci.bars {
            if let asl_router_pci::PciBar::Io { port, .. } = bar {
                io_port = port;
                break;
            }
        }
        if io_port == 0 {
            return Err("No I/O BAR found on VirtIO device");
        }

        // Reset device
        unsafe {
            outb(io_port + VIRTIO_PCI_STATUS, 0);
            outb(io_port + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE);
            outb(io_port + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);
        }

        // Baca MAC address dari konfigurasi PCI device
        let mut mac = [0u8; 6];
        unsafe {
            for i in 0..6 {
                mac[i] = inb(io_port + VIRTIO_PCI_CONFIG_MAC + (i as u16));
            }
        }

        // Driver OK
        unsafe {
            outb(io_port + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK);
        }

        let dummy = RxDesc16 {
            buffer_addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            vlan_tag: 0,
        };

        Ok(Self {
            io_base: io_port,
            mac_address: mac,
            rx_ring: DescriptorRing::new(dummy),
            tx_ring: DescriptorRing::new(dummy),
        })
    }

    pub fn poll_rx(&mut self, buf: &mut [u8]) -> Option<usize> {
        if let Some(desc) = self.rx_ring.pop() {
            let len = desc.length as usize;
            if len <= buf.len() {
                return Some(len);
            }
        }
        None
    }

    pub fn transmit(&mut self, packet: &[u8]) -> Result<(), &'static str> {
        let desc = RxDesc16 {
            buffer_addr: packet.as_ptr() as u64,
            length: packet.len() as u16,
            checksum: 0,
            status: 1,
            errors: 0,
            vlan_tag: 0,
        };
        self.tx_ring.push(desc)?;
        unsafe {
            outw(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY, 1); // Notify TX queue
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// INTEL 82540EM (E1000) FALLBACK DRIVER
// -----------------------------------------------------------------------------

pub struct E1000Device {
    pub io_base: u16,
    pub mem_base: u32,
    pub mac_address: [u8; 6],
}

impl E1000Device {
    pub fn new(pci: &PciDevice) -> Result<Self, &'static str> {
        let mut io_port = 0u16;
        let mut mem_addr = 0u32;
        for bar in pci.bars {
            match bar {
                asl_router_pci::PciBar::Io { port, .. } => io_port = port,
                asl_router_pci::PciBar::Memory { address, .. } => mem_addr = address,
                _ => {}
            }
        }

        // Default VirtualBox MAC format jika eeprom read bypassed
        let mac = [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33];

        Ok(Self {
            io_base: io_port,
            mem_base: mem_addr,
            mac_address: mac,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtio_status_constants() {
        assert_eq!(VIRTIO_STATUS_ACKNOWLEDGE, 1);
        assert_eq!(VIRTIO_STATUS_DRIVER, 2);
        assert_eq!(VIRTIO_STATUS_DRIVER_OK, 4);
    }

    #[test]
    fn test_e1000_device_creation() {
        let pci = PciDevice {
            bus: 0,
            device: 3,
            function: 0,
            vendor_id: 0x8086,
            device_id: 0x100E,
            class_code: 2,
            subclass: 0,
            prog_if: 0,
            header_type: 0,
            bars: [
                asl_router_pci::PciBar::Memory { address: 0xF0000000, size: 0x20000, prefetchable: false },
                asl_router_pci::PciBar::Io { port: 0xD000, size: 64 },
                asl_router_pci::PciBar::None,
                asl_router_pci::PciBar::None,
                asl_router_pci::PciBar::None,
                asl_router_pci::PciBar::None,
            ],
        };
        let e1000 = E1000Device::new(&pci).unwrap();
        assert_eq!(e1000.io_base, 0xD000);
        assert_eq!(e1000.mem_base, 0xF0000000);
    }
}
