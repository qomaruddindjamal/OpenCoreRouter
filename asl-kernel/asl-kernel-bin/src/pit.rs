//! ASL PIT 8254 Time Base (polled)
//! =================================
//! Sumber waktu nyata untuk pacing DHCP, retransmit TCP, dan uptime WebUI.
//!
//! Channel 2 mode 2 (rate generator) dengan reload 0xFFFF => periode latch
//! 65536 / 1.193182 MHz = 54,9 ms. `pump()` memakai latch-command lalu
//! pengurangan modular, jadi beberapa wrap antar dua pembacaan tetap terhitung
//! selama selisih waktu < 54,9 ms. Interval yang lebih panjang (mis. output
//! serial 2 KiB pada 38400 baud) kehilangan satu wrap sehingga jam berjalan
//! MELENA (under-count), bukan berlebih. Time base ini untuk pacing, BUKAN
//! timestamp presisi. [VERIFIED: latch dibaca langsung dari PIO 0x42]

const PIT_CH2_DATA: u16 = 0x42;
const PIT_CMD: u16 = 0x43;
/// Jumlah tick PIT per millisecond (1.193182 MHz).
const TICKS_PER_MS: u64 = 1_193;

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
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
#[inline(always)]
unsafe fn outb(_port: u16, _val: u8) {}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
unsafe fn inb(_port: u16) -> u8 {
    0
}

pub struct PitTimer {
    counts: u64,
    last: u16,
    armed: bool,
}

impl PitTimer {
    pub const fn new() -> Self {
        Self {
            counts: 0,
            last: 0,
            armed: false,
        }
    }

    /// Program ch2: lobyte/hibyte, mode 2, binary, reload 0xFFFF.
    pub fn init(&mut self) {
        unsafe {
            outb(PIT_CMD, 0xB4); // SC=ch2(10) RW=11 MODE=010 BCD=0
            outb(PIT_CH2_DATA, 0xFF);
            outb(PIT_CH2_DATA, 0xFF);
        }
        self.last = 0xFFFF;
        self.counts = 0;
        self.armed = true;
    }

    /// Baca latch ch2 saat ini (nilai 0 == reload penuh; selisih modular tetap benar).
    fn latch(&mut self) -> u16 {
        unsafe {
            outb(PIT_CMD, 0x80); // SC=ch2 RW=00 (latch command)
            let lo = inb(PIT_CH2_DATA);
            let hi = inb(PIT_CH2_DATA);
            (lo as u16) | ((hi as u16) << 8)
        }
    }

    /// Akumulasikan selisih counter. Harus dipanggil setiap iterasi poll loop.
    pub fn pump(&mut self) {
        if !self.armed {
            return;
        }
        let cur = self.latch();
        let delta = self.last.wrapping_sub(cur) as u64;
        self.counts += delta;
        self.last = cur;
    }

    pub fn elapsed_ms(&mut self) -> u64 {
        self.pump();
        self.counts / TICKS_PER_MS
    }
}
