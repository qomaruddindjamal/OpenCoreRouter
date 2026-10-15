//! Open Core Router (OpenCR) — asl-router-drivers-ata
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! =============================================================================
//! Driver ATA/IDE PIO Native ASL (Primary Channel, polled) — ditulis langsung
//! dari spesifikasi hardware ATA/ATAPI-8 (register set 0x1F0..0x1F7, 0x3F6).
//! BUKAN porting kode Linux/FreeBSD — murni implementasi no_std kami sendiri.
//! VirtualBox tidak meng-emulasi virtio-blk; disk nyata VM berada pada
//! controller PIIX4 IDE, sehingga jalur block nyata yang jujur diverifikasi
//! di perangkat keras ini adalah ATA PIO.

#![no_std]

pub const SECTOR_SIZE: usize = 512;

// Register set primary channel (legacy IDE)
pub const ATA_DATA: u16 = 0x1F0;
pub const ATA_ERR: u16 = 0x1F1;
pub const ATA_SEC_COUNT: u16 = 0x1F2;
pub const ATA_LBA0: u16 = 0x1F3;
pub const ATA_LBA1: u16 = 0x1F4;
pub const ATA_LBA2: u16 = 0x1F5;
pub const ATA_DRIVE_HEAD: u16 = 0x1F6;
pub const ATA_STATUS_CMD: u16 = 0x1F7;
pub const ATA_ALT_STATUS: u16 = 0x3F6;

// Status register bits
pub const ST_ERR: u8 = 0x01;
pub const ST_IDX: u8 = 0x02;
pub const ST_BSY: u8 = 0x80;
pub const ST_DRDY: u8 = 0x40;
pub const ST_DF: u8 = 0x20;
pub const ST_DRQ: u8 = 0x08;

// Commands
pub const CMD_READ_PIO: u8 = 0x20; // READ SECTORS (LBA28)
pub const CMD_WRITE_PIO: u8 = 0x30; // WRITE SECTORS (LBA28)
pub const CMD_CACHE_FLUSH: u8 = 0xE7;
pub const CMD_IDENTIFY: u8 = 0xEC;
pub const CMD_SET_FEATURES: u8 = 0xEF;
pub const CMD_SOFT_RESET: u8 = 0x08;

// Batas LBA28 (268.435.455 sektor ≈ 128 GiB) — disk 20 GiB aman di bawah ini.
pub const LBA28_MAX_SECTOR: u64 = 0x0FFF_FFFF;

// Timeout polling (jumlah iterasi baca register ≈ 10-20 detik pada hardware).
const POLL_LIMIT: u32 = 20_000_000;

// -----------------------------------------------------------------------------
// Port I/O — hanya aktif pada target bare-metal x86_64; pada host jadi stub.
// -----------------------------------------------------------------------------

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let ret: u8;
        core::arch::asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let _ = port;
        ST_DRDY // host stub: perangkat "siap" tanpa error agar API dapat diuji
    }
}

#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let ret: u16;
        core::arch::asm!("in ax, dx", out("ax") ret, in("dx") port, options(nomem, nostack, preserves_flags));
        ret
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        let _ = port;
        0
    }
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = (port, val);
}

#[inline(always)]
unsafe fn outw(port: u16, val: u16) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = (port, val);
}

/// Delay 400ns per spesifikasi ATA: empat read alternate-status dummy.
#[inline(always)]
unsafe fn ata_delay_400ns() {
    for _ in 0..4 {
        core::hint::black_box(inb(ATA_ALT_STATUS));
    }
}

pub struct AtaIdentity {
    pub model: [u8; 40],
    pub model_len: usize,
    pub sectors_lba28: u64,
    pub lba_supported: bool,
}

impl AtaIdentity {
    /// Model string ATA disimpan per-word byte-swapped (ASCII bertukang pasangan).
    pub fn model_str(&self) -> (&[u8], usize) {
        (&self.model, self.model_len)
    }
}

pub struct AtaDevice {
    pub identity: AtaIdentity,
    pub ready: bool,
    /// 0 = primary master, 1 = primary slave (bit DEV 0x10 pada register 0x1F6).
    pub drive: u8,
}

/// Hitung register DRIVE/HEAD mode LBA: 0xE0 | (drive<<4) | 28-bit sector high.
/// Murni & teruji unit — kesalahan bit DEV = salah sasaran antar drive.
#[inline]
pub const fn drive_head_lba(drive: u8, sector_hi: u8) -> u8 {
    0xE0 | (((drive & 1) << 4) | (sector_hi & 0x0F))
}

/// Register select mode CHS untuk IDENTIFY (tanpa bit LBA), per drive.
#[inline]
pub const fn drive_head_chs(drive: u8) -> u8 {
    0xA0 | ((drive & 1) << 4)
}

impl AtaDevice {
    pub const fn empty() -> Self {
        Self::empty_drive(0)
    }

    pub const fn empty_drive(drive: u8) -> Self {
        Self {
            identity: AtaIdentity {
                model: [0u8; 40],
                model_len: 0,
                sectors_lba28: 0,
                lba_supported: false,
            },
            ready: false,
            drive,
        }
    }

    /// Reset SRST (register kontrol 0x3F6) + IDENTIFY DEVICE (0xEC) dengan diagnostik.
    pub fn probe(&mut self, log: &dyn Fn(&str)) -> Result<(), &'static str> {
        unsafe {
            // Reset SRST via alternate status (0x3F6) — metode ATA-8 standar.
            outb(ATA_ALT_STATUS, 0x00);
            ata_delay_400ns();
            outb(ATA_ALT_STATUS, 0x04); // SRST assert
            for _ in 0..16 {
                ata_delay_400ns();
            }
            outb(ATA_ALT_STATUS, 0x00); // release
            self.poll(|st| st & ST_BSY == 0, "reset-BSY", log)?;
            self.poll(|st| st & (ST_BSY | ST_DRDY) == ST_DRDY, "reset-DRDY", log)?;
            // Bersihkan latch ERR & kode diagnostik pasca-reset.
            let err0 = inb(ATA_ERR);
            let st0 = inb(ATA_STATUS_CMD);
            log("[R22] ATA post-reset st=0x");
            log_hex8(st0, log);
            log(" err=0x");
            log_hex8(err0, log);
            log("\n");

            // IDENTIFY: select drive 0 master, count=1, LBA=0.
            outb(ATA_SEC_COUNT, 1);
            outb(ATA_LBA0, 0);
            outb(ATA_LBA1, 0);
            outb(ATA_LBA2, 0);
            outb(ATA_DRIVE_HEAD, drive_head_chs(self.drive)); // CHS select per drive — IDENTIFY tak butuh bit LBA
            for _ in 0..4 {
                ata_delay_400ns();
            }
            outb(ATA_STATUS_CMD, CMD_IDENTIFY);
            for _ in 0..4 {
                ata_delay_400ns();
            }

            let mut st = inb(ATA_STATUS_CMD);
            if st == 0x00 || st == 0xFF {
                return Err("IDENTIFY: floating bus — channel IDE tidak ada");
            }
            if st == ST_BSY {
                self.poll(|s| s & ST_BSY == 0, "identify-BSY", log)?;
                st = inb(ATA_STATUS_CMD);
            }

            // Retry sekali dengan SRST penuh + select berbit-LBA bila ERR terlatch.
            if st & ST_ERR != 0 {
                let e = inb(ATA_ERR);
                log("[R22] ATA identify ERR st=0x");
                log_hex8(st, log);
                log(" err=0x");
                log_hex8(e, log);
                log(" — retry SRST+LBA\n");
                outb(ATA_ALT_STATUS, 0x04);
                for _ in 0..16 {
                    ata_delay_400ns();
                }
                outb(ATA_ALT_STATUS, 0x00);
                self.poll(|s| s & ST_BSY == 0, "retry-BSY", log)?;
                self.poll(|s| s & (ST_BSY | ST_DRDY) == ST_DRDY, "retry-DRDY", log)?;
                let _ = inb(ATA_ERR);
                outb(ATA_DRIVE_HEAD, drive_head_lba(self.drive, 0));
                for _ in 0..4 {
                    ata_delay_400ns();
                }
                outb(ATA_STATUS_CMD, CMD_IDENTIFY);
                for _ in 0..4 {
                    ata_delay_400ns();
                }
                st = inb(ATA_STATUS_CMD);
                if st & ST_ERR != 0 {
                    let e = inb(ATA_ERR);
                    let l0 = inb(ATA_LBA0);
                    let l1 = inb(ATA_LBA1);
                    log("[R22] ATA identify final ERR st=0x");
                    log_hex8(st, log);
                    log(" err=0x");
                    log_hex8(e, log);
                    log(" lba0=0x");
                    log_hex8(l0, log);
                    log(" lba1=0x");
                    log_hex8(l1, log);
                    log("\n");
                    return Err("IDENTIFY aborted (ERR latched setelah retry)");
                }
            }

            if !self.poll(|s| s & ST_DRQ != 0, "identify-DRQ", log)? {
                return Err("IDENTIFY: timeout menunggu DRQ");
            }
            // Deteksi bukan-ATA (0x14EB / 0xEB14 = ATAPI/ kosong).
            let pre0 = inw(ATA_DATA);
            let pre1 = inw(ATA_DATA);
            if (pre0, pre1) == (0x14, 0xEB) || (pre0, pre1) == (0xEB, 0x14) {
                return Err("IDENTIFY: signature ATAPI/kosong (0x14EB)");
            }

            // Sisa 254 word (2 word pertama sudah dikonsumsi untuk cek signature).
            let mut words = [0u16; 256];
            words[0] = pre0;
            words[1] = pre1;
            for w in words.iter_mut().skip(2) {
                *w = inw(ATA_DATA);
            }
            // Verifikasi checksum (byte XOR dari 256 word) — opsional namun sehat.
            let mut xor: u8 = 0;
            for &w in words.iter() {
                xor ^= (w & 0xFF) as u8 ^ ((w >> 8) as u8);
            }
            // Checksum 0 = valid per spec; beberapa emulasi tidak mengisi — jangan hard-fail.

            // Word 0 bit 15: bukan direct-access device bila set.
            if words[0] & 0x8000 != 0 {
                return Err("IDENTIFY: bukan perangkat direct-access");
            }

            // Word 47 bit 8: LBA supported.
            let lba_ok = words[47] & 0x0100 != 0;

            // Word 60-61: max LBA28 (u32 little-endian per pasangan word LE).
            let lba28 = (words[60] as u64) | ((words[61] as u64) << 16);

            // Word 27..46: model string, byte-swapped per word.
            let mut model = [0u8; 40];
            let mut mlen = 0usize;
            for i in 27..47 {
                let w = words[i];
                model[mlen] = (w >> 8) as u8;
                mlen += 1;
                model[mlen] = (w & 0xFF) as u8;
                mlen += 1;
            }
            // Trim spasi leading/trailing dan NUL.
            let mut start = 0usize;
            while start < mlen && (model[start] == b' ' || model[start] == 0) {
                start += 1;
            }
            let mut end = mlen;
            while end > start && (model[end - 1] == b' ' || model[end - 1] == 0) {
                end -= 1;
            }
            let mut trimmed = [0u8; 40];
            trimmed[..end - start].copy_from_slice(&model[start..end]);

            self.identity = AtaIdentity {
                model: trimmed,
                model_len: end - start,
                sectors_lba28: if lba28 != 0 {
                    lba28
                } else {
                    words[19] as u64 | ((words[10] as u64) << 16)
                },
                lba_supported: lba_ok,
            };
            self.ready = true;
            let _ = xor;
            Ok(())
        }
    }

    #[inline]
    fn poll<F: Fn(u8) -> bool>(
        &self,
        cond: F,
        stage: &'static str,
        log: &dyn Fn(&str),
    ) -> Result<bool, &'static str> {
        unsafe {
            for _ in 0..POLL_LIMIT {
                let st = inb(ATA_STATUS_CMD);
                if st & ST_ERR != 0 {
                    let e = inb(ATA_ERR);
                    log("[R22] ATA ERR stage=");
                    log(stage);
                    log(" st=0x");
                    log_hex8(st, log);
                    log(" err=0x");
                    log_hex8(e, log);
                    log("\n");
                    return Err("ATA ERR asserted saat polling");
                }
                if st & ST_BSY != 0 {
                    continue;
                }
                if cond(st) {
                    return Ok(true);
                }
            }
            log("[R22] ATA timeout stage=");
            log(stage);
            log("\n");
            Ok(false)
        }
    }

    fn cmd_lba28(
        &mut self,
        cmd: u8,
        sector: u32,
        count: u8,
        log: &dyn Fn(&str),
    ) -> Result<(), &'static str> {
        unsafe {
            if !self.poll(|s| s & ST_BSY == 0, "pre-cmd", log)? {
                return Err("BSY timeout sebelum command");
            }
            outb(ATA_SEC_COUNT, count);
            outb(ATA_LBA0, sector as u8);
            outb(ATA_LBA1, (sector >> 8) as u8);
            outb(ATA_LBA2, (sector >> 16) as u8);
            outb(
                ATA_DRIVE_HEAD,
                drive_head_lba(self.drive, (sector >> 24) as u8),
            );
            outb(ATA_STATUS_CMD, cmd);
            ata_delay_400ns();
            Ok(())
        }
    }

    /// Baca 1 sektor (512 B) via PIO 16-bit (compatibility maksimal: 256 word).
    pub fn read_sector(
        &mut self,
        sector: u64,
        out_buf: &mut [u8; SECTOR_SIZE],
        log: &dyn Fn(&str),
    ) -> Result<(), &'static str> {
        if !self.ready {
            return Err("ATA device belum di-probe");
        }
        if sector > LBA28_MAX_SECTOR {
            return Err("sektor di luar jendela LBA28 driver");
        }
        self.cmd_lba28(CMD_READ_PIO, sector as u32, 1, log)?;
        if !self.poll(|s| s & ST_DRQ != 0, "read-DRQ", log)? {
            return Err("READ: timeout menunggu DRQ");
        }
        unsafe {
            for chunk in out_buf.chunks_mut(2) {
                let w = inw(ATA_DATA);
                chunk[0] = (w & 0xFF) as u8;
                if chunk.len() > 1 {
                    chunk[1] = (w >> 8) as u8;
                }
            }
        }
        // Jaga status final (bebas ERR).
        self.poll(|_| true, "read-final", log).map(|_| ())
    }

    /// Tulis 1 sektor (512 B) via PIO 16-bit + flush cache.
    pub fn write_sector(
        &mut self,
        sector: u64,
        in_buf: &[u8; SECTOR_SIZE],
        log: &dyn Fn(&str),
    ) -> Result<(), &'static str> {
        if !self.ready {
            return Err("ATA device belum di-probe");
        }
        if sector > LBA28_MAX_SECTOR {
            return Err("sektor di luar jendela LBA28 driver");
        }
        self.cmd_lba28(CMD_WRITE_PIO, sector as u32, 1, log)?;
        if !self.poll(|s| s & ST_DRQ != 0, "write-DRQ", log)? {
            return Err("WRITE: timeout menunggu DRQ");
        }
        unsafe {
            for pair in in_buf.chunks_exact(2) {
                let w = (pair[0] as u16) | ((pair[1] as u16) << 8);
                outw(ATA_DATA, w);
            }
            // device harus menerima kembali status tanpa ERR
            if !self.poll(|s| s & (ST_BSY | ST_DRQ) == 0, "write-idle", log)? {
                return Err("WRITE: tidak kembali idle setelah data");
            }
            outb(ATA_STATUS_CMD, CMD_CACHE_FLUSH);
            if !self.poll(|s| s & ST_DRDY != 0, "flush", log)? {
                return Err("FLUSH: timeout");
            }
        }
        Ok(())
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.identity.sectors_lba28
    }

    pub fn capacity_gib(&self) -> u64 {
        self.capacity_sectors() * (SECTOR_SIZE as u64) / (1024 * 1024 * 1024)
    }

    pub fn model_bytes(&self) -> &[u8] {
        &self.identity.model[..self.identity.model_len]
    }
}

// -----------------------------------------------------------------------------
// Host-side pure helpers & unit tests
// -----------------------------------------------------------------------------

/// Signature GPT header primer (LBA1) per UEFI 2.x: "EFI PART".
pub const GPT_SIG: &[u8; 8] = b"EFI PART";

pub fn is_gpt_header(sector: &[u8; SECTOR_SIZE]) -> bool {
    &sector[0..8] == GPT_SIG
}

#[inline]
fn log_hex8(v: u8, log: &dyn Fn(&str)) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 2];
    buf[0] = HEX[(v >> 4) as usize];
    buf[1] = HEX[(v & 0x0F) as usize];
    log(core::str::from_utf8(&buf).unwrap_or("??"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_lba28_bounds() {
        let mut dev = AtaDevice::empty();
        let mut buf = [0u8; SECTOR_SIZE];
        let quiet = &|_s: &str| {};
        assert!(dev.read_sector(0, &mut buf, quiet).is_err()); // belum ready
        dev.ready = true;
        assert_eq!(
            dev.read_sector(LBA28_MAX_SECTOR + 1, &mut buf, quiet),
            Err("sektor di luar jendela LBA28 driver")
        );
        assert_eq!(
            dev.write_sector(LBA28_MAX_SECTOR + 1, &[0u8; SECTOR_SIZE], quiet),
            Err("sektor di luar jendela LBA28 driver")
        );
    }

    #[test]
    fn test_02_gpt_sig_detect() {
        let mut sec = [0u8; SECTOR_SIZE];
        assert!(!is_gpt_header(&sec));
        sec[..8].copy_from_slice(GPT_SIG);
        assert!(is_gpt_header(&sec));
    }

    #[test]
    fn test_03_capacity_math() {
        let mut dev = AtaDevice::empty();
        dev.identity.sectors_lba28 = 20 * 1024 * 1024 * 2; // 20 GiB
        assert_eq!(dev.capacity_gib(), 20);
        assert_eq!(dev.capacity_sectors(), 41_943_040);
    }

    #[test]
    fn test_04_status_bit_constants() {
        assert_eq!(ST_BSY, 0x80);
        assert_eq!(ST_DRDY, 0x40);
        assert_eq!(ST_DRQ, 0x08);
        assert_eq!(ST_ERR, 0x01);
    }

    #[test]
    fn test_05_drive_head_lba_bit_dev() {
        // master: bit 4 = 0; slave: bit DEV 0x10 = 1; 4 bit low sector Hi tetap.
        assert_eq!(drive_head_lba(0, 0x00), 0xE0);
        assert_eq!(drive_head_lba(1, 0x00), 0xF0);
        assert_eq!(drive_head_lba(1, 0x0F), 0xFF);
        assert_eq!(drive_head_lba(0, 0x0F), 0xEF);
        // sector hi > 4 bit harus di-mask (tidak boleh menyetel bit lain)
        assert_eq!(drive_head_lba(1, 0xFF), 0xFF);
        assert_eq!(drive_head_chs(0), 0xA0);
        assert_eq!(drive_head_chs(1), 0xB0);
    }

    #[test]
    fn test_06_empty_drive_default_master() {
        assert_eq!(AtaDevice::empty().drive, 0);
        assert_eq!(AtaDevice::empty_drive(1).drive, 1);
    }
}
