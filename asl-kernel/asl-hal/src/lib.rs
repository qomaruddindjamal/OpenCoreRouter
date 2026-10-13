//! Open Core Router (OpenCR) — asl-hal
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Hardware Abstraction Layer (CPU/MMU/IRQ/DMA/MMIO)

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";

pub trait CpuContract {
    fn disable_interrupts(&mut self);
    fn enable_interrupts(&mut self);
    fn memory_barrier(&self);
}

pub struct DummyCpu {
    pub interrupts_enabled: bool,
}

impl DummyCpu {
    pub const fn new() -> Self {
        Self { interrupts_enabled: true }
    }
}

impl CpuContract for DummyCpu {
    fn disable_interrupts(&mut self) {
        self.interrupts_enabled = false;
    }

    fn enable_interrupts(&mut self) {
        self.interrupts_enabled = true;
    }

    fn memory_barrier(&self) {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

pub struct MmioRegister<T> {
    address: usize,
    _marker: core::marker::PhantomData<T>,
}

impl<T: Copy> MmioRegister<T> {
    pub const fn new(address: usize) -> Self {
        Self {
            address,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn read_volatile(&self) -> T {
        unsafe {
            core::ptr::read_volatile(self.address as *const T)
        }
    }

    pub fn write_volatile(&self, val: T) {
        unsafe {
            core::ptr::write_volatile(self.address as *mut T, val);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_contract() {
        let mut cpu = DummyCpu::new();
        assert!(cpu.interrupts_enabled);
        cpu.disable_interrupts();
        assert!(!cpu.interrupts_enabled);
        cpu.enable_interrupts();
        assert!(cpu.interrupts_enabled);
        cpu.memory_barrier();
    }

    #[test]
    fn test_hal_marker_info() {
        assert_eq!(OS_MARKER, "RINJANI_1_0_3");
    }

    #[test]
    fn test_mmio_register_memory_semantics() {
        let mut backing_data: u32 = 0xAA55AA55;
        let mmio: MmioRegister<u32> = MmioRegister::new(&mut backing_data as *mut u32 as usize);
        assert_eq!(mmio.read_volatile(), 0xAA55AA55);
        mmio.write_volatile(0x12345678);
        assert_eq!(mmio.read_volatile(), 0x12345678);
    }
}
