//! ASL UART COM1 Telemetry Driver
//! ==================================
//! Direct Port I/O (0x3F8) untuk pancaran telemetri milestone boot [R1]..[R7].

const COM1_PORT: u16 = 0x3F8;

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack, preserves_flags)
    );
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (port, val);
    }
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    #[cfg(target_arch = "x86_64")]
    {
        let ret: u8;
        core::arch::asm!(
            "in al, dx",
            out("al") ret,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
        ret
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = port;
        0
    }
}

pub struct SerialPort;

impl SerialPort {
    pub const fn new() -> Self {
        Self
    }

    pub fn init(&self) {
        unsafe {
            outb(COM1_PORT + 1, 0x00); // Disable interrupts
            outb(COM1_PORT + 3, 0x80); // Enable DLAB (set baud rate divisor)
            outb(COM1_PORT + 0, 0x03); // Set divisor to 3 (38400 baud)
            outb(COM1_PORT + 1, 0x00);
            outb(COM1_PORT + 3, 0x03); // 8 bits, no parity, one stop bit
            outb(COM1_PORT + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
            outb(COM1_PORT + 4, 0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    pub fn is_transmit_empty(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            (inb(COM1_PORT + 5) & 0x20) != 0
        }
        #[cfg(not(target_arch = "x86_64"))]
        true
    }

    pub fn write_byte(&self, b: u8) {
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(COM1_PORT, b);
        }
    }

    pub fn write_str(&self, s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
    }
}
