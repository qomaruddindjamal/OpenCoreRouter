//! Open Core Router (OpenCR) — asl-router-pci
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Legacy PCI Configuration Space Enumerator (Ports 0xCF8 / 0xCFC)

#![no_std]

pub const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
pub const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Abstraksi Base Address Register (BAR)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    None,
    Io {
        port: u16,
        size: u32,
    },
    Memory {
        address: u32,
        size: u32,
        prefetchable: bool,
    },
}

/// Deskriptor Perangkat PCI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
    pub bars: [PciBar; 6],
}

impl PciDevice {
    pub const fn empty() -> Self {
        Self {
            bus: 0,
            device: 0,
            function: 0,
            vendor_id: 0xFFFF,
            device_id: 0xFFFF,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
            header_type: 0,
            bars: [PciBar::None; 6],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.vendor_id != 0xFFFF && self.vendor_id != 0x0000
    }

    pub fn is_virtio_net(&self) -> bool {
        self.vendor_id == 0x1AF4 && (self.device_id == 0x1000 || self.device_id == 0x1041)
    }

    pub fn is_virtio_blk(&self) -> bool {
        self.vendor_id == 0x1AF4 && (self.device_id == 0x1001 || self.device_id == 0x1042)
    }

    pub fn is_e1000(&self) -> bool {
        self.vendor_id == 0x8086
            && (self.device_id == 0x100E || self.device_id == 0x100F || self.device_id == 0x10D3)
    }
}

// -----------------------------------------------------------------------------
// HARDWARE I/O READ/WRITE
// -----------------------------------------------------------------------------

#[inline(always)]
unsafe fn pci_outd(port: u16, val: u32) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    core::arch::asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") val,
        options(nomem, nostack, preserves_flags)
    );
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = (port, val);
}

#[inline(always)]
unsafe fn pci_ind(port: u16) -> u32 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let ret: u32;
        core::arch::asm!(
            "in eax, dx",
            out("eax") ret,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
        ret
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let _ = port;
        0xFFFF_FFFF
    }
}

pub fn pci_config_read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        pci_outd(PCI_CONFIG_ADDRESS, address);
        pci_ind(PCI_CONFIG_DATA)
    }
}

pub fn pci_config_write_u32(bus: u8, device: u8, function: u8, offset: u8, val: u32) {
    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        pci_outd(PCI_CONFIG_ADDRESS, address);
        pci_outd(PCI_CONFIG_DATA, val);
    }
}

/// Aktifkan IO Space, Memory Space & Bus Master DMA pada PCI Command register.
pub fn pci_enable_bus_master(bus: u8, device: u8, function: u8) {
    let cmd = pci_config_read_u32(bus, device, function, 0x04);
    pci_config_write_u32(bus, device, function, 0x04, cmd | 0x0007);
}

pub fn pci_config_read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let dword = pci_config_read_u32(bus, device, function, offset);
    let shift = (offset & 2) * 8;
    ((dword >> shift) & 0xFFFF) as u16
}

pub fn pci_config_read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let dword = pci_config_read_u32(bus, device, function, offset);
    let shift = (offset & 3) * 8;
    ((dword >> shift) & 0xFF) as u8
}

// -----------------------------------------------------------------------------
// PCI ENUMERATION ENGINE
// -----------------------------------------------------------------------------

pub struct PciEnumerator;

impl PciEnumerator {
    pub fn scan_device(bus: u8, dev: u8, func: u8) -> Option<PciDevice> {
        let vendor_id = pci_config_read_u16(bus, dev, func, 0x00);
        if vendor_id == 0xFFFF || vendor_id == 0x0000 {
            return None;
        }

        let device_id = pci_config_read_u16(bus, dev, func, 0x02);
        let class_rev = pci_config_read_u32(bus, dev, func, 0x08);
        let class_code = ((class_rev >> 24) & 0xFF) as u8;
        let subclass = ((class_rev >> 16) & 0xFF) as u8;
        let prog_if = ((class_rev >> 8) & 0xFF) as u8;
        let header_type = pci_config_read_u8(bus, dev, func, 0x0E);

        let mut bars = [PciBar::None; 6];
        if (header_type & 0x7F) == 0 {
            // Standar PCI Header 0x00 memiliki 6 BAR
            let mut bar_idx = 0;
            while bar_idx < 6 {
                let bar_offset = 0x10 + (bar_idx as u8) * 4;
                let bar_raw = pci_config_read_u32(bus, dev, func, bar_offset);
                if bar_raw != 0 {
                    if (bar_raw & 1) == 1 {
                        // I/O Space BAR
                        let port = (bar_raw & 0xFFFC) as u16;
                        bars[bar_idx] = PciBar::Io { port, size: 0 };
                    } else {
                        // Memory Space BAR
                        let address = bar_raw & 0xFFFFFFF0;
                        let prefetch = (bar_raw & 0x08) != 0;
                        bars[bar_idx] = PciBar::Memory {
                            address,
                            size: 0,
                            prefetchable: prefetch,
                        };
                    }
                }
                bar_idx += 1;
            }
        }

        Some(PciDevice {
            bus,
            device: dev,
            function: func,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            header_type,
            bars,
        })
    }

    pub fn enumerate<F>(mut callback: F)
    where
        F: FnMut(PciDevice),
    {
        for bus in 0..=3 {
            for dev in 0..32 {
                let header = pci_config_read_u8(bus, dev, 0, 0x0E);
                let max_func = if (header & 0x80) != 0 { 8 } else { 1 };
                for func in 0..max_func {
                    if let Some(device) = Self::scan_device(bus, dev, func) {
                        callback(device);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_device_helpers() {
        let virtio = PciDevice {
            bus: 0,
            device: 3,
            function: 0,
            vendor_id: 0x1AF4,
            device_id: 0x1000,
            class_code: 0x02,
            subclass: 0x00,
            prog_if: 0,
            header_type: 0,
            bars: [
                PciBar::Io {
                    port: 0xC000,
                    size: 64,
                },
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
            ],
        };
        assert!(virtio.is_valid());
        assert!(virtio.is_virtio_net());
        assert!(!virtio.is_virtio_blk());
        assert!(!virtio.is_e1000());

        let blk = PciDevice {
            bus: 0,
            device: 5,
            function: 0,
            vendor_id: 0x1AF4,
            device_id: 0x1001,
            class_code: 0x01,
            subclass: 0x00,
            prog_if: 0,
            header_type: 0,
            bars: [
                PciBar::Io {
                    port: 0xC100,
                    size: 64,
                },
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
                PciBar::None,
            ],
        };
        assert!(blk.is_valid());
        assert!(!blk.is_virtio_net());
        assert!(blk.is_virtio_blk());
        assert!(!blk.is_e1000());

        let e1000 = PciDevice {
            bus: 0,
            device: 4,
            function: 0,
            vendor_id: 0x8086,
            device_id: 0x100E,
            class_code: 0x02,
            subclass: 0x00,
            prog_if: 0,
            header_type: 0,
            bars: [PciBar::None; 6],
        };
        assert!(e1000.is_valid());
        assert!(!e1000.is_virtio_net());
        assert!(e1000.is_e1000());
    }

    #[test]
    fn test_pci_empty_device_is_invalid() {
        let empty = PciDevice::empty();
        assert!(!empty.is_valid());
    }

    #[test]
    fn test_pci_bar_variants() {
        let io_bar = PciBar::Io {
            port: 0xD000,
            size: 32,
        };
        let mem_bar = PciBar::Memory {
            address: 0xE0000000,
            size: 4096,
            prefetchable: false,
        };
        assert_ne!(io_bar, mem_bar);
    }
}
