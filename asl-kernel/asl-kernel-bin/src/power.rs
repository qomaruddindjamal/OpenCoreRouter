//! ASL Power Control — ACPI PM1_CNT Power-Off & Triple-Fault Reboot
//! =================================================================
//! Bare-metal (x86_64, ring-0). Kandidat port PM1a_CNT dicoba berurutan;
//! bila salah satu diterima chipsets, VM mati sebelum kandidat berikutnya
//! sehingga baris log TERAKHIR mengidentifikasi port pemenang (evidence-first).

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn outw(port: u16, val: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn outw(_port: u16, _val: u16) {}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn outl(_port: u16, _val: u32) {}

use core::sync::atomic::{AtomicU64, Ordering};

/// Offset HHDM sebagaimana dijawab bootloader Limine (request resmi di main.rs).
/// 0 = belum tersedia (request gagal / bootloader tidak mendukung).
static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

pub fn set_hhdm_offset(offset: u64) {
    HHDM_OFFSET.store(offset, Ordering::Relaxed);
}

pub fn hhdm_offset() -> Option<u64> {
    let v = HHDM_OFFSET.load(Ordering::Relaxed);
    if v == 0 {
        None
    } else {
        Some(v)
    }
}

#[inline(always)]
fn hh<T>(phys: usize) -> *const T {
    match hhdm_offset() {
        Some(off) => (off.wrapping_add(phys as u64)) as *const T,
        None => core::ptr::null(),
    }
}

#[inline(always)]
fn settle_spin() {
    for _ in 0..2_000_000u32 {
        core::hint::spin_loop();
    }
}

// -----------------------------------------------------------------------------
// AKSI: ACPI FADT DISCOVERY (RSD PTR -> XSDT/RSDT -> FACP -> PM1a_CNT_BLK GAS)
// -----------------------------------------------------------------------------

fn rsdp_signature_at(p: *const u8) -> bool {
    if p.is_null() {
        return false;
    }
    let s = unsafe { core::slice::from_raw_parts(p, 8) };
    if s != b"RSD PTR " {
        return false;
    }
    // Validasi checksum ACPI 1.0 (20 byte pertama) untuk menolak false positive.
    let sum: u8 = unsafe { core::slice::from_raw_parts(p, 20) }
        .iter()
        .fold(0u8, |a, b| a.wrapping_add(*b));
    sum == 0
}

/// Cari RSDP di area BIOS 0xE0000..0xFFFFF (16-byte scan) lalu EBDA.
/// Return VA hasil HHDM, bukan PA.
pub fn find_rsdp_va() -> Option<*const u8> {
    if hhdm_offset().is_none() {
        return None;
    }
    let mut off = 0x000E_0000usize;
    while off < 0x0010_0000 {
        let p = hh::<u8>(off);
        if rsdp_signature_at(p) {
            return Some(p);
        }
        off += 16;
    }
    // EBDA: pointer u16 di PA 0x40E, segment << 4, scan 1 KiB
    let ebda_seg = unsafe { *hh::<u16>(0x40E) } as usize;
    let ebda_pa = ebda_seg * 16;
    if ebda_pa != 0 && ebda_pa < 0x0010_0000 {
        let mut o = 0usize;
        while o < 1024 {
            let p = hh::<u8>(ebda_pa + o);
            if rsdp_signature_at(p) {
                return Some(p);
            }
            o += 16;
        }
    }
    None
}

pub struct Pm1CntInfo {
    pub port: u16,
    pub width: u8,
}

/// Parse RSDP -> XSDT (atau RSDT) -> FACP -> GAS PM1a_CNT_BLK (offset FACP+60).
pub fn locate_pm1_cnt(log: &dyn Fn(&str)) -> Option<Pm1CntInfo> {
    let rsdp = find_rsdp_va()?;
    log("[R25] RSDP ditemukan via HHDM.\n");

    // Layout RSDP resmi (ACPI spec):
    //   0..8 signature | 8 checksum | 9..15 OEMID | 15 revision
    //   16..20 RsdtAddress (u32 LE) | 20..24 length (u32) | 24..32 XsdtAddress (u64)
    let rev = unsafe { *rsdp.add(15) };
    let rsdt_pa = unsafe { (rsdp as *const u32).add(4).read_unaligned() }; // offset 16
    let rsdp_len = unsafe { (rsdp as *const u32).add(5).read_unaligned() }; // offset 20
    let xsdt_raw = if rev >= 2 && rsdp_len >= 32 {
        unsafe { (rsdp as *const u64).add(3).read_unaligned() } // offset 24
    } else {
        0
    };
    log("[R25] RSDP rev=");
    log_dec(rev as u32, log);
    log(" len=");
    log_dec(rsdp_len, log);
    log(" XSDT_pa=0x");
    log_hex64(xsdt_raw, log);
    log(" RSDT_pa=0x");
    log_hex64(rsdt_pa as u64, log);
    log("\n");
    let mut table_pa: usize = 0;
    let mut prefer_xsdt = false;
    if xsdt_raw != 0 && xsdt_raw < 0x40_0000_0000 {
        table_pa = xsdt_raw as usize;
        prefer_xsdt = true;
    }
    if table_pa == 0 && rsdt_pa != 0 && (rsdt_pa as u64) < 0x40_0000_0000 {
        table_pa = rsdt_pa as usize;
    }
    if table_pa == 0 {
        log("[R25] XSDT/RSDT pointer tidak valid.\n");
        return None;
    }

    let hdr = hh::<u8>(table_pa);
    let sig_xsdt = unsafe { core::slice::from_raw_parts(hdr, 4) == *b"XSDT" };
    let sig_rsdt = unsafe { core::slice::from_raw_parts(hdr, 4) == *b"RSDT" };
    if !(sig_xsdt || sig_rsdt) {
        log("[R25] Root table signature bukan XSDT/RSDT — lewat.\n");
        // fallback ke pointer lain bila perlu
        if prefer_xsdt && (rsdt_pa as u64) != 0 && (rsdt_pa as u64) < 0x40_0000_0000 {
            table_pa = rsdt_pa as usize;
            let hdr2 = hh::<u8>(table_pa);
            let ok2 = unsafe {
                core::slice::from_raw_parts(hdr2, 4) == *b"XSDT"
                    || core::slice::from_raw_parts(hdr2, 4) == *b"RSDT"
            };
            if !ok2 {
                log("[R25] RSDT fallback juga tidak valid.\n");
                return None;
            }
        } else {
            return None;
        }
    }
    let is_xsdt = unsafe { core::slice::from_raw_parts(hh::<u8>(table_pa), 4) == *b"XSDT" };
    let tbl_len = unsafe { hh::<u32>(table_pa + 4).read_unaligned() } as usize;
    let entry_size = if is_xsdt { 8 } else { 4 };
    let entry_count = (tbl_len.saturating_sub(36)) / entry_size;
    if entry_count == 0 || entry_count > 128 {
        log("[R25] Root table count tidak valid.\n");
        return None;
    }
    log("[R25] Root table = ");
    log(if is_xsdt { "XSDT" } else { "RSDT" });
    log(" entries=");
    log_dec(entry_count as u32, log);
    log("\n");

    for i in 0..entry_count {
        let ent_pa = if is_xsdt {
            (unsafe { hh::<u64>(table_pa + 36 + i * 8).read_unaligned() }) as usize
        } else {
            (unsafe { hh::<u32>(table_pa + 36 + i * 4).read_unaligned() }) as usize
        };
        if ent_pa == 0 || ent_pa > 0x40_0000_0000 {
            log("[R25] entri ");
            log_dec(i as u32, log);
            log(" pa tidak valid, skip\n");
            continue;
        }
        let sig = unsafe { core::slice::from_raw_parts(hh::<u8>(ent_pa), 4) };
        log("[R25] entri ");
        log_dec(i as u32, log);
        log(" pa=0x");
        log_hex64(ent_pa as u64, log);
        log(" sig=");
        log(core::str::from_utf8(sig).unwrap_or("????"));
        log("\n");
        if sig == *b"FACP" {
            let space_id = unsafe { *hh::<u8>(ent_pa + 60) };
            let width = unsafe { *hh::<u8>(ent_pa + 61) };
            let addr = unsafe { hh::<u64>(ent_pa + 64).read_unaligned() };
            log("[R25] FACP: PM1a_CNT space_id=");
            log_dec(space_id as u32, log);
            log(" width=");
            log_dec(width as u32, log);
            log(" addr=0x");
            log_hex64(addr, log);
            log("\n");
            // Quirk VirtualBox: FADT ACPI 1.0 diisi space_id=0/width=0 tetapi
            // alamat GAS adalah port IO (0x4004). Terima bila addr <= 0xFFFF.
            if addr > 0xFFFF {
                log("[R25] PM1a_CNT di luar IO16 space.\n");
                return None;
            }
            return Some(Pm1CntInfo {
                port: addr as u16,
                width,
            });
        }
    }
    log("[R25] FACP tidak ditemukan pada root table.\n");
    None
}

/// SLP_EN (bit 13) | SLP_TYPa S5 (7 << 10) = 0x3C00 per ACPI spec.
pub const S5_VAL: u16 = 0x3C00;

pub struct Pcand {
    pub name: &'static str,
    pub port: u16,
    pub val: u16,
}

pub const CANDIDATES: [Pcand; 6] = [
    Pcand {
        name: "PIIX4 base 0x4000 PM1_CNT=0x4002",
        port: 0x4002,
        val: S5_VAL,
    },
    Pcand {
        name: "ICH PMBASE 0x4000 PM1_CNT=0x4004",
        port: 0x4004,
        val: S5_VAL,
    },
    Pcand {
        name: "ICH9 PMBASE 0xB000 PM1_CNT=0xB002",
        port: 0xB002,
        val: S5_VAL,
    },
    Pcand {
        name: "ICH9 PMBASE 0xB000 PM1_CNT=0xB004",
        port: 0xB004,
        val: S5_VAL,
    },
    Pcand {
        name: "0x4002 SLP_EN only",
        port: 0x4002,
        val: 0x2000,
    },
    Pcand {
        name: "0x4002 S5 legacy 0x2007",
        port: 0x4002,
        val: 0x2007,
    },
];

/// Coba seluruh kandidat power-off. Tidak pernah return bila sukses (VM mati).
/// Return false hanya bila semua kandidat ditolak hardware.
pub fn try_poweroff_candidates(log: &dyn Fn(&str)) -> bool {
    if let Some(o) = hhdm_offset() {
        log("[R25] HHDM offset resmi bootloader = 0x");
        log_hex64(o, log);
        log("\n");
    } else {
        log("[R25] HHDM tidak tersedia dari bootloader — ACPI scan dilewati.\n");
    }
    // Jalur 1 (TERVERIFIKASI VirtualBox, probe7/7b): port resmi FADT PM1a_CNT,
    // SLP_TYP 0..7 via outw 16-bit. Pemenang VBox: typ=1 -> outw(0x4004,0x2400).
    // Baris log TERAKHIR sebelum VM mati = kombinasi pemenang (attribution).
    if let Some(pm1) = locate_pm1_cnt(log) {
        for typ in 0..=7u16 {
            let val = 0x2000u16 | (typ << 10);
            log("[R25] W outw 0x");
            log_hex(pm1.port, log);
            log(" = 0x");
            log_hex16(val, log);
            log("\n");
            unsafe { outw(pm1.port, val) };
            settle_spin();
        }
        for typ in 0..=7u16 {
            let val = 0x2000u32 | ((typ as u32) << 10);
            log("[R25] W outl 0x");
            log_hex(pm1.port, log);
            log(" = 0x");
            log_hex64(val as u64, log);
            log("\n");
            unsafe { outl(pm1.port, val) };
            settle_spin();
        }
        log("[R25] Semua SLP_TYP pada port FADT ditolak — VM masih hidup.\n");
    } else {
        log("[R25] ACPI discovery gagal — fallback kandidat statis.\n");
        for c in CANDIDATES.iter() {
            log("[R25] PM1_CNT try: port=0x");
            log_hex(c.port, log);
            log(" val=0x");
            log_hex16(c.val, log);
            log(" (");
            log(c.name);
            log(")\n");
            unsafe { outw(c.port, c.val) };
            settle_spin();
            log("[R25]   -> still alive, candidate rejected\n");
        }
    }
    false
}

#[inline(always)]
fn log_hex(v: u16, log: &dyn Fn(&str)) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 4];
    for i in 0..4 {
        buf[3 - i] = HEX[((v >> (4 * i)) & 0xF) as usize];
    }
    log(core::str::from_utf8(&buf).unwrap_or("????"));
}

#[inline(always)]
fn log_hex16(v: u16, log: &dyn Fn(&str)) {
    log_hex(v, log);
}

fn log_hex64(v: u64, log: &dyn Fn(&str)) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 16];
    for i in 0..16 {
        buf[15 - i] = HEX[((v >> (4 * i)) & 0xF) as usize];
    }
    log(core::str::from_utf8(&buf).unwrap_or("????????????????"));
}

fn log_dec(v: u32, log: &dyn Fn(&str)) {
    if v == 0 {
        log("0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let mut out = [0u8; 10];
    for j in 0..i {
        out[j] = buf[i - 1 - j];
    }
    log(core::str::from_utf8(&out[..i]).unwrap_or("?"));
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

/// Reboot via triple fault: load null IDT lalu picu int3 -> #GP -> #DF -> shutdown.
/// Berjalan di VirtualBox & QEMU tanpa dukungan ACPI S5.
#[cfg(target_arch = "x86_64")]
pub fn triple_fault_reboot() -> ! {
    let idtr = Idtr { limit: 0, base: 0 };
    unsafe {
        core::arch::asm!("cli");
        core::arch::asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));
        core::arch::asm!("int3", options(nomem, nostack));
    }
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn triple_fault_reboot() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
