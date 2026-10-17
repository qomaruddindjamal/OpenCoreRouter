//! Open Core Router (OpenCR) — asl-router-installer
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! =============================================================================
//! In-Kernel Self-Installer native (OPCR-76 / Sprint IS-03).
//!
//! Model: salin sektor-per-sektor dari disk sumber (disk tempat kernel ini
//! di-boot) ke disk target kosong pada channel IDE yang sama. Pipeline 9 tahap
//! eksplisit: PREFLIGHT → TARGET_IDENTIFICATION → TARGET_CONFIRMATION →
//! PARTITION_PLAN → PARTITION_WRITE → BOOT_ARTIFACT_INSTALL →
//! CONFIG_INITIALIZATION → READBACK_VERIFICATION → INSTALL_COMPLETE.
//!
//! Proteksi wajib: kapasitas src==tgt, sumber harus GPT valid (CRC header +
//! CRC array entri), target harus blank (LBA1 bukan "EFI PART" + sampling
//! sektor nol), konfirmasi token eksplisit sebelum sektor pertama ditulis,
//! aritmetika checked (overflow), rentang partisi divalidasi (overlap/luar
//! usable), tiap sektor yang ditulis dibaca-balik byte-per-byte, digest
//! SHA-256 jalur tulis vs jalur verifikasi harus identik, dan jurnal RJCFG01
//! target diinisialisasi baru (seq=1 Slot A) tanpa pernah menyentuh jurnal
//! sumber.
//!
//! BUKAN porting kode Linux/FreeBSD — murni spesifikasi publik (UEFI 2.10 GPT,
//! FAT 1.03 BPB, FIPS 180-4 SHA-256) yang diimplementasikan sendiri `no_std`.

#![no_std]

pub use asl_router_gpt::SECTOR_SIZE;

/// Token konfirmasi yang HARUS disuplai operator sebelum satu sektor pun ditulis.
pub const CONFIRM_TOKEN: &str = "YES-INSTALL";

/// Sektor cadangan GPT di ekor disk: 32 sektor array + 1 header.
pub const BACKUP_SECTORS: u64 = 33;
/// Region kepala disk yang disalin verbatim: MBR(1) + header(1) + array(32).
pub const HEAD_SECTORS: u64 = 34;
/// Area RJCFG01 yang diinisialisasi di target: header(1) + reserved(7) +
/// Slot A(8) + Slot B(8) = 24 sektor, cocok dgn konstanta on-disk persistence.
pub const RJCFG_INIT_SECTORS: u64 = 24;
/// Slot A mulai di sektor partisi-relative 8 (RJCFG_SLOT_A_FIRST_SECTOR).
pub const RJCFG_SLOT_A_OFFSET: u64 = 8;
/// Slot B mulai di sektor partisi-relative 16 (RJCFG_SLOT_B_FIRST_SECTOR).
pub const RJCFG_SLOT_B_OFFSET: u64 = 16;
/// Jumlah sektor per slot jurnal (4096 B / 512 B).
pub const RJCFG_SLOT_SECTORS: u64 = 8;
/// Kapasitas minimum disk yang masuk akal untuk layout Rinjani.
pub const MIN_CAP_SECTORS: u64 = 1_048_576; // 512 MiB

/// Ukuran slot jurnal on-disk dalam byte (sinkron dgn asl-router-persistence).
const SLOT_BYTES: usize = 4096;

// -----------------------------------------------------------------------------
// Abstraksi block device
// -----------------------------------------------------------------------------

/// Akses block minimal yang dibutuhkan installer. Implementasi produksi:
/// adapter `AtaBlockIo` di biner kernel; implementasi uji: `MockDisk` sparse.
pub trait BlockIo {
    fn capacity_sectors(&self) -> u64;
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str>;
    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str>;
}

// -----------------------------------------------------------------------------
// Kesalahan bertahap (stage-tagged, dapat ditelusuri di log)
// -----------------------------------------------------------------------------

pub const STAGE_PREFLIGHT: &str = "PREFLIGHT";
pub const STAGE_TARGET_ID: &str = "TARGET_IDENTIFICATION";
pub const STAGE_CONFIRM: &str = "TARGET_CONFIRMATION";
pub const STAGE_PLAN: &str = "PARTITION_PLAN";
pub const STAGE_PART_WRITE: &str = "PARTITION_WRITE";
pub const STAGE_ARTIFACT: &str = "BOOT_ARTIFACT_INSTALL";
pub const STAGE_CONFIG: &str = "CONFIG_INITIALIZATION";
pub const STAGE_READBACK: &str = "READBACK_VERIFICATION";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallFailure {
    pub stage: &'static str,
    pub reason: &'static str,
}

impl InstallFailure {
    fn new(stage: &'static str, reason: &'static str) -> Self {
        Self { stage, reason }
    }
}

// -----------------------------------------------------------------------------
// Rencana & laporan instalasi
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallPlan {
    pub total_sectors: u64,
    pub esp_start: u64,
    pub esp_end: u64,
    /// Sektor ESP yang benar-benar terpakai (boot sector + FSInfo + backup +
    /// 2×FAT + cluster alokasi terakhir, hasil scan FAT0) — batas salin.
    pub esp_used_sectors: u64,
    pub cfg_start: u64,
    pub cfg_end: u64,
    pub data_start: u64,
    pub data_end: u64,
    pub entry_lba: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallReport {
    pub plan: InstallPlan,
    pub sectors_written: u64,
    pub esp_sectors: u64,
    pub sha_written: [u8; 32],
    pub sha_readback: [u8; 32],
}

// -----------------------------------------------------------------------------
// Geometri FAT32 ESP (BPB) — parsing untuk menentukan batas salin
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EspGeometry {
    pub bytes_per_sector: u64,
    pub sectors_per_cluster: u64,
    pub reserved_sectors: u64,
    pub fat_count: u64,
    pub fat_len: u64,
    pub root_cluster: u64,
    pub data_sectors: u64,
    /// Offset sektor dalam ESP ke awal region data (cluster 2).
    pub data_start: u64,
}

/// Parse BPB FAT32 dari sektor boot ESP. Hanya menerima geometri yang
/// aritmetikanya tidak overflow dan konsisten dengan ukuran partisi.
pub fn parse_fat32_bpb(
    boot: &[u8; SECTOR_SIZE],
    esp_total_sectors: u64,
) -> Result<EspGeometry, &'static str> {
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return Err("BPB: signature boot sector 0x55AA hilang");
    }
    if &boot[82..87] != b"FAT32" {
        return Err("BPB: bukan volume FAT32 (fstyp)");
    }
    let bps = u16::from_le_bytes([boot[11], boot[12]]) as u64;
    if bps != SECTOR_SIZE as u64 {
        return Err("BPB: bytes_per_sector != 512");
    }
    let spc = boot[13] as u64;
    if spc == 0 || !spc.is_power_of_two() || spc > 128 {
        return Err("BPB: sectors_per_cluster tidak valid");
    }
    let reserved = u16::from_le_bytes([boot[14], boot[15]]) as u64;
    if reserved < 1 {
        return Err("BPB: reserved_sector_count nol");
    }
    let fats = boot[16] as u64;
    if fats < 1 || fats > 4 {
        return Err("BPB: jumlah FAT tidak valid");
    }
    // Field FAT12/FAT16 wajib nol pada volume FAT32 (Microsoft FAT 1.03).
    if u16::from_le_bytes([boot[17], boot[18]]) != 0 {
        return Err("BPB: root entry count harus 0 pada FAT32");
    }
    if u16::from_le_bytes([boot[19], boot[20]]) != 0 {
        return Err("BPB: total sektor 16 harus 0 pada FAT32");
    }
    if u16::from_le_bytes([boot[22], boot[23]]) != 0 {
        return Err("BPB: fat16 length harus 0 pada FAT32");
    }
    let fat_len = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]) as u64;
    if fat_len == 0 {
        return Err("BPB: fat32 length nol");
    }
    // Total sektor 32-bit harus konsisten dgn ukuran partisi tempat ESP berada.
    let tot32 = u32::from_le_bytes([boot[32], boot[33], boot[34], boot[35]]) as u64;
    if tot32 != 0 && tot32 != esp_total_sectors {
        return Err("BPB: total sektor volume != ukuran partisi ESP");
    }
    let root_cluster = u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]]) as u64;
    let meta = reserved
        .checked_add(
            fats.checked_mul(fat_len)
                .ok_or("BPB: overflow meta region")?,
        )
        .ok_or("BPB: overflow meta region")?;
    if meta >= esp_total_sectors {
        return Err("BPB: region metadata melebihi ukuran partisi");
    }
    let data_sectors = esp_total_sectors - meta;
    if data_sectors > (1u64 << 40) || spc > (1u64 << 20) {
        return Err("BPB: geometri terlalu besar");
    }
    Ok(EspGeometry {
        bytes_per_sector: bps,
        sectors_per_cluster: spc,
        reserved_sectors: reserved,
        fat_count: fats,
        fat_len,
        root_cluster,
        data_sectors,
        data_start: meta,
    })
}

/// Scan FAT0 sumber: cluster alokasi tertinggi → batas salin ESP (sektor dari
/// awal ESP). Hasil minimal adalah region metadata (reserved + fats×fat_len).
pub fn esp_used_sectors<S: BlockIo + ?Sized>(
    src: &mut S,
    esp_start: u64,
    geo: &EspGeometry,
) -> Result<u64, &'static str> {
    let mut max_idx: Option<u64> = None;
    let entries_per_sector = (SECTOR_SIZE / 4) as u64; // 128
    let fat_base = esp_start
        .checked_add(geo.reserved_sectors)
        .ok_or("FAT: overflow posisi FAT0")?;
    let mut sec = [0u8; SECTOR_SIZE];
    for fs in 0..geo.fat_len {
        src.read_sector(fat_base + fs, &mut sec)?;
        for i in 0..entries_per_sector {
            let gidx = fs * entries_per_sector + i;
            // Cluster 0/1 tidak dipakai; cluster ke-N (N = gidx-2).
            if gidx >= 2 && gidx - 2 >= geo.cluster_count() {
                break;
            }
            let off = (i as usize) * 4;
            let raw = u32::from_le_bytes([sec[off], sec[off + 1], sec[off + 2], sec[off + 3]]);
            let v = raw & 0x0F_FFF_FFF;
            if v != 0 {
                max_idx = Some(gidx.max(max_idx.unwrap_or(0)));
            }
        }
    }
    let meta = geo.data_start;
    let used = match max_idx {
        Some(g) => {
            // gidx tertinggi = g → cluster offset (g-2) → span (g-1)×spc sektor.
            let cluster_sectors = g
                .checked_sub(1)
                .ok_or("FAT: indeks cluster tidak valid")?
                .checked_mul(geo.sectors_per_cluster)
                .ok_or("FAT: overflow span cluster")?;
            meta.checked_add(cluster_sectors)
                .ok_or("FAT: overflow span ESP")?
        }
        None => meta,
    };
    if used > meta + geo.data_sectors {
        return Err("FAT: span terpakai melebihi partisi");
    }
    Ok(used)
}

impl EspGeometry {
    /// Jumlah cluster data yang tersedia pada volume.
    pub const fn cluster_count(&self) -> u64 {
        self.data_sectors / self.sectors_per_cluster
    }
}

// -----------------------------------------------------------------------------
// SHA-256 (FIPS 180-4) kini tinggal di crate primitif asl-router-crypto; dipajang
// kembali di sini agar jalur tulis/verifikasi ESP memakai implementasi yang sama.
pub use asl_router_crypto::sha256;

// -----------------------------------------------------------------------------
// Helper log numerik (tanpa alloc)
// -----------------------------------------------------------------------------

fn dec_bytes(v: u64, buf: &mut [u8; 24]) -> &str {
    let mut n = v;
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}

fn log_u64(log: &dyn Fn(&str), v: u64) {
    let mut buf = [0u8; 24];
    log(dec_bytes(v, &mut buf));
}

fn log_hex_bytes(log: &dyn Fn(&str), bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut pair = [0u8; 2];
    for b in bytes {
        pair[0] = HEX[(b >> 4) as usize];
        pair[1] = HEX[(b & 0x0F) as usize];
        log(core::str::from_utf8(&pair).unwrap_or("??"));
    }
}

fn is_zero_sector(sec: &[u8; SECTOR_SIZE]) -> bool {
    sec.iter().all(|&b| b == 0)
}

fn read_le_u32(sec: &[u8; SECTOR_SIZE], off: usize) -> u32 {
    u32::from_le_bytes([sec[off], sec[off + 1], sec[off + 2], sec[off + 3]])
}

fn read_le_u64(sec: &[u8; SECTOR_SIZE], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&sec[off..off + 8]);
    u64::from_le_bytes(b)
}

/// CRC32 atas array entri GPT secara streaming, tanpa buffer 16 KiB. Panjang
/// region = num_entries * entry_size (bukan selalu 128 entri — disk bentukan
/// Limine mendeklarasikan 44 entri), dibatasi jendela 32 sektor.
fn stream_array_crc<B: BlockIo + ?Sized>(
    dev: &mut B,
    entry_lba: u64,
    num_entries: u32,
    entry_size: u32,
) -> Result<u32, &'static str> {
    let total = (num_entries as u64)
        .checked_mul(entry_size as u64)
        .ok_or("GPT: overflow ukuran array entri")?;
    let sectors = (total + SECTOR_SIZE as u64 - 1) / SECTOR_SIZE as u64;
    if total == 0 || sectors > asl_router_gpt::GPT_ENTRY_ARRAY_SECTORS as u64 {
        return Err("GPT: array entri di luar jendela 32 sektor");
    }
    let mut state = 0xFFFF_FFFFu32;
    let mut sec = [0u8; SECTOR_SIZE];
    let mut remaining = total;
    for i in 0..sectors {
        dev.read_sector(entry_lba + i, &mut sec)?;
        let take = core::cmp::min(remaining, SECTOR_SIZE as u64) as usize;
        state = !asl_router_gpt::crc32_update(state, &sec[..take]);
        remaining -= take as u64;
    }
    Ok(asl_router_gpt::crc32_update(state, &[]))
}

// -----------------------------------------------------------------------------
// Tahap 1, 2, 4: PREFLIGHT + TARGET_IDENTIFICATION + PARTITION_PLAN (murni baca)
// -----------------------------------------------------------------------------

/// Inspeksi penuh tanpa menulis apa pun: validasi sumber (GPT primer + CRC
/// header + CRC array entri + entri partisi), blankness target, kapasitas
/// cocok, dan span salin ESP terdeteksi lewat BPB + scan FAT0.
pub fn inspect<S: BlockIo + ?Sized, T: BlockIo + ?Sized>(
    src: &mut S,
    tgt: &mut T,
    log: &dyn Fn(&str),
) -> Result<InstallPlan, InstallFailure> {
    // PREFLIGHT — kapasitas
    let src_cap = src.capacity_sectors();
    let tgt_cap = tgt.capacity_sectors();
    if src_cap == 0 {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "disk sumber tidak teridentifikasi (kapasitas nol)",
        ));
    }
    if tgt_cap == 0 {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "disk target tidak ditemukan (kapasitas nol)",
        ));
    }
    if src_cap < MIN_CAP_SECTORS {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "kapasitas sumber di bawah minimum layout Rinjani",
        ));
    }
    if src_cap != tgt_cap {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "kapasitas target != sumber — layout backup GPT tidak akan cocok",
        ));
    }
    log("[R27] PREFLIGHT kapasitas src==tgt=");
    log_u64(log, src_cap);
    log("\n");

    // PREFLIGHT — sumber punya GPT primer valid
    let mut sec = [0u8; SECTOR_SIZE];
    src.read_sector(1, &mut sec)
        .map_err(|_| InstallFailure::new(STAGE_PREFLIGHT, "baca LBA1 sumber gagal"))?;
    if &sec[0..8] != b"EFI PART" {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "LBA1 sumber bukan header GPT",
        ));
    }
    let hinfo = asl_router_gpt::parse_gpt_header_lba1(&sec).ok_or(InstallFailure::new(
        STAGE_PREFLIGHT,
        "parse header GPT sumber gagal",
    ))?;
    if !hinfo.header_crc_ok {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "CRC32 header GPT sumber TIDAK valid",
        ));
    }
    let arr_crc_expected = read_le_u32(&sec, 88);
    let arr_crc_actual =
        stream_array_crc(src, hinfo.entry_lba, hinfo.num_entries, hinfo.entry_size)
            .map_err(|e| InstallFailure::new(STAGE_PREFLIGHT, e))?;
    if arr_crc_actual != arr_crc_expected {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "CRC32 array entri GPT sumber tidak cocok",
        ));
    }

    // PREFLIGHT — target harus blank
    tgt.read_sector(1, &mut sec)
        .map_err(|_| InstallFailure::new(STAGE_PREFLIGHT, "baca LBA1 target gagal"))?;
    if &sec[0..8] == b"EFI PART" {
        return Err(InstallFailure::new(
            STAGE_PREFLIGHT,
            "target memuat header GPT — bukan disk kosong; TIDAK ditulis (proteksi salah target)",
        ));
    }
    let cap = src_cap;
    let probes = [
        0u64,
        2,
        34,
        cap / 2,
        cap.saturating_sub(BACKUP_SECTORS),
        cap - 1,
    ];
    for &probe in probes.iter() {
        if probe >= cap {
            continue;
        }
        tgt.read_sector(probe, &mut sec).map_err(|_| {
            InstallFailure::new(STAGE_PREFLIGHT, "baca sektor sampling target gagal")
        })?;
        if !is_zero_sector(&sec) {
            log("[R27] PREFLIGHT target tidak blank di LBA=");
            log_u64(log, probe);
            log("\n");
            return Err(InstallFailure::new(
                STAGE_PREFLIGHT,
                "target tidak blank pada sektor sampling — disk sudah berisi data",
            ));
        }
    }
    log("[R27] PREFLIGHT PASS — sumber valid, target blank terproteksi\n");

    // TARGET_IDENTIFICATION
    log("[R27] TARGET_IDENTIFIED kapasitas_sektor=");
    log_u64(log, cap);
    log(" backup_header_lba=");
    log_u64(log, cap - 1);
    log(" entry_lba=");
    log_u64(log, hinfo.entry_lba);
    log("\n");

    // PARTITION_PLAN — validasi rentang & overlap
    if hinfo.entry_size != 128 || hinfo.num_entries == 0 || hinfo.num_entries > 128 {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "field entri header GPT sumber tidak wajar",
        ));
    }
    if cap < HEAD_SECTORS + BACKUP_SECTORS + 1024 {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "kapasitas terlalu kecil utk region kepala+ekor GPT",
        ));
    }
    let mut ent = [0u8; SECTOR_SIZE];
    src.read_sector(hinfo.entry_lba, &mut ent)
        .map_err(|_| InstallFailure::new(STAGE_PLAN, "baca sektor entri 0..3 gagal"))?;
    let mut esp: Option<asl_router_gpt::GptEntryInfo> = None;
    let mut cfg: Option<asl_router_gpt::GptEntryInfo> = None;
    let mut data: Option<asl_router_gpt::GptEntryInfo> = None;
    let mut prev_end: Option<u64> = None;
    for i in 0..4usize {
        let off = i * 128;
        let e = match asl_router_gpt::parse_gpt_entry(&ent[off..off + 128]) {
            Some(e) if e.used => e,
            _ => continue,
        };
        if e.start_lba > e.end_lba {
            return Err(InstallFailure::new(STAGE_PLAN, "entri dgn start > end"));
        }
        if e.start_lba < hinfo.first_usable_lba || e.end_lba > hinfo.last_usable_lba {
            return Err(InstallFailure::new(
                STAGE_PLAN,
                "rentang partisi di luar usable LBA header (disk korup/liar)",
            ));
        }
        if e.end_lba >= cap {
            return Err(InstallFailure::new(
                STAGE_PLAN,
                "partisi melampaui kapasitas disk",
            ));
        }
        if let Some(pe) = prev_end {
            if e.start_lba <= pe {
                return Err(InstallFailure::new(
                    STAGE_PLAN,
                    "overlap partisi terdeteksi",
                ));
            }
        }
        prev_end = Some(e.end_lba);
        let name = &e.name[..e.name_len];
        if name == b"EFI System Partition" {
            if esp.is_some() {
                return Err(InstallFailure::new(STAGE_PLAN, "lebih dari satu ESP"));
            }
            esp = Some(e);
        } else if name == b"RJCFG01" {
            if cfg.is_some() {
                return Err(InstallFailure::new(STAGE_PLAN, "lebih dari satu RJCFG01"));
            }
            cfg = Some(e);
        } else if name == b"RJDATA" {
            if data.is_some() {
                return Err(InstallFailure::new(STAGE_PLAN, "lebih dari satu RJDATA"));
            }
            data = Some(e);
        }
    }
    let esp = esp.ok_or(InstallFailure::new(
        STAGE_PLAN,
        "partisi ESP tidak ditemukan",
    ))?;
    let cfg = cfg.ok_or(InstallFailure::new(
        STAGE_PLAN,
        "partisi RJCFG01 tidak ditemukan",
    ))?;
    let data = data.ok_or(InstallFailure::new(
        STAGE_PLAN,
        "partisi RJDATA tidak ditemukan",
    ))?;
    let cfg_sectors = cfg
        .end_lba
        .checked_sub(cfg.start_lba)
        .and_then(|d| d.checked_add(1))
        .ok_or(InstallFailure::new(
            STAGE_PLAN,
            "overflow hitungan span RJCFG01",
        ))?;
    if cfg_sectors < RJCFG_INIT_SECTORS {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "span RJCFG01 lebih kecil dari kebutuhan jurnal",
        ));
    }
    let esp_sectors = esp
        .end_lba
        .checked_sub(esp.start_lba)
        .and_then(|d| d.checked_add(1))
        .ok_or(InstallFailure::new(
            STAGE_PLAN,
            "overflow hitungan span ESP",
        ))?;
    if esp_sectors == 0 || esp.start_lba < HEAD_SECTORS {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "ESP tumpang tindih region kepala GPT",
        ));
    }
    if esp.end_lba >= cfg.start_lba || cfg.end_lba >= data.start_lba {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "urutan partisi tidak monoton",
        ));
    }
    if cfg
        .end_lba
        .checked_add(1)
        .and_then(|n| n.checked_add(RJCFG_SLOT_B_OFFSET + RJCFG_SLOT_SECTORS))
        .is_none()
    {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "overflow perhitungan span jurnal",
        ));
    }

    // Deteksi span ESP terpakai via BPB + scan FAT0
    let mut boot = [0u8; SECTOR_SIZE];
    src.read_sector(esp.start_lba, &mut boot)
        .map_err(|_| InstallFailure::new(STAGE_PLAN, "baca boot sector ESP gagal"))?;
    let geo =
        parse_fat32_bpb(&boot, esp_sectors).map_err(InstallFailure::new_wrapped(STAGE_PLAN))?;
    if geo.root_cluster < 2 {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "BPB: root cluster tidak wajar",
        ));
    }
    let esp_used = esp_used_sectors(src, esp.start_lba, &geo)
        .map_err(InstallFailure::new_wrapped(STAGE_PLAN))?;
    if esp_used > esp_sectors {
        return Err(InstallFailure::new(
            STAGE_PLAN,
            "span terpakai ESP melebihi partisi",
        ));
    }
    log("[R27] PARTITION_PLAN_VALIDATED ESP=");
    log_u64(log, esp.start_lba);
    log("..");
    log_u64(log, esp.end_lba);
    log(" esp_terpakai=");
    log_u64(log, esp_used);
    log(" RJCFG01=");
    log_u64(log, cfg.start_lba);
    log("..");
    log_u64(log, cfg.end_lba);
    log(" RJDATA=");
    log_u64(log, data.start_lba);
    log("..");
    log_u64(log, data.end_lba);
    log("\n");

    Ok(InstallPlan {
        total_sectors: cap,
        esp_start: esp.start_lba,
        esp_end: esp.end_lba,
        esp_used_sectors: esp_used,
        cfg_start: cfg.start_lba,
        cfg_end: cfg.end_lba,
        data_start: data.start_lba,
        data_end: data.end_lba,
        entry_lba: hinfo.entry_lba,
    })
}

impl InstallFailure {
    fn new_wrapped(stage: &'static str) -> impl Fn(&'static str) -> Self {
        move |reason: &'static str| InstallFailure { stage, reason }
    }
}

// -----------------------------------------------------------------------------
// Orkestrasi install lengkap (inspect → konfirmasi → tulis → verifikasi)
// -----------------------------------------------------------------------------

struct CopyCtxt {
    sha_w: sha256::Sha256,
    sectors: u64,
}

impl CopyCtxt {
    fn new() -> Self {
        Self {
            sha_w: sha256::Sha256::new(),
            sectors: 0,
        }
    }

    /// Tulis satu sektor ke target, baca-balik, bandingkan, lalu masukkan ke
    /// akumulator digest jalur tulis.
    fn commit<T: BlockIo + ?Sized>(
        &mut self,
        tgt: &mut T,
        lba: u64,
        bytes: &[u8; SECTOR_SIZE],
        stage: &'static str,
    ) -> Result<(), InstallFailure> {
        tgt.write_sector(lba, bytes)
            .map_err(|e| InstallFailure::new(stage, e))?;
        let mut tbuf = [0u8; SECTOR_SIZE];
        tgt.read_sector(lba, &mut tbuf)
            .map_err(|_| InstallFailure::new(stage, "baca-balik sektor target gagal"))?;
        if &tbuf != bytes {
            return Err(InstallFailure::new(stage, "readback byte-mismatch"));
        }
        self.sha_w.update(bytes);
        self.sectors += 1;
        Ok(())
    }

    fn copy_one<S: BlockIo + ?Sized, T: BlockIo + ?Sized>(
        &mut self,
        src: &mut S,
        tgt: &mut T,
        lba: u64,
        stage: &'static str,
    ) -> Result<(), InstallFailure> {
        let mut sbuf = [0u8; SECTOR_SIZE];
        src.read_sector(lba, &mut sbuf)
            .map_err(|_| InstallFailure::new(stage, "baca sektor sumber gagal"))?;
        self.commit(tgt, lba, &sbuf, stage)
    }
}

/// Jalankan instalasi penuh. `src_desc`/`tgt_desc` adalah deskripsi identitas
/// drive dari caller (mis. model ATA + posisi channel); installer MENOLAK bila
/// keduanya identik. Tulis tidak dimulai sama sekali sebelum `confirm_token`
/// cocok persis dengan `CONFIRM_TOKEN`.
pub fn install_run<S: BlockIo + ?Sized, T: BlockIo + ?Sized>(
    src: &mut S,
    tgt: &mut T,
    src_desc: &str,
    tgt_desc: &str,
    confirm_token: &str,
    log: &dyn Fn(&str),
) -> Result<InstallReport, InstallFailure> {
    log("[R27] INSTALL START source=");
    log(src_desc);
    log(" target=");
    log(tgt_desc);
    log("\n");
    if src_desc == tgt_desc {
        return Err(InstallFailure::new(
            STAGE_TARGET_ID,
            "deskriptor sumber == target — penolakan proteksi",
        ));
    }

    let plan = inspect(src, tgt, log)?;

    // TARGET_CONFIRMATION
    if confirm_token != CONFIRM_TOKEN {
        return Err(InstallFailure::new(
            STAGE_CONFIRM,
            "token konfirmasi tidak cocok — TIDAK ADA sektor yang ditulis",
        ));
    }
    log("[R27] TARGET CONFIRMED (token benar) — operasi destruktif DIMULAI\n");

    let mut ctx = CopyCtxt::new();

    // PARTITION_WRITE — kepala disk verbatim (MBR + header + array entri;
    // stage2 Limine tertanam di slack array sehingga ikut tersalin)
    log("[R27] GPT WRITE START head LBA 0..");
    log_u64(log, HEAD_SECTORS - 1);
    log("\n");
    for lba in 0..HEAD_SECTORS {
        ctx.copy_one(src, tgt, lba, STAGE_PART_WRITE)?;
    }

    // Backup GPT di ekor — posisi sama dgn sumber (kapasitas sudah divalidasi sama)
    let bstart = plan
        .total_sectors
        .checked_sub(BACKUP_SECTORS)
        .ok_or(InstallFailure::new(
            STAGE_PART_WRITE,
            "underflow posisi backup",
        ))?;
    for lba in bstart..plan.total_sectors {
        ctx.copy_one(src, tgt, lba, STAGE_PART_WRITE)?;
    }
    log("[R27] GPT READBACK VERIFIED (head + backup, byte-per-byte)\n");

    // BOOT_ARTIFACT_INSTALL — salin span ESP terpakai (boot chain + file boot)
    log("[R27] ARTIFACT WRITE START ESP LBA ");
    log_u64(log, plan.esp_start);
    log(" span=");
    log_u64(log, plan.esp_used_sectors);
    log("\n");
    for i in 0..plan.esp_used_sectors {
        let lba = plan
            .esp_start
            .checked_add(i)
            .ok_or(InstallFailure::new(STAGE_ARTIFACT, "overflow LBA span ESP"))?;
        if lba > plan.esp_end {
            return Err(InstallFailure::new(
                STAGE_ARTIFACT,
                "span ESP melintasi batas partisi",
            ));
        }
        ctx.copy_one(src, tgt, lba, STAGE_ARTIFACT)?;
    }
    log("[R27] ARTIFACT READBACK VERIFIED per-sektor (ESP terpakai)\n");

    // CONFIG_INITIALIZATION — RJCFG01 BARU di target (seq=1 Slot A); wilayah
    // 24 sektor ditulis deterministik (header + reserved + Slot A + Slot B).
    log("[R27] RJCFG01 INIT on target LBA ");
    log_u64(log, plan.cfg_start);
    log("\n");
    let journal = asl_router_persistence::RjcfgJournal::new_default();
    let mut hdr_sec = [0u8; SECTOR_SIZE];
    journal.serialize_header_to_sector(&mut hdr_sec);
    let zero = [0u8; SECTOR_SIZE];
    ctx.commit(tgt, plan.cfg_start, &hdr_sec, STAGE_CONFIG)?;
    for i in 1..RJCFG_SLOT_A_OFFSET {
        ctx.commit(tgt, plan.cfg_start + i, &zero, STAGE_CONFIG)?;
    }
    for i in 0..RJCFG_SLOT_SECTORS {
        let mut sec = [0u8; SECTOR_SIZE];
        let o = (i as usize) * SECTOR_SIZE;
        sec.copy_from_slice(&journal.slot_a[o..o + SECTOR_SIZE]);
        ctx.commit(
            tgt,
            plan.cfg_start + RJCFG_SLOT_A_OFFSET + i,
            &sec,
            STAGE_CONFIG,
        )?;
    }
    for i in 0..RJCFG_SLOT_SECTORS {
        ctx.commit(
            tgt,
            plan.cfg_start + RJCFG_SLOT_B_OFFSET + i,
            &zero,
            STAGE_CONFIG,
        )?;
    }

    // Verifikasi struktural jurnal dari BACAAN TARGET (bukan buffer tulis)
    let mut vsec = [0u8; SECTOR_SIZE];
    tgt.read_sector(plan.cfg_start, &mut vsec)
        .map_err(|_| InstallFailure::new(STAGE_CONFIG, "baca header RJCFG01 target gagal"))?;
    let hdr = asl_router_persistence::RjcfgJournal::deserialize_header_from_sector(&vsec)
        .map_err(|e| InstallFailure::new(STAGE_CONFIG, e))?;
    let mut slot_buf = [0u8; SLOT_BYTES];
    for i in 0..RJCFG_SLOT_SECTORS {
        tgt.read_sector(plan.cfg_start + RJCFG_SLOT_A_OFFSET + i, &mut vsec)
            .map_err(|_| InstallFailure::new(STAGE_CONFIG, "baca slot A RJCFG01 target gagal"))?;
        let o = (i as usize) * SECTOR_SIZE;
        slot_buf[o..o + SECTOR_SIZE].copy_from_slice(&vsec);
    }
    let loaded = asl_router_persistence::RjcfgJournal::load_from_disk(&hdr, &slot_buf)
        .map_err(|e| InstallFailure::new(STAGE_CONFIG, e))?;
    let seq = loaded.header.sequence_number;
    let slot = loaded.header.active_slot;
    if seq != 1 || slot != 0 {
        return Err(InstallFailure::new(
            STAGE_CONFIG,
            "jurnal target bukan seq=1 Slot A",
        ));
    }
    log("[R27] RJCFG01 INITIALIZED (seq=1 Slot A, CRC tervalidasi dari bacaan target)\n");

    // READBACK_VERIFICATION — digest global: hash ulang SEMUA wilayah tulis di
    // target dalam urutan TULIS yang sama (kepala, ekor, ESP, jurnal).
    log("[R27] INSTALL VERIFICATION — hash ulang wilayah target...\n");
    let cfg_span_end = plan
        .cfg_start
        .checked_add(RJCFG_INIT_SECTORS)
        .ok_or(InstallFailure::new(STAGE_READBACK, "overflow span jurnal"))?;
    if cfg_span_end > plan.cfg_end + 1 {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "span jurnal melampaui partisi RJCFG01",
        ));
    }
    let mut sha_r = sha256::Sha256::new();
    let mut read_ok = true;
    {
        let mut buf = [0u8; SECTOR_SIZE];
        let ranges = [
            (0u64, HEAD_SECTORS),
            (bstart, BACKUP_SECTORS),
            (plan.esp_start, plan.esp_used_sectors),
            (plan.cfg_start, RJCFG_INIT_SECTORS),
        ];
        for &(start, count) in ranges.iter() {
            for i in 0..count {
                if tgt.read_sector(start + i, &mut buf).is_err() {
                    read_ok = false;
                    break;
                }
                sha_r.update(&buf);
            }
            if !read_ok {
                break;
            }
        }
    }
    if !read_ok {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "gagal membaca kembali wilayah target",
        ));
    }
    let sha_written = ctx.sha_w.finalize();
    let sha_readback = sha_r.finalize();
    if sha_written != sha_readback {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "SHA-256 jalur verifikasi != jalur tulis — instalasi TIDAK dipercaya",
        ));
    }

    // Sanity struktural akhir pada TARGET: header primer, CRC array, backup
    let mut tsec = [0u8; SECTOR_SIZE];
    tgt.read_sector(1, &mut tsec)
        .map_err(|_| InstallFailure::new(STAGE_READBACK, "baca header target gagal"))?;
    let th = asl_router_gpt::parse_gpt_header_lba1(&tsec).ok_or(InstallFailure::new(
        STAGE_READBACK,
        "parse header target gagal",
    ))?;
    if !th.header_crc_ok {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "CRC header target tidak valid",
        ));
    }
    if stream_array_crc(tgt, th.entry_lba, th.num_entries, th.entry_size)
        .map_err(|e| InstallFailure::new(STAGE_READBACK, e))?
        != read_le_u32(&tsec, 88)
    {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "CRC array entri target tidak cocok",
        ));
    }
    if th.last_usable_lba >= plan.total_sectors - BACKUP_SECTORS {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "last_usable_lba target menggangu cadangan GPT",
        ));
    }
    tgt.read_sector(plan.total_sectors - 1, &mut tsec)
        .map_err(|_| InstallFailure::new(STAGE_READBACK, "baca backup header target gagal"))?;
    if &tsec[0..8] != b"EFI PART" {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "backup header target tidak ditemukan",
        ));
    }
    let cur = read_le_u64(&tsec, 24);
    if cur != plan.total_sectors - 1 {
        log("[R27] READBACK backup current_lba salah=");
        log_u64(log, cur);
        log("\n");
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "current_lba backup header salah",
        ));
    }
    let bh = asl_router_gpt::parse_gpt_header_lba1(&tsec).ok_or(InstallFailure::new(
        STAGE_READBACK,
        "parse backup header target gagal",
    ))?;
    if !bh.header_crc_ok {
        return Err(InstallFailure::new(
            STAGE_READBACK,
            "CRC backup header target tidak valid",
        ));
    }

    // INSTALL_COMPLETE
    let report = InstallReport {
        plan,
        sectors_written: ctx.sectors,
        esp_sectors: plan.esp_used_sectors,
        sha_written,
        sha_readback,
    };
    log("[R27] INSTALL VERIFICATION PASSED sektor_tulis=");
    log_u64(log, report.sectors_written);
    log(" sha256=");
    log_hex_bytes(log, &report.sha_written);
    log("\n[R27] INSTALL DONE\n");
    Ok(report)
}

// -----------------------------------------------------------------------------
// Host-side unit tests (mock block device — BUKAN pengujian hardware nyata)
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use asl_router_gpt::{
        build_protective_mbr, compute_crc32, GptHeader, GptPartitionEntry, GPT_HEADER_REVISION_1_0,
        GPT_HEADER_SIGNATURE, GPT_HEADER_SIZE, GPT_MAX_PARTITIONS, GPT_PARTITION_ENTRY_SIZE,
        GUID_ESP, GUID_RINJANI_CFG, GUID_RINJANI_DATA,
    };
    use std::collections::BTreeMap;
    use std::vec;
    use std::vec::Vec;

    fn quiet(_s: &str) {}

    // -- Mock disk sparse -----------------------------------------------------

    struct MockDisk {
        capacity: u64,
        data: BTreeMap<u64, [u8; SECTOR_SIZE]>,
        reads: BTreeMap<u64, u32>,
        writes: Vec<u64>,
        fail_read_at: Option<u64>,
        fail_write_at: Option<u64>,
        /// Tulis diterima tetapi disimpan dalam keadaan rusak (uji readback mismatch).
        corrupt_write_at: Option<u64>,
        /// Baca pertama jujur, bacaan berikutnya di LBA ini dibalik (uji digest mismatch).
        flip_on_later_read_at: Option<u64>,
    }

    impl MockDisk {
        fn new(capacity: u64) -> Self {
            Self {
                capacity,
                data: BTreeMap::new(),
                reads: BTreeMap::new(),
                writes: Vec::new(),
                fail_read_at: None,
                fail_write_at: None,
                corrupt_write_at: None,
                flip_on_later_read_at: None,
            }
        }
        fn put(&mut self, lba: u64, bytes: &[u8]) {
            let mut sec = [0u8; SECTOR_SIZE];
            sec.copy_from_slice(&bytes[..SECTOR_SIZE]);
            self.data.insert(lba, sec);
        }
        fn get(&self, lba: u64) -> [u8; SECTOR_SIZE] {
            self.data.get(&lba).copied().unwrap_or([0u8; SECTOR_SIZE])
        }
    }

    impl BlockIo for MockDisk {
        fn capacity_sectors(&self) -> u64 {
            self.capacity
        }
        fn read_sector(
            &mut self,
            lba: u64,
            buf: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), &'static str> {
            if lba >= self.capacity {
                return Err("LBA di luar kapasitas disk");
            }
            if self.fail_read_at == Some(lba) {
                return Err("mock: gagal baca sektor");
            }
            let mut got = self.get(lba);
            let n = self.reads.entry(lba).or_insert(0u32);
            *n += 1;
            if self.flip_on_later_read_at == Some(lba) && *n > 1 {
                got[0x100] ^= 0xFF;
            }
            buf.copy_from_slice(&got);
            Ok(())
        }
        fn write_sector(
            &mut self,
            lba: u64,
            bytes: &[u8; SECTOR_SIZE],
        ) -> Result<(), &'static str> {
            if lba >= self.capacity {
                return Err("LBA di luar kapasitas disk");
            }
            if self.fail_write_at == Some(lba) {
                return Err("mock: gagal tulis sektor");
            }
            let mut stored = *bytes;
            if self.corrupt_write_at == Some(lba) {
                stored[0x1F0] ^= 0xA5;
            }
            self.data.insert(lba, stored);
            self.writes.push(lba);
            Ok(())
        }
    }

    // -- Builder disk sumber --------------------------------------------------

    struct Spec {
        total: u64,
        esp_start: u64,
        esp_len: u64,
        cfg_start: u64,
        cfg_len: u64,
        data_start: u64,
        data_end: u64,
        reserved: u64,
        fats: u64,
        fat_len: u64,
        spc: u64,
        max_cluster: u64,
        corrupt_hdr_crc: bool,
        corrupt_arr_crc: bool,
        entry_lba: u64,
        drop_cfg: bool,
        first_usable: u64,
    }

    impl Spec {
        /// Layout sehat: kapasitas 512 MiB, ESP 8 MiB, RJCFG01 48 sektor.
        fn good() -> Self {
            let total: u64 = 1_048_576;
            Self {
                total,
                esp_start: 34,
                esp_len: 8192,
                cfg_start: 34 + 8192,
                cfg_len: 48,
                data_start: 34 + 8192 + 48,
                data_end: total - 34,
                reserved: 32,
                fats: 2,
                fat_len: 8,
                spc: 1,
                max_cluster: 200,
                corrupt_hdr_crc: false,
                corrupt_arr_crc: false,
                entry_lba: 2,
                drop_cfg: false,
                first_usable: 34,
            }
        }
    }

    fn make_bpb(
        spc: u64,
        reserved: u64,
        fats: u64,
        fat_len: u64,
        esp_sectors: u64,
    ) -> [u8; SECTOR_SIZE] {
        let mut b = [0u8; SECTOR_SIZE];
        b[11..13].copy_from_slice(&512u16.to_le_bytes());
        b[13] = spc as u8;
        b[14..16].copy_from_slice(&(reserved as u16).to_le_bytes());
        b[16] = fats as u8;
        b[32..36].copy_from_slice(&0u32.to_le_bytes()); // fat16 length wajib 0
        b[32..36].copy_from_slice(&(esp_sectors as u32).to_le_bytes()); // TotalSectors32
        b[36..40].copy_from_slice(&(fat_len as u32).to_le_bytes()); // FATSz32
        b[44..48].copy_from_slice(&2u32.to_le_bytes()); // RootClus
        b[82..87].copy_from_slice(b"FAT32");
        b[510] = 0x55;
        b[511] = 0xAA;
        b
    }

    fn entry(name: &str, type_guid: [u8; 16], start: u64, end: u64, no: u8) -> GptPartitionEntry {
        let mut e = GptPartitionEntry::empty();
        e.partition_type_guid = type_guid;
        e.unique_partition_guid = asl_router_gpt::make_unique_guid(no);
        e.starting_lba = start;
        e.ending_lba = end;
        e.set_name(name);
        e
    }

    fn header(
        spec: &Spec,
        arr_crc: u32,
        current_lba: u64,
        backup_lba: u64,
        entry_lba: u64,
    ) -> GptHeader {
        GptHeader {
            signature: GPT_HEADER_SIGNATURE,
            revision: GPT_HEADER_REVISION_1_0,
            header_size: GPT_HEADER_SIZE,
            header_crc32: 0,
            reserved: 0,
            current_lba,
            backup_lba,
            first_usable_lba: spec.first_usable,
            last_usable_lba: spec.total - 34,
            disk_guid: [7u8; 16],
            partition_entry_lba: entry_lba,
            num_partition_entries: GPT_MAX_PARTITIONS as u32,
            size_of_partition_entry: GPT_PARTITION_ENTRY_SIZE,
            partition_array_crc32: arr_crc,
        }
    }

    fn build_source(spec: &Spec) -> MockDisk {
        let mut d = MockDisk::new(spec.total);

        // LBA0 — protective MBR
        let mut mbr = [0u8; SECTOR_SIZE];
        build_protective_mbr(spec.total, &mut mbr);
        d.put(0, &mbr);

        // Array entri 128 slot
        let mut arr = [0u8; GPT_MAX_PARTITIONS * GPT_PARTITION_ENTRY_SIZE as usize];
        let mut entries: Vec<GptPartitionEntry> = Vec::new();
        entries.push(entry(
            "EFI System Partition",
            GUID_ESP,
            spec.esp_start,
            spec.esp_start + spec.esp_len - 1,
            1,
        ));
        if !spec.drop_cfg && spec.cfg_len > 0 {
            entries.push(entry(
                "RJCFG01",
                GUID_RINJANI_CFG,
                spec.cfg_start,
                spec.cfg_start + spec.cfg_len - 1,
                2,
            ));
        }
        entries.push(entry(
            "RJDATA",
            GUID_RINJANI_DATA,
            spec.data_start,
            spec.data_end,
            3,
        ));
        for (i, e) in entries.iter().enumerate() {
            let bytes = e.to_bytes();
            arr[i * 128..(i + 1) * 128].copy_from_slice(&bytes);
        }
        let mut arr_crc = compute_crc32(&arr);
        if spec.corrupt_arr_crc {
            arr_crc ^= 0xDEAD_BEEF;
        }
        for i in 0..32u64 {
            let off = (i as usize) * SECTOR_SIZE;
            d.put(spec.entry_lba + i, &arr[off..off + SECTOR_SIZE]);
        }

        // Header primer di LBA1
        let mut h = header(spec, arr_crc, 1, spec.total - 1, spec.entry_lba);
        let mut hb = [0u8; SECTOR_SIZE];
        let hbytes = h.to_bytes();
        hb[..GPT_HEADER_SIZE as usize].copy_from_slice(&hbytes);
        if spec.corrupt_hdr_crc {
            hb[40] ^= 0xFF; // rusak isi -> header CRC tidak cocok
        }
        d.put(1, &hb);

        // Header + array cadangan di ekor
        let mut bh = header(spec, arr_crc, spec.total - 1, 1, spec.total - 33);
        let mut bb = [0u8; SECTOR_SIZE];
        let bbytes = bh.to_bytes();
        bb[..GPT_HEADER_SIZE as usize].copy_from_slice(&bbytes);
        d.put(spec.total - 1, &bb);
        for i in 0..32u64 {
            let off = (i as usize) * SECTOR_SIZE;
            d.put(spec.total - 33 + i, &arr[off..off + SECTOR_SIZE]);
        }

        // ESP: boot sector BPB + FAT + cluster teralokasi
        let boot = make_bpb(
            spec.spc,
            spec.reserved,
            spec.fats,
            spec.fat_len,
            spec.esp_len,
        );
        d.put(spec.esp_start, &boot);
        let meta = spec.reserved + spec.fats * spec.fat_len;
        for f in 0..spec.fats {
            let base = spec.esp_start + spec.reserved + f * spec.fat_len;
            for fs in 0..spec.fat_len {
                let mut sec = [0u8; SECTOR_SIZE];
                for i in 0..128u64 {
                    let gidx = fs * 128 + i;
                    if gidx < 2 || gidx > spec.max_cluster {
                        continue;
                    }
                    let off = (i as usize) * 4;
                    sec[off..off + 4].copy_from_slice(&0x0FFF_FFF8u32.to_le_bytes());
                }
                d.put(base + fs, &sec);
            }
        }
        // Isi cluster (data non-nol) supaya byte-equality punya arti
        for c in 2..=spec.max_cluster {
            let mut sec = [0u8; SECTOR_SIZE];
            for (i, b) in sec.iter_mut().enumerate() {
                *b = (c as u8).wrapping_add(i as u8);
            }
            let lba = spec.esp_start + meta + (c - 2) * spec.spc;
            if lba <= spec.esp_start + spec.esp_len - 1 {
                d.put(lba, &sec);
            }
        }
        d
    }

    // -- A. SHA-256 -----------------------------------------------------------

    #[test]
    fn test_20_sha256_fips_vectors() {
        // Vektor resmi FIPS 180-4 / NIST.
        let empty = digest_oneshot_str(b"");
        let expect_empty: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(empty, expect_empty);
        let abc = sha256::digest_oneshot(b"abc");
        let expect_abc: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(abc, expect_abc);
    }

    fn digest_oneshot_str(s: &[u8]) -> [u8; 32] {
        sha256::digest_oneshot(s)
    }

    #[test]
    fn test_21_sha256_streaming_peek_equals_oneshot() {
        let mut h = sha256::Sha256::new();
        let mut expect = sha256::Sha256::new();
        for i in 0..257u32 {
            let chunk = [i as u8; 3];
            expect.update(&chunk);
            // pemotongan tak sejajar blok (sektor 512 B dipecah 3+3+...)
            h.update(&chunk[..1]);
            h.update(&chunk[1..]);
        }
        assert_eq!(h.peek(), expect.finalize());
        // peek tidak boleh mengubah akumulator
        let mut k = sha256::Sha256::new();
        k.update(b"1234567890123456789012345678901234567890123456789012345678901234567890");
        let p1 = k.peek();
        let p2 = k.peek();
        assert_eq!(p1, p2);
        assert_eq!(p1, k.finalize());
    }

    // -- B. BPB parser --------------------------------------------------------

    #[test]
    fn test_22_bpb_rejects_bad_input() {
        let b = make_bpb(1, 32, 2, 8, 8192);
        assert!(parse_fat32_bpb(&b, 8192).is_ok());

        let mut nosig = b;
        nosig[510] = 0x00;
        assert_eq!(
            parse_fat32_bpb(&nosig, 8192),
            Err("BPB: signature boot sector 0x55AA hilang")
        );

        let mut notfat = b;
        notfat[82..87].copy_from_slice(b"FAT16");
        assert_eq!(
            parse_fat32_bpb(&notfat, 8192),
            Err("BPB: bukan volume FAT32 (fstyp)")
        );

        let mut bps = b;
        bps[11..13].copy_from_slice(&1024u16.to_le_bytes());
        assert_eq!(
            parse_fat32_bpb(&bps, 8192),
            Err("BPB: bytes_per_sector != 512")
        );

        let mut spc0 = b;
        spc0[13] = 0;
        assert_eq!(
            parse_fat32_bpb(&spc0, 8192),
            Err("BPB: sectors_per_cluster tidak valid")
        );

        let mut spc3 = b;
        spc3[13] = 3;
        assert_eq!(
            parse_fat32_bpb(&spc3, 8192),
            Err("BPB: sectors_per_cluster tidak valid")
        );

        let mut fat16len = b;
        fat16len[22..24].copy_from_slice(&9u16.to_le_bytes());
        assert_eq!(
            parse_fat32_bpb(&fat16len, 8192),
            Err("BPB: fat16 length harus 0 pada FAT32")
        );

        let mut rootent = b;
        rootent[17..19].copy_from_slice(&224u16.to_le_bytes());
        assert_eq!(
            parse_fat32_bpb(&rootent, 8192),
            Err("BPB: root entry count harus 0 pada FAT32")
        );

        let mut tot16 = b;
        tot16[19..21].copy_from_slice(&32000u16.to_le_bytes());
        assert_eq!(
            parse_fat32_bpb(&tot16, 8192),
            Err("BPB: total sektor 16 harus 0 pada FAT32")
        );

        // BPB menyebut volume 8192 sektor tetapi dipasang di partisi 4096 -> ditolak
        assert_eq!(
            parse_fat32_bpb(&b, 4096),
            Err("BPB: total sektor volume != ukuran partisi ESP")
        );

        let mut fatzero = b;
        fatzero[36..40].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            parse_fat32_bpb(&fatzero, 8192),
            Err("BPB: fat32 length nol")
        );

        let mut fats0 = b;
        fats0[16] = 0;
        assert_eq!(
            parse_fat32_bpb(&fats0, 8192),
            Err("BPB: jumlah FAT tidak valid")
        );

        let mut fats9 = b;
        fats9[16] = 9;
        assert_eq!(
            parse_fat32_bpb(&fats9, 8192),
            Err("BPB: jumlah FAT tidak valid")
        );

        // region metadata melebihi ukuran partisi
        let tight = make_bpb(1, 32, 2, 8, 48);
        assert_eq!(
            parse_fat32_bpb(&tight, 48),
            Err("BPB: region metadata melebihi ukuran partisi")
        );

        // geometri terlalu besar (dibatasi sebelum aritmetika span).
        // tot32 dinolkan agar ukuran partisi dari caller yang jadi acuan,
        // sehingga yang diuji adalah guard geometri, bukan konsistensi BPB.
        let mut big = b;
        big[13] = 4;
        big[16] = 4;
        big[32..36].copy_from_slice(&0u32.to_le_bytes());
        big[36..40].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert_eq!(
            parse_fat32_bpb(&big, u64::MAX),
            Err("BPB: geometri terlalu besar")
        );
    }

    #[test]
    fn test_23_bpb_and_fat_span_synthetic() {
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let boot = src.get(spec.esp_start);
        let geo = parse_fat32_bpb(&boot, spec.esp_len).unwrap();
        assert_eq!(geo.reserved_sectors, 32);
        assert_eq!(geo.fat_len, 8);
        assert_eq!(geo.root_cluster, 2);
        let meta = 32 + 2 * 8;
        assert_eq!(geo.data_start, meta);
        // cluster tertinggi teralokasi = 200 -> span = meta + (200-1)*spc
        assert_eq!(
            esp_used_sectors(&mut src, spec.esp_start, &geo).unwrap(),
            meta + 199
        );
    }

    #[test]
    fn test_24_esp_span_cosong_moved_to_meta() {
        // "Ukuran artefak kosong": FAT tanpa cluster teralokasi -> span = metadata saja.
        let mut spec = Spec::good();
        spec.max_cluster = 1;
        let mut src = build_source(&spec);
        let boot = src.get(spec.esp_start);
        let geo = parse_fat32_bpb(&boot, spec.esp_len).unwrap();
        let used = esp_used_sectors(&mut src, spec.esp_start, &geo).unwrap();
        assert_eq!(used, geo.data_start);
        assert!(used > 0);
    }

    #[test]
    fn test_25_bpb_diparse_dari_format_esp_asli() {
        // Validasi parser terhadap keluaran formatter FAT32 produksi (IS-01).
        use asl_router_fat32::{compute_fat_len, EspFile, ESP_SECTORS};
        let mut buf: Vec<u8> = vec![0u8; ESP_SECTORS as usize * SECTOR_SIZE];
        let payload: Vec<u8> = vec![0xABu8; 4096];
        let files = [
            EspFile {
                path: "boot/rinjani-kernel.elf",
                data: &payload,
            },
            EspFile {
                path: "boot/limine/limine.conf",
                data: b"timeout 5\n",
            },
        ];
        let stats = asl_router_fat32::format_esp(&mut buf, &files).expect("format_esp gagal");
        let boot: [u8; SECTOR_SIZE] = buf[0..SECTOR_SIZE].try_into().unwrap();
        let geo =
            parse_fat32_bpb(&boot, ESP_SECTORS).expect("BPB hasil formatter harus lolos parser");
        assert_eq!(geo.fat_len, compute_fat_len(ESP_SECTORS));
        assert_eq!(geo.bytes_per_sector, 512);
        assert!(stats.clusters_used >= 1);

        // Batas salin HARUS menutupi seluruh sektor ESP yang berisi data non-nol.
        let mut last_nonzero = 0u64;
        for i in 0..ESP_SECTORS {
            let off = i as usize * SECTOR_SIZE;
            if buf[off..off + SECTOR_SIZE].iter().any(|&b| b != 0) {
                last_nonzero = i;
            }
        }
        // Mock hanya perlu region FAT0; sektor tak diisi otomatis terbaca nol.
        let mut src = MockDisk::new(ESP_SECTORS);
        for i in 0..(geo.reserved_sectors + geo.fat_len) {
            let off = i as usize * SECTOR_SIZE;
            src.put(i, &buf[off..off + SECTOR_SIZE]);
        }
        let used = esp_used_sectors(&mut src, 0, &geo).unwrap();
        assert!(
            used > last_nonzero,
            "span salin ESP (used={}) harus menutupi sektor terpakai terakhir ({})",
            used,
            last_nonzero
        );
        assert!(used <= ESP_SECTORS);
    }

    // -- C. Preflight / plan: kasus gagal ------------------------------------

    #[test]
    fn test_26_disk_tidak_ditemukan_dan_kapasitas_nol() {
        let mut src = MockDisk::new(0);
        let mut tgt = MockDisk::new(0);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "disk sumber tidak teridentifikasi (kapasitas nol)"
            })
        );
        let mut src = MockDisk::new(MIN_CAP_SECTORS);
        let mut tgt = MockDisk::new(0);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "disk target tidak ditemukan (kapasitas nol)"
            })
        );
    }

    #[test]
    fn test_27_kapasitas_src_tgt_berbeda_dan_terlalu_kecil() {
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total - 2048);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "kapasitas target != sumber — layout backup GPT tidak akan cocok"
            })
        );

        let mut small_spec = Spec::good();
        small_spec.total = 4096;
        small_spec.data_end = 4096 - 34;
        let mut s2 = build_source(&small_spec);
        let mut t2 = MockDisk::new(4096);
        assert_eq!(
            inspect(&mut s2, &mut t2, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "kapasitas sumber di bawah minimum layout Rinjani"
            })
        );
    }

    #[test]
    fn test_28_sumber_bukan_gpt_dan_io_gagal() {
        let mut src = MockDisk::new(MIN_CAP_SECTORS);
        let mut tgt = MockDisk::new(MIN_CAP_SECTORS);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "LBA1 sumber bukan header GPT"
            })
        );

        let spec = Spec::good();
        let mut src = build_source(&spec);
        src.fail_read_at = Some(1);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "baca LBA1 sumber gagal"
            })
        );

        let mut src = build_source(&spec);
        src.fail_read_at = Some(5); // sektor array entri
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "mock: gagal baca sektor"
            })
        );

        let mut tgt = MockDisk::new(spec.total);
        tgt.fail_read_at = Some(0);
        let mut src = build_source(&spec);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "baca sektor sampling target gagal"
            })
        );
    }

    #[test]
    fn test_29_crc_header_dan_crc_array_sumber_salah() {
        let mut spec = Spec::good();
        spec.corrupt_hdr_crc = true;
        let src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        let mut src = src;
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "CRC32 header GPT sumber TIDAK valid"
            })
        );

        let mut spec = Spec::good();
        spec.corrupt_arr_crc = true;
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "CRC32 array entri GPT sumber tidak cocok"
            })
        );
    }

    #[test]
    fn test_30_target_sudah_berisi_gpt_atau_data_ditolak() {
        let spec = Spec::good();
        let src = build_source(&spec);
        // target meniru disk terpasang (punya GPT) -> proteksi salah target
        let mut tgt = build_source(&spec);
        let mut src = src;
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "target memuat header GPT — bukan disk kosong; TIDAK ditulis (proteksi salah target)"
            })
        );

        // target kosong tapi ada satu sektor berisi data
        let mut tgt = MockDisk::new(spec.total);
        let junk = [0x5Au8; SECTOR_SIZE];
        tgt.put(spec.total / 2, &junk);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PREFLIGHT,
                reason: "target tidak blank pada sektor sampling — disk sudah berisi data"
            })
        );
        assert!(tgt.writes.is_empty(), "inspeksi tidak boleh menulis target");
    }

    #[test]
    fn test_31_overlap_di_luar_kapasitas_dan_partisi_hilang() {
        // overlap: ESP dan RJCFG01 bertumpuk
        let mut spec = Spec::good();
        spec.cfg_start = spec.esp_start + spec.esp_len - 10;
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PLAN,
                reason: "overlap partisi terdeteksi"
            })
        );

        // di luar usable LBA header
        let mut spec = Spec::good();
        spec.data_end = spec.total - 10; // melewati last_usable (total-34)
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PLAN,
                reason: "rentang partisi di luar usable LBA header (disk korup/liar)"
            })
        );

        // partisi RJCFG01 hilang
        let mut spec = Spec::good();
        spec.drop_cfg = true;
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PLAN,
                reason: "partisi RJCFG01 tidak ditemukan"
            })
        );

        // urutan tidak monoton (RJDATA sebelum RJCFG01)
        let mut spec = Spec::good();
        spec.data_start = spec.esp_start + 100;
        spec.data_end = spec.esp_start + 200;
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PLAN,
                reason: "overlap partisi terdeteksi"
            })
        );

        // RJCFG01 terlalu kecil utk jurnal
        let mut spec = Spec::good();
        spec.cfg_len = 8;
        spec.data_start = spec.cfg_start + 8;
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PLAN,
                reason: "span RJCFG01 lebih kecil dari kebutuhan jurnal"
            })
        );

        // ESP menimpa region kepala GPT (header + array entri dipindah agar
        // pemeriksaan ini yang lebih dulu bertemu, bukan CRC array)
        let mut spec = Spec::good();
        spec.esp_start = 20;
        spec.first_usable = 20;
        spec.entry_lba = 400;
        spec.cfg_start = 20 + spec.esp_len;
        spec.data_start = spec.cfg_start + spec.cfg_len;
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            inspect(&mut src, &mut tgt, &quiet),
            Err(InstallFailure {
                stage: STAGE_PLAN,
                reason: "ESP tumpang tindih region kepala GPT"
            })
        );
    }

    // -- D. Konfirmasi & proteksi destruktif ---------------------------------

    #[test]
    fn test_32_token_salah_nol_sektor_ditulis() {
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        let r = install_run(
            &mut src,
            &mut tgt,
            "ata0-master",
            "ata0-slave",
            "YES",
            &quiet,
        );
        assert_eq!(
            r,
            Err(InstallFailure {
                stage: STAGE_CONFIRM,
                reason: "token konfirmasi tidak cocok — TIDAK ADA sektor yang ditulis"
            })
        );
        assert!(tgt.writes.is_empty());
        // sumber juga tidak pernah disentuh
        assert!(src.writes.is_empty());
    }

    #[test]
    fn test_33_deskriptor_sama_ditolak() {
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        assert_eq!(
            install_run(
                &mut src,
                &mut tgt,
                "ata0-master",
                "ata0-master",
                CONFIRM_TOKEN,
                &quiet
            ),
            Err(InstallFailure {
                stage: STAGE_TARGET_ID,
                reason: "deskriptor sumber == target — penolakan proteksi"
            })
        );
        assert!(tgt.writes.is_empty());
    }

    // -- E. Happy path end-to-end (mock) -------------------------------------

    #[test]
    fn test_34_install_happy_path_terverifikasi() {
        let spec = Spec::good();
        let src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        let mut src = src;
        let meta_expect = spec.reserved + spec.fats * spec.fat_len;
        let report = install_run(
            &mut src,
            &mut tgt,
            "ata0-master VBOX HARDDISK",
            "ata0-slave",
            CONFIRM_TOKEN,
            &quiet,
        )
        .expect("instalasi mock harus sukses");
        assert_eq!(report.plan.total_sectors, spec.total);
        assert_eq!(report.plan.esp_used_sectors, meta_expect + 199);
        assert_eq!(
            report.sectors_written,
            HEAD_SECTORS + BACKUP_SECTORS + report.esp_sectors + RJCFG_INIT_SECTORS
        );
        assert_eq!(report.sha_written, report.sha_readback);

        // Target identik sumber pada region yang disalin
        for lba in 0..HEAD_SECTORS {
            assert_eq!(tgt.get(lba), src.get(lba), "kepala disk LBA {}", lba);
        }
        for lba in (spec.total - BACKUP_SECTORS)..spec.total {
            assert_eq!(tgt.get(lba), src.get(lba), "ekor disk LBA {}", lba);
        }
        for i in 0..report.esp_sectors {
            assert_eq!(tgt.get(spec.esp_start + i), src.get(spec.esp_start + i));
        }
        // Region yang TIDAK disalin harus tetap kosong di target
        let skip = spec.esp_start + report.esp_sectors + 10;
        assert!(skip < spec.cfg_start);
        assert!(is_zero_sector(&tgt.get(skip)));

        // Digest independen: wilayah salin dihitung dari SUMBER, wilayah jurnal
        // dihitung dari isi TARGET yang baru diinisialisasi (urutan tulis sama).
        let mut h = sha256::Sha256::new();
        for &(start, count) in &[
            (0u64, HEAD_SECTORS),
            (spec.total - BACKUP_SECTORS, BACKUP_SECTORS),
            (spec.esp_start, report.esp_sectors),
        ] {
            for i in 0..count {
                h.update(&src.get(start + i));
            }
        }
        for i in 0..RJCFG_INIT_SECTORS {
            h.update(&tgt.get(spec.cfg_start + i));
        }
        assert_eq!(report.sha_written, h.finalize());

        // GPT target valid secara mandiri
        let th = src.get(1);
        assert_eq!(tgt.get(1), th);
        let info = asl_router_gpt::parse_gpt_header_lba1(&tgt.get(1)).unwrap();
        assert!(info.header_crc_ok);
        assert_eq!(
            stream_array_crc(&mut tgt, info.entry_lba, info.num_entries, info.entry_size).unwrap(),
            read_le_u32(&tgt.get(1), 88)
        );

        // Jurnal RJCFG01 target = baru (seq 1, Slot A) dan hostname default
        let cfg_hdr_sector = tgt.get(spec.cfg_start);
        let hdr =
            asl_router_persistence::RjcfgJournal::deserialize_header_from_sector(&cfg_hdr_sector)
                .unwrap();
        let seq = hdr.sequence_number;
        let active = hdr.active_slot;
        assert_eq!(seq, 1);
        assert_eq!(active, 0);
        let mut slot = [0u8; 4096];
        for i in 0..RJCFG_SLOT_SECTORS {
            let s = tgt.get(spec.cfg_start + RJCFG_SLOT_A_OFFSET + i);
            let o = (i as usize) * SECTOR_SIZE;
            slot[o..o + SECTOR_SIZE].copy_from_slice(&s);
        }
        let j = asl_router_persistence::RjcfgJournal::load_from_disk(&hdr, &slot).unwrap();
        let mut name = [0u8; 32];
        assert_eq!(j.get_hostname_str(&mut name), "rinjani");

        // Jurnal SUMBER tidak pernah disentuh
        assert!(src.writes.is_empty());
        // Target tidak pernah ditulisi di luar region yang diizinkan
        for &w in tgt.writes.iter() {
            let in_head = w < HEAD_SECTORS;
            let in_tail = w >= spec.total - BACKUP_SECTORS;
            let in_esp = w >= spec.esp_start && w < spec.esp_start + report.esp_sectors;
            let in_cfg = w >= spec.cfg_start && w < spec.cfg_start + RJCFG_INIT_SECTORS;
            assert!(
                in_head || in_tail || in_esp || in_cfg,
                "tulis tak terencana di LBA {}",
                w
            );
        }
    }

    // -- F. Injected I/O failures: setiap tahap bisa gagal -------------------

    #[test]
    fn test_35_gagal_tulis_pada_setiap_tahap() {
        let spec = Spec::good();
        let meta = spec.reserved + spec.fats * spec.fat_len;
        let used = meta + 199;
        let cases: [(u64, &'static str); 5] = [
            (10, STAGE_PART_WRITE),                                   // kepala GPT
            (spec.total - 5, STAGE_PART_WRITE),                       // ekor/backup GPT
            (spec.esp_start + 40, STAGE_ARTIFACT),                    // ESP
            (spec.cfg_start + RJCFG_SLOT_A_OFFSET + 2, STAGE_CONFIG), // Slot A jurnal
            (spec.cfg_start, STAGE_CONFIG),                           // header jurnal
        ];
        for (lba, stage) in cases.iter() {
            let mut src = build_source(&spec);
            let mut tgt = MockDisk::new(spec.total);
            tgt.fail_write_at = Some(*lba);
            let r = install_run(
                &mut src,
                &mut tgt,
                "ata0-master",
                "ata0-slave",
                CONFIRM_TOKEN,
                &quiet,
            );
            let e = r.expect_err("instalasi wajib gagal saat tulis ditolak");
            assert_eq!(e.stage, *stage, "LBA {}", lba);
            assert_eq!(src.writes.len(), 0);
        }
        let _ = used;
    }

    #[test]
    fn test_36_readback_mismatch_dan_gagal_baca_sumber() {
        let spec = Spec::good();

        // tulis diterima tapi media menyimpan data rusak -> readback menangkap
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        tgt.corrupt_write_at = Some(spec.esp_start + 30);
        let e = install_run(&mut src, &mut tgt, "s", "t", CONFIRM_TOKEN, &quiet)
            .expect_err("readback mismatch harus menghentikan instalasi");
        assert_eq!(e.stage, STAGE_ARTIFACT);
        assert_eq!(e.reason, "readback byte-mismatch");

        // gagal baca sektor sumber saat menyalin kepala (LBA0 tidak dibaca preflight)
        let mut src = build_source(&spec);
        src.fail_read_at = Some(0);
        let mut tgt = MockDisk::new(spec.total);
        let e = install_run(&mut src, &mut tgt, "s", "t", CONFIRM_TOKEN, &quiet)
            .expect_err("gagal baca sumber harus menghentikan instalasi");
        assert_eq!(e.stage, STAGE_PART_WRITE);
        assert_eq!(e.reason, "baca sektor sumber gagal");
    }

    #[test]
    fn test_37_digest_mismatch_menahan_install_done() {
        // Media jujur saat baca-balik per-sektor (read #1) tetapi mengembalikan
        // data berbeda pada pembacaan verifikasi (read #2) -> digest berbeda.
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        tgt.flip_on_later_read_at = Some(spec.esp_start + 60);
        let e = install_run(&mut src, &mut tgt, "s", "t", CONFIRM_TOKEN, &quiet)
            .expect_err("digest berbeda wajib menolak klaim sukses");
        assert_eq!(e.stage, STAGE_READBACK);
        assert_eq!(
            e.reason,
            "SHA-256 jalur verifikasi != jalur tulis — instalasi TIDAK dipercaya"
        );
    }

    #[test]
    fn test_38_jurnal_sumber_tidak_pernah_disentuh_dan_rjcfg_gagal_baca() {
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        // Sektor Slot A target gagal dibaca ketika diverifikasi -> tahap config gagal
        tgt.fail_read_at = Some(spec.cfg_start + RJCFG_SLOT_A_OFFSET + 3);
        let e = install_run(&mut src, &mut tgt, "s", "t", CONFIRM_TOKEN, &quiet)
            .expect_err("gagal baca slot A target harus menggagalkan tahap config");
        assert_eq!(e.stage, STAGE_CONFIG);
        assert!(src.writes.is_empty());
    }

    #[test]
    fn test_39_mock_lba_di_luar_kapasitas() {
        let mut d = MockDisk::new(1024);
        let mut buf = [0u8; SECTOR_SIZE];
        assert_eq!(
            d.read_sector(1024, &mut buf),
            Err("LBA di luar kapasitas disk")
        );
        assert_eq!(
            d.write_sector(4096, &[1u8; SECTOR_SIZE]),
            Err("LBA di luar kapasitas disk")
        );
        assert_eq!(d.read_sector(0, &mut buf), Ok(()));
        assert!(is_zero_sector(&buf));
    }

    #[test]
    fn test_40_header_backup_target_salah_terdeteksi() {
        // Setelah instalasi sukses, rusak backup header di target lalu jalankan
        // ulang pipeline verifikasi struktural (simulasi media korup pasca-tulis).
        let spec = Spec::good();
        let mut src = build_source(&spec);
        let mut tgt = MockDisk::new(spec.total);
        let report = install_run(&mut src, &mut tgt, "s", "t", CONFIRM_TOKEN, &quiet).unwrap();
        assert_eq!(report.sha_written, report.sha_readback);
        let mut bad = tgt.get(spec.total - 1);
        bad[..8].copy_from_slice(b"NOTEPART");
        tgt.put(spec.total - 1, &bad);
        let info = asl_router_gpt::parse_gpt_header_lba1(&tgt.get(spec.total - 1));
        assert!(
            info.is_none(),
            "backup header target tak boleh lolos parse setelah dirusak"
        );
    }

    #[test]
    fn test_41_array_crc_region_deklaratif_gaya_limine() {
        // Disk bentukan Limine mendeklarasikan 44 entri (bukan 128), sehingga
        // CRC array HARUS dihitung atas region num_entries*entry_size — termasuk
        // kasus sektor terakhir yang hanya sebagian terpakai.
        let mut d = MockDisk::new(8192);
        for s in 2..14u64 {
            let mut sec = [0u8; SECTOR_SIZE];
            for (i, b) in sec.iter_mut().enumerate() {
                *b = ((s as usize * 31 + i * 7) % 251) as u8;
            }
            d.put(s, &sec);
        }
        let crc_over = |n: u64| -> u32 {
            let mut buf: Vec<u8> = Vec::new();
            for s in 2..(2 + (n * 128 + 511) / 512) {
                buf.extend_from_slice(&d.get(s));
            }
            buf.truncate(n as usize * 128);
            let mut state = 0xFFFF_FFFFu32;
            for chunk in buf.chunks(SECTOR_SIZE) {
                state = !asl_router_gpt::crc32_update(state, chunk);
            }
            asl_router_gpt::crc32_update(state, &[])
        };
        let expect44 = crc_over(44);
        let expect45 = crc_over(45);
        assert_eq!(stream_array_crc(&mut d, 2, 44, 128).unwrap(), expect44);
        assert_eq!(stream_array_crc(&mut d, 2, 45, 128).unwrap(), expect45);
        assert_eq!(
            stream_array_crc(&mut d, 2, 0, 128),
            Err("GPT: array entri di luar jendela 32 sektor")
        );
        assert!(stream_array_crc(&mut d, 2, 129, 128).is_err());
    }
}
