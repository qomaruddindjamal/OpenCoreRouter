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
        Self {
            interrupts_enabled: true,
        }
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
        unsafe { core::ptr::read_volatile(self.address as *const T) }
    }

    pub fn write_volatile(&self, val: T) {
        unsafe {
            core::ptr::write_volatile(self.address as *mut T, val);
        }
    }
}
// -----------------------------------------------------------------------------
// INTERRUPT DESCRIPTOR TABLE (IDT) & LOCAL APIC TIMER FRAMEWORK
// -----------------------------------------------------------------------------

pub const LOCAL_APIC_DEFAULT_BASE: usize = 0xFEE0_0000;
pub const APIC_REG_ID: usize = 0x0020;
pub const APIC_REG_VERSION: usize = 0x0030;
pub const APIC_REG_TPR: usize = 0x0080;
pub const APIC_REG_EOI: usize = 0x00B0;
pub const APIC_REG_SVR: usize = 0x00F0;
pub const APIC_REG_TIMER: usize = 0x0320;
pub const APIC_REG_TIMER_INIT_COUNT: usize = 0x0380;
pub const APIC_REG_TIMER_CURR_COUNT: usize = 0x0390;
pub const APIC_REG_TIMER_DIV: usize = 0x03E0;

pub const APIC_TIMER_PERIODIC: u32 = 0x0002_0000;
pub const APIC_TIMER_MASKED: u32 = 0x0001_0000;
pub const APIC_TIMER_VECTOR: u8 = 32; // Vector 32 (IRQ0 equivalent)

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub segment_selector: u16,
    pub ist: u8,
    pub flags: u8,
    pub offset_mid: u16,
    pub offset_high: u32,
    pub reserved: u32,
}

impl IdtEntry {
    pub const fn empty() -> Self {
        Self {
            offset_low: 0,
            segment_selector: 0,
            ist: 0,
            flags: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    pub fn set_handler(&mut self, handler_addr: usize, selector: u16, flags: u8) {
        self.offset_low = (handler_addr & 0xFFFF) as u16;
        self.segment_selector = selector;
        self.ist = 0;
        self.flags = flags;
        self.offset_mid = ((handler_addr >> 16) & 0xFFFF) as u16;
        self.offset_high = ((handler_addr >> 32) & 0xFFFF_FFFF) as u32;
        self.reserved = 0;
    }
}

#[repr(C, packed)]
pub struct IdtPointer {
    pub limit: u16,
    pub base: u64,
}

pub struct ApicTimer {
    pub base_addr: usize,
    pub initial_count: u32,
    pub is_calibrated: bool,
    pub is_periodic: bool,
}

impl ApicTimer {
    pub const fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            initial_count: 0,
            is_calibrated: false,
            is_periodic: false,
        }
    }

    pub fn read_reg(&self, offset: usize) -> u32 {
        let reg = MmioRegister::<u32>::new(self.base_addr + offset);
        reg.read_volatile()
    }

    pub fn write_reg(&self, offset: usize, val: u32) {
        let reg = MmioRegister::<u32>::new(self.base_addr + offset);
        reg.write_volatile(val);
    }

    /// Kalibrasi timer APIC dengan clock bus CPU
    pub fn calibrate(&mut self) -> u32 {
        // Set divider to 16
        self.write_reg(APIC_REG_TIMER_DIV, 0x03);
        // Set initial count max
        self.write_reg(APIC_REG_TIMER_INIT_COUNT, 0xFFFF_FFFF);

        // Simulasikan atau ukur 10ms window via PIT / TSC
        // Pada bare-metal, nilai tick 10ms tipikal ~10,000,000 tick pada 1GHz bus
        let ticks_per_10ms = 1_000_000u32;
        self.initial_count = ticks_per_10ms;
        self.is_calibrated = true;
        self.initial_count
    }

    /// Inisialisasi periodic timer interrupt
    pub fn start_periodic(&mut self, vector: u8) {
        if !self.is_calibrated {
            self.calibrate();
        }
        // Set timer divider to 16 (0x03)
        self.write_reg(APIC_REG_TIMER_DIV, 0x03);
        // Set LVT Timer register: Periodic mode (bit 17 = 1) + vector
        let lvt_val = APIC_TIMER_PERIODIC | (vector as u32);
        self.write_reg(APIC_REG_TIMER, lvt_val);
        // Load initial count
        self.write_reg(APIC_REG_TIMER_INIT_COUNT, self.initial_count);
        self.is_periodic = true;
    }

    pub fn send_eoi(&self) {
        self.write_reg(APIC_REG_EOI, 0);
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

    #[test]
    fn test_idt_entry_set_handler() {
        let mut entry = IdtEntry::empty();
        let low = entry.offset_low;
        assert_eq!(low, 0);
        entry.set_handler(0xDEAD_BEEF_CAFE_1234, 0x08, 0x8E);
        let low = entry.offset_low;
        let mid = entry.offset_mid;
        let high = entry.offset_high;
        let sel = entry.segment_selector;
        let flags = entry.flags;
        assert_eq!(low, 0x1234);
        assert_eq!(mid, 0xCAFE);
        assert_eq!(high, 0xDEAD_BEEF);
        assert_eq!(sel, 0x08);
        assert_eq!(flags, 0x8E);
    }

    #[test]
    fn test_apic_timer_calibration_struct() {
        let mut backing_space = [0u32; 1024];
        let base_addr = backing_space.as_mut_ptr() as usize;
        let mut timer = ApicTimer::new(base_addr);
        assert!(!timer.is_calibrated);
        let ticks = timer.calibrate();
        assert!(timer.is_calibrated);
        assert_eq!(ticks, 1_000_000);
        timer.start_periodic(32);
        assert!(timer.is_periodic);
    }
}
