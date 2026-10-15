//! Open Core Router (OpenCR) — asl-router-fat32
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Native FAT32 EFI System Partition formatter (no_std, zero-alloc).
//! Fixed geometry: 512B sectors, 1 sector/cluster, 32 reserved, 2 FAT copies,
//! root directory cluster = 2. Supports Long File Names (LFN) required by the
//! Limine boot layout (boot/limine/limine.conf, limine-bios.sys).

#![no_std]

pub const SECTOR_SIZE: usize = 512;
pub const ESP_SECTORS: u64 = 262_144; // 128 MiB — Rinjani GPT partition 1

pub const FAT32_EOC: u32 = 0x0FFF_FFFF;
pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_VOLUME_ID: u8 = 0x08;
pub const ATTR_DIRECTORY: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;
pub const ATTR_LFN: u8 = 0x0F;

pub const MAX_FILES: usize = 24;
pub const MAX_DIRS: usize = 8;
pub const MAX_ENTRIES_PER_DIR: usize = 16; // one cluster of 32-byte entries
pub const MAX_NAME_LEN: usize = 255;

const RESERVED_SECTORS: u64 = 32;
const NUM_FATS: u64 = 2;
const ROOT_CLUSTER: u32 = 2;

// DOS fixed timestamp used for all installer artifacts: 2026-10-15 00:00:00
const DOS_TIME: u16 = 0x0000;
const DOS_DATE: u16 = ((2026 - 1980) << 9) | (10 << 5) | 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatError {
    BufferNotSectorAligned,
    VolumeTooSmall,
    NotEnoughClusters,
    TooManyFiles,
    TooManyDirs,
    TooManyEntries,
    NameTooLong,
    BadPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fat32Stats {
    pub total_sectors: u64,
    pub fat_sectors: u32,
    pub total_clusters: u32,
    pub clusters_used: u32,
}

/// One file to be materialized inside the ESP. `path` uses '/' separators,
/// e.g. "boot/limine/limine.conf". Case is preserved through LFN entries.
#[derive(Debug, Clone, Copy)]
pub struct EspFile<'a> {
    pub path: &'a str,
    pub data: &'a [u8],
}

// -----------------------------------------------------------------------------
// GEOMETRY
// -----------------------------------------------------------------------------

/// Iterative fixed-point: smallest-ish f with f >= g(f), where
/// g(f) = ceil((allocatable(f)+2)*4 / 512). g is decreasing, so the
/// iteration may oscillate between two values; the larger one is valid.
pub fn compute_fat_len(total_sectors: u64) -> u64 {
    let g = |f: u64| -> u64 {
        let allocatable = total_sectors.saturating_sub(RESERVED_SECTORS + NUM_FATS * f);
        ((allocatable + 2) * 4 + 511) / 512
    };
    let mut f = ((total_sectors - RESERVED_SECTORS) * 4 + 511) / 512;
    loop {
        let nf = g(f);
        if nf >= f {
            return nf;
        }
        f = nf;
    }
}

fn cluster_count(total_sectors: u64, fat_len: u64) -> u64 {
    total_sectors.saturating_sub(RESERVED_SECTORS + NUM_FATS * fat_len)
}

const fn cluster_sector(cluster: u32, fat_len: u64) -> usize {
    ((RESERVED_SECTORS + NUM_FATS * fat_len) as usize + (cluster as usize - 2)) * SECTOR_SIZE
}

// -----------------------------------------------------------------------------
// 8.3 SHORT NAME + LFN SUPPORT
// -----------------------------------------------------------------------------

const fn sfn_spaces() -> [u8; 11] {
    [b' '; 11]
}

fn ascii_upper(b: u8) -> u8 {
    if b.is_ascii_lowercase() {
        b - 32
    } else {
        b
    }
}

fn is_valid_83_char(b: u8) -> bool {
    matches!(b,
        b'A'..=b'Z' | b'0'..=b'9' | b'!' | b'#' | b'$' | b'%' | b'&' | b'\''
        | b'(' | b')' | b'-' | b'@' | b'^' | b'_' | b'`' | b'{' | b'}' | b'~')
}

fn sanitize(b: u8) -> u8 {
    let u = ascii_upper(b);
    if is_valid_83_char(u) {
        u
    } else {
        b'_'
    }
}

/// Splits "base.ext" at the LAST dot. Returns (base, ext_without_dot).
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

/// Generates a collision-free 8.3 short name.
/// Returns (sfn_bytes, exact: true when the original name needs no LFN).
fn make_sfn(name: &str, used: &[[u8; 11]], used_count: usize) -> ([u8; 11], bool) {
    let (base, ext) = split_name(name);
    let bbytes = base.as_bytes();
    let ebytes = ext.as_bytes();

    let exact = bbytes.len() <= 8 && ebytes.len() <= 3 && {
        let mut ok = true;
        let mut i = 0;
        while i < bbytes.len() {
            if !is_valid_83_char(ascii_upper(bbytes[i])) {
                ok = false;
            }
            i += 1;
        }
        let mut j = 0;
        while j < ebytes.len() {
            if !is_valid_83_char(ascii_upper(ebytes[j])) {
                ok = false;
            }
            j += 1;
        }
        ok
    };

    if exact {
        let mut s = sfn_spaces();
        let mut i = 0;
        while i < bbytes.len() {
            s[i] = ascii_upper(bbytes[i]);
            i += 1;
        }
        let mut j = 0;
        while j < ebytes.len() {
            s[8 + j] = ascii_upper(ebytes[j]);
            j += 1;
        }
        let mut collision = false;
        let mut k = 0;
        while k < used_count {
            if used[k] == s {
                collision = true;
            }
            k += 1;
        }
        if !collision {
            return (s, true);
        }
    }

    // Truncated "~n" generation
    let stem_len = if bbytes.len() <= 6 { bbytes.len() } else { 6 };
    let ext_take = if ebytes.len() <= 3 { ebytes.len() } else { 3 };
    let mut n = 1u8;
    loop {
        let mut s = sfn_spaces();
        let mut i = 0;
        while i < stem_len {
            s[i] = sanitize(bbytes[i]);
            i += 1;
        }
        // "~n" occupies positions stem_len..stem_len+2 (guaranteed <= 8)
        s[stem_len] = b'~';
        if n < 10 {
            s[stem_len + 1] = b'0' + n;
        } else {
            s[stem_len + 1] = b'0' + n / 10;
            // two-digit counters beyond 8.3 window are not representable; n>9
            // is impossible for our bounded directory sizes.
        }
        let mut j = 0;
        while j < ext_take {
            s[8 + j] = sanitize(ebytes[j]);
            j += 1;
        }
        let mut collision = false;
        let mut k = 0;
        while k < used_count {
            if used[k] == s {
                collision = true;
            }
            k += 1;
        }
        if !collision {
            return (s, false);
        }
        n += 1;
        if n > 9 {
            // Deterministic fallback: cannot happen within bounded tables.
            return (s, false);
        }
    }
}

/// Microsoft LFN checksum over an 11-byte short name.
pub fn lfn_checksum(sfn: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    let mut i = 0;
    while i < 11 {
        let lsb = sum & 1;
        sum = (sum >> 1) | (lsb << 7);
        sum = sum.wrapping_add(sfn[i]);
        i += 1;
    }
    sum
}

/// UTF-16 code units of a name (ASCII-only here) into a stack buffer.
fn name_to_utf16(name: &str, out: &mut [u16; MAX_NAME_LEN]) -> usize {
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() && i < MAX_NAME_LEN {
        out[i] = b[i] as u16;
        i += 1;
    }
    i
}

fn lfn_entry_count(name_len: usize) -> usize {
    (name_len + 13 - 1) / 13
}

/// Writes LFN entries for `name` into dir buffer `entries` starting at slot
/// `start` (each slot 32 bytes). Returns number of slots written.
fn write_lfn_entries(entries: &mut [u8], start_slot: usize, name: &str, checksum: u8) -> usize {
    let mut u16buf = [0u16; MAX_NAME_LEN];
    let len = name_to_utf16(name, &mut u16buf);
    let count = lfn_entry_count(len);
    let mut ordinal = count;
    while ordinal > 0 {
        let slot = start_slot + (count - ordinal);
        let base = slot * 32;
        for k in 0..32 {
            entries[base + k] = 0;
        }
        entries[base + 11] = ATTR_LFN;
        entries[base + 12] = 0; // reserved
        entries[base + 13] = checksum;
        let mut seq = ordinal as u8;
        if ordinal == count {
            seq |= 0x40; // last in sequence
        }
        entries[base] = seq;
        // Character offsets within an LFN entry, in order:
        // 1,3,5,7,9,11,13,15,17,19,21 (5 u16 pairs region A) then 28,30
        let offsets: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        let mut i = 0;
        while i < 13 {
            let idx = (ordinal - 1) * 13 + i;
            let mut unit: u16 = if idx < len {
                u16buf[idx]
            } else if idx == len {
                0x0000 // end-of-name marker
            } else {
                0xFFFF // padding
            };
            // For the final (lowest ordinal) entry: pad after terminator
            if ordinal == 1 && idx > len {
                unit = 0xFFFF;
            }
            let at = base + offsets[i];
            entries[at] = (unit & 0xFF) as u8;
            entries[at + 1] = (unit >> 8) as u8;
            i += 1;
        }
        ordinal -= 1;
    }
    count
}

// -----------------------------------------------------------------------------
// INTERNAL TABLES
// -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct DirRec<'a> {
    name: &'a str,     // last path segment ("" for root)
    parent: usize,     // index into dirs, usize::MAX for root
    sfn: [u8; 11],
    cluster: u32,
}

#[derive(Clone, Copy)]
struct FileRec<'a> {
    lfn: &'a str, // file name segment
    dir: usize,
    sfn: [u8; 11],
    exact_sfn: bool,
    first_cluster: u32,
    size: u32,
}

fn dir_index_of<'a>(dirs: &mut [DirRec<'a>; MAX_DIRS], ndirs: &mut usize, path_dir: &'a str, files: &[EspFile<'a>]) -> Option<usize> {
    // Ensure all ancestor dirs exist for the directory path string (may be "")
    let mut parent = 0usize; // children of the root live at dir index 0
    if path_dir.is_empty() {
        return Some(0);
    }
    let mut rest = path_dir;
    while let Some(dot) = rest.find('/') {
        let seg = &rest[..dot];
        rest = &rest[dot + 1..];
        if seg.is_empty() {
            return None;
        }
        parent = find_or_add_dir(dirs, ndirs, seg, parent, files)?;
    }
    if !rest.is_empty() {
        parent = find_or_add_dir(dirs, ndirs, rest, parent, files)?;
    }
    Some(parent)
}

fn find_or_add_dir<'a>(
    dirs: &mut [DirRec<'a>; MAX_DIRS],
    ndirs: &mut usize,
    seg: &'a str,
    parent: usize,
    _files: &[EspFile<'a>],
) -> Option<usize> {
    if seg.is_empty() || seg.len() > MAX_NAME_LEN {
        return None;
    }
    let mut i = 0;
    while i < *ndirs {
        if dirs[i].name == seg && dirs[i].parent == parent {
            return Some(i);
        }
        i += 1;
    }
    if *ndirs >= MAX_DIRS {
        return None;
    }
    dirs[*ndirs] = DirRec { name: seg, parent, sfn: sfn_spaces(), cluster: 0 };
    let idx = *ndirs;
    *ndirs += 1;
    Some(idx)
}

// -----------------------------------------------------------------------------
// BOOT SECTOR / FSINFO / FAT WRITERS
// -----------------------------------------------------------------------------

fn put_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn write_boot_sector(buf: &mut [u8], geom: &Fat32Stats) {
    let s = &mut buf[0..SECTOR_SIZE];
    for x in s.iter_mut() {
        *x = 0;
    }
    s[0] = 0xEB;
    s[1] = 0x58;
    s[2] = 0x90;
    s[3..11].copy_from_slice(b"MSWIN4.1");
    put_u16(s, 11, 512);              // bytes per sector
    s[13] = 1;                        // sectors per cluster
    put_u16(s, 14, RESERVED_SECTORS as u16);
    s[16] = NUM_FATS as u8;
    put_u16(s, 17, 0);                // root entry count (0 for FAT32)
    put_u16(s, 19, 0);                // total sectors 16
    s[21] = 0xF8;                     // media descriptor
    put_u16(s, 22, 0);                // FAT size 16 (0 for FAT32)
    put_u16(s, 24, 63);               // sectors per track
    put_u16(s, 26, 255);              // heads
    put_u32(s, 28, 0);                // hidden sectors
    put_u32(s, 32, geom.total_sectors as u32);
    put_u32(s, 36, geom.fat_sectors); // FAT size 32
    put_u16(s, 40, 0);                // ext flags
    put_u16(s, 42, 0);                // FS version
    put_u32(s, 44, ROOT_CLUSTER);     // root cluster
    put_u16(s, 48, 1);                // FSInfo sector
    put_u16(s, 50, 6);                // backup boot sector
    s[64] = 0x80;                     // drive number
    s[66] = 0x29;                     // boot signature
    put_u32(s, 67, 0x524A_4E31);      // volume serial "RJN1"
    s[71..82].copy_from_slice(b"RINJANI ESP");
    s[82..90].copy_from_slice(b"FAT32   ");
    s[510] = 0x55;
    s[511] = 0xAA;
}

fn write_fsinfo(buf: &mut [u8], geom: &Fat32Stats) {
    let s = &mut buf[SECTOR_SIZE..2 * SECTOR_SIZE];
    for x in s.iter_mut() {
        *x = 0;
    }
    put_u32(s, 0, 0x4161_5252); // LeadSig
    put_u32(s, 484, 0x6141_7261); // StrucSig
    put_u32(s, 488, geom.total_clusters - geom.clusters_used); // free count
    put_u32(s, 492, geom.clusters_used + 2); // next free hint
    s[508] = 0x55;
    s[509] = 0xAA;
    s[510] = 0x55;
    s[511] = 0xAA;
}

fn fat_entry_off(fat: u64, cluster: u32, geom: &Fat32Stats) -> usize {
    ((RESERVED_SECTORS + fat * geom.fat_sectors as u64) as usize * SECTOR_SIZE) + (cluster as usize * 4)
}

fn write_fat_entries(buf: &mut [u8], geom: &Fat32Stats, cluster: u32, value: u32) {
    for fat in 0..NUM_FATS {
        let at = fat_entry_off(fat, cluster, geom);
        put_u32(&mut buf[at..at + 4], 0, value & 0x0FFF_FFFF);
    }
}

// -----------------------------------------------------------------------------
// DIRECTORY ENTRY
// -----------------------------------------------------------------------------

fn write_sfn_entry(
    entries: &mut [u8],
    slot: usize,
    sfn: &[u8; 11],
    attr: u8,
    first_cluster: u32,
    size: u32,
) {
    let base = slot * 32;
    entries[base..base + 11].copy_from_slice(sfn);
    entries[base + 11] = attr;
    put_u16(entries, base + 14, DOS_TIME);
    put_u16(entries, base + 16, DOS_DATE);
    put_u16(entries, base + 20, (first_cluster >> 16) as u16); // FAT32 high cluster
    put_u16(entries, base + 26, first_cluster as u16);
    put_u32(entries, base + 28, size);
}

// -----------------------------------------------------------------------------
// PUBLIC API
// -----------------------------------------------------------------------------

pub fn format_esp<'a>(buf: &mut [u8], files: &[EspFile<'a>]) -> Result<Fat32Stats, FatError> {
    if buf.len() % SECTOR_SIZE != 0 {
        return Err(FatError::BufferNotSectorAligned);
    }
    let total_sectors = (buf.len() / SECTOR_SIZE) as u64;
    if total_sectors < 2 * ESP_SECTORS / 4 {
        return Err(FatError::VolumeTooSmall);
    }
    if files.len() > MAX_FILES {
        return Err(FatError::TooManyFiles);
    }

    let fat_len = compute_fat_len(total_sectors);
    let clusters = cluster_count(total_sectors, fat_len);
    if clusters < 65_525 {
        return Err(FatError::VolumeTooSmall);
    }
    let mut geom = Fat32Stats {
        total_sectors,
        fat_sectors: fat_len as u32,
        total_clusters: clusters as u32,
        clusters_used: 0,
    };

    // --- Parse paths, build dir table -------------------------------------
    let mut dirs = [DirRec { name: "", parent: usize::MAX, sfn: sfn_spaces(), cluster: 0 }; MAX_DIRS];
    dirs[0].cluster = ROOT_CLUSTER;
    let mut ndirs = 1usize;
    let mut frecs = [const {
        FileRec {
            lfn: "",
            dir: 0,
            sfn: sfn_spaces(),
            exact_sfn: false,
            first_cluster: 0,
            size: 0,
        }
    }; MAX_FILES];
    let mut nfiles = 0usize;

    for f in files {
        if f.path.is_empty() || f.path.starts_with('/') || f.path.ends_with('/') || f.path.contains("//") {
            return Err(FatError::BadPath);
        }
        let slash = f.path.rfind('/');
        let (dirpart, name) = match slash {
            Some(i) => (&f.path[..i], &f.path[i + 1..]),
            None => ("", f.path),
        };
        if name.is_empty() || name.len() > MAX_NAME_LEN || name == "." || name == ".." {
            return Err(FatError::BadPath);
        }
        let d = dir_index_of(&mut dirs, &mut ndirs, dirpart, files).ok_or(FatError::BadPath)?;
        frecs[nfiles] = FileRec {
            lfn: name,
            dir: d,
            sfn: sfn_spaces(),
            exact_sfn: false,
            first_cluster: 0,
            size: f.data.len() as u32,
        };
        nfiles += 1;
    }

    // --- Assign short names (collision-aware, deterministic) ---------------
    // Directories first (discovery order), then files (input order).
    let mut d = 1usize;
    while d < ndirs {
        let parent = dirs[d].parent;
        let mut used = [sfn_spaces(); MAX_ENTRIES_PER_DIR];
        let mut nused = 0usize;
        let mut k = 1usize;
        while k < d {
            if dirs[k].parent == parent {
                used[nused] = dirs[k].sfn;
                nused += 1;
            }
            k += 1;
        }
        let (s, _exact) = make_sfn(dirs[d].name, &used, nused);
        dirs[d].sfn = s;
        d += 1;
    }

    // --- Cluster allocation -------------------------------------------------
    let mut next_cluster: u32 = ROOT_CLUSTER + 1;
    let mut k = 1usize;
    while k < ndirs {
        dirs[k].cluster = next_cluster;
        next_cluster += 1;
        k += 1;
    }
    let mut fi = 0usize;
    while fi < nfiles {
        let need = (frecs[fi].size as usize + SECTOR_SIZE - 1) / SECTOR_SIZE;
        if need == 0 {
            frecs[fi].first_cluster = 0;
        } else {
            frecs[fi].first_cluster = next_cluster;
            next_cluster += need as u32;
        }
        // short name for the file inside its directory
        let mut used = [sfn_spaces(); MAX_ENTRIES_PER_DIR];
        let mut nused = 0usize;
        let mut k = 1usize;
        while k < ndirs {
            if dirs[k].parent == frecs[fi].dir {
                used[nused] = dirs[k].sfn;
                nused += 1;
            }
            k += 1;
        }
        let mut j = 0usize;
        while j < fi {
            if frecs[j].dir == frecs[fi].dir {
                used[nused] = frecs[j].sfn;
                nused += 1;
            }
            j += 1;
        }
        let (s, exact) = make_sfn(frecs[fi].lfn, &used, nused);
        frecs[fi].sfn = s;
        frecs[fi].exact_sfn = exact;
        fi += 1;
    }
    if next_cluster - 2 > geom.total_clusters {
        return Err(FatError::NotEnoughClusters);
    }
    geom.clusters_used = next_cluster - 2;

    // Entry-count guard per directory
    let mut dchk = 0usize;
    while dchk < ndirs {
        let mut slots = 2usize; // . and ..
        let mut k = 1usize;
        while k < ndirs {
            if dirs[k].parent == dchk {
                slots += 1;
            }
            k += 1;
        }
        let mut j = 0usize;
        while j < nfiles {
            if frecs[j].dir == dchk {
                slots += 1;
                if !frecs[j].exact_sfn {
                    slots += lfn_entry_count(frecs[j].lfn.len());
                }
            }
            j += 1;
        }
        if slots > MAX_ENTRIES_PER_DIR {
            return Err(FatError::TooManyEntries);
        }
        dchk += 1;
    }

    // --- Write boot sector / FSInfo / backup ---------------------------------
    let buflen = buf.len();
    for byte in buf[..buflen].iter_mut() {
        *byte = 0;
    }
    write_boot_sector(buf, &geom);
    write_fsinfo(buf, &geom);
    let mut boot_copy = [0u8; SECTOR_SIZE];
    boot_copy.copy_from_slice(&buf[0..SECTOR_SIZE]);
    buf[6 * SECTOR_SIZE..7 * SECTOR_SIZE].copy_from_slice(&boot_copy);

    // --- FAT special entries --------------------------------------------------
    write_fat_entries(buf, &geom, 0, 0x0FFF_FFF8);
    write_fat_entries(buf, &geom, 1, FAT32_EOC);

    // --- File data + chains ----------------------------------------------------
    let mut fidx = 0usize;
    while fidx < nfiles {
        let need = (frecs[fidx].size as usize + SECTOR_SIZE - 1) / SECTOR_SIZE;
        if need > 0 {
            let start = frecs[fidx].first_cluster;
            let mut c = 0usize;
            while c < need {
                let cluster = start + c as u32;
                let off = cluster_sector(cluster, fat_len);
                let slice_from = c * SECTOR_SIZE;
                let slice_to = ((c + 1) * SECTOR_SIZE).min(frecs[fidx].size as usize);
                let src = files[fidx].data;
                buf[off..off + (slice_to - slice_from)].copy_from_slice(&src[slice_from..slice_to]);
                let next_val = if c + 1 < need {
                    (start + c as u32 + 1) | 0x0000_0000
                } else {
                    FAT32_EOC
                };
                write_fat_entries(buf, &geom, cluster, next_val);
                c += 1;
            }
        }
        fidx += 1;
    }

    // --- Directory clusters ---------------------------------------------------
    let mut d = 0usize;
    while d < ndirs {
        let cluster = dirs[d].cluster;
        let off = cluster_sector(cluster, fat_len);
        write_fat_entries(buf, &geom, cluster, FAT32_EOC);
        let dsectors = &mut buf[off..off + SECTOR_SIZE];
        // we re-borrow: use raw slice for entries
        let parent_cluster = if d == 0 { 0 } else { dirs[dirs[d].parent].cluster };
        let self_cluster = cluster;

        // '.' entry (0x2E + 10 spaces per MS spec)
        dsectors[0] = b'.';
        dsectors[1..11].copy_from_slice(&sfn_spaces()[0..10]);
        dsectors[11] = ATTR_DIRECTORY;
        put_u16(dsectors, 14, DOS_TIME);
        put_u16(dsectors, 16, DOS_DATE);
        put_u16(dsectors, 20, (self_cluster >> 16) as u16);
        put_u16(dsectors, 26, self_cluster as u16);
        // '..' entry (0x2E 0x2E + 9 spaces)
        dsectors[32] = b'.';
        dsectors[33] = b'.';
        dsectors[34..43].copy_from_slice(&sfn_spaces()[0..9]);
        dsectors[32 + 11] = ATTR_DIRECTORY;
        put_u16(dsectors, 32 + 14, DOS_TIME);
        put_u16(dsectors, 32 + 16, DOS_DATE);
        put_u16(dsectors, 32 + 20, (parent_cluster >> 16) as u16);
        put_u16(dsectors, 32 + 26, parent_cluster as u16);

        let mut slot = 2usize;
        let mut k = 1usize;
        while k < ndirs {
            if dirs[k].parent == d {
                write_sfn_entry(dsectors, slot, &dirs[k].sfn, ATTR_DIRECTORY, dirs[k].cluster, 0);
                slot += 1;
            }
            k += 1;
        }
        let mut j = 0usize;
        while j < nfiles {
            if frecs[j].dir == d {
                if !frecs[j].exact_sfn {
                    let chk = lfn_checksum(&frecs[j].sfn);
                    let used = write_lfn_entries(dsectors, slot, frecs[j].lfn, chk);
                    slot += used;
                }
                write_sfn_entry(dsectors, slot, &frecs[j].sfn, ATTR_ARCHIVE, frecs[j].first_cluster, frecs[j].size);
                slot += 1;
            }
            j += 1;
        }
        d += 1;
    }

    Ok(geom)
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    // -----------------------------------------------------------------------
    // Test-side FAT32 mini reader (independent re-parse of the formatted image)
    // -----------------------------------------------------------------------

    struct Reader {
        buf: Vec<u8>,
        fat_len: u32,
    }

    impl Reader {
        fn open(buf: Vec<u8>) -> Self {
            let fat_len = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);
            Self { buf, fat_len }
        }

        fn cluster_off(&self, cluster: u32) -> usize {
            ((RESERVED_SECTORS + NUM_FATS * self.fat_len as u64) as usize
                + (cluster as usize - 2)) * SECTOR_SIZE
        }

        fn fat_get(&self, cluster: u32) -> u32 {
            let at = (RESERVED_SECTORS as usize) * SECTOR_SIZE + (cluster as usize * 4);
            u32::from_le_bytes(self.buf[at..at + 4].try_into().unwrap()) & 0x0FFF_FFFF
        }

        fn chain(&self, mut cluster: u32) -> Vec<u32> {
            let mut out = Vec::new();
            while cluster >= 2 && cluster < FAT32_EOC {
                out.push(cluster);
                if out.len() > 8192 {
                    break;
                }
                cluster = self.fat_get(cluster);
            }
            out
        }

        fn read_clusters(&self, chain: &[u32], bytes: usize) -> Vec<u8> {
            let mut out = Vec::new();
            for c in chain {
                let off = self.cluster_off(*c);
                out.extend_from_slice(&self.buf[off..off + SECTOR_SIZE]);
            }
            out.truncate(bytes);
            out
        }

        fn root_chain(&self) -> Vec<u32> {
            let root = u32::from_le_bytes(self.buf[44..48].try_into().unwrap());
            vec![root]
        }

        fn dir_cluster_at(&self, dir_chain: &[u32], tag: &[u8], attr: u8) -> Option<u32> {
            let dirb = self.read_clusters(dir_chain, dir_chain.len() * SECTOR_SIZE);
            let mut i = 0;
            while i + 32 <= dirb.len() {
                if dirb[i] == 0 {
                    break;
                }
                if dirb[i + 11] == attr && &dirb[i..i + tag.len()] == tag {
                    return Some(u16::from_le_bytes([dirb[i + 26], dirb[i + 27]]) as u32);
                }
                i += 32;
            }
            None
        }

        /// List regular files in one directory cluster chain, reconstructing
        /// LFN names; returns (dir_path + name, bytes).
        fn list_files(&self, dir_path: &str, chain_root: &[u32]) -> Vec<(String, Vec<u8>)> {
            let dirbytes = self.read_clusters(chain_root, chain_root.len() * SECTOR_SIZE);
            let mut results = Vec::new();
            let mut i = 0;
            let mut chunks: Vec<(u8, String)> = Vec::new();
            while i + 32 <= dirbytes.len() {
                let e = &dirbytes[i..i + 32];
                if e[0] == 0 {
                    break;
                }
                if e[11] == ATTR_LFN {
                    let offs = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
                    let mut chunk = Vec::new();
                    for o in offs {
                        let u = u16::from_le_bytes([e[o], e[o + 1]]);
                        if u != 0 && u != 0xFFFF {
                            chunk.push(u);
                        }
                    }
                    let s: String = chunk.iter().filter_map(|&c| char::from_u32(c as u32)).collect();
                    chunks.push((e[0] & 0x3F, s));
                    i += 32;
                    continue;
                }
                if e[11] == ATTR_DIRECTORY || (e[11] & ATTR_VOLUME_ID) != 0 {
                    chunks.clear();
                    i += 32;
                    continue;
                }
                let hi = u16::from_le_bytes([e[20], e[21]]) as u32;
                let lo = u16::from_le_bytes([e[26], e[27]]) as u32;
                let first = (hi << 16) | lo;
                let size = u32::from_le_bytes([e[28], e[29], e[30], e[31]]) as usize;
                let name = if chunks.is_empty() {
                    let base = core::str::from_utf8(&e[0..8]).unwrap_or("").trim();
                    let ext = core::str::from_utf8(&e[8..11]).unwrap_or("").trim();
                    if ext.is_empty() { base.to_string() } else { format!("{}.{}", base, ext) }
                } else {
                    chunks.sort_by_key(|c| c.0);
                    chunks.iter().map(|(_, s)| s.as_str()).collect::<String>()
                };
                chunks.clear();
                let data = self.read_clusters(&self.chain(first), size);
                let full = if dir_path.is_empty() { name } else { format!("{}/{}", dir_path, name) };
                results.push((full, data));
                i += 32;
            }
            results
        }
    }

    fn esp_buf() -> Vec<u8> {
        vec![0u8; ESP_SECTORS as usize * SECTOR_SIZE]
    }

    #[test]
    fn fat_test_01_roundtrip_three_files_with_lfn() {
        let payload_a = b"limine probe content for conf file".to_vec();
        let payload_b: Vec<u8> = (0u32..9000).map(|i| (i % 251) as u8).collect();
        let mut buf = esp_buf();
        let files = [
            EspFile { path: "boot/limine/limine.conf", data: &payload_a },
            EspFile { path: "boot/rinjani-kernel.elf", data: &payload_b },
            EspFile { path: "boot/limine/limine-bios.sys", data: &payload_a },
        ];
        let stats = format_esp(&mut buf, &files).expect("format ok");
        assert!(stats.total_clusters >= 65_525);

        let r = Reader::open(buf);
        let boot = r.dir_cluster_at(&r.root_chain(), b"BOOT", ATTR_DIRECTORY)
            .expect("BOOT dir entry");
        let boot_chain = r.chain(boot);
        let mut all = r.list_files("boot", &boot_chain);
        let limine = r.dir_cluster_at(&boot_chain, b"LIMINE", ATTR_DIRECTORY)
            .expect("LIMINE dir entry");
        all.extend(r.list_files("boot/limine", &r.chain(limine)));

        assert_eq!(all.len(), 3, "names: {:?}", all.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
        for (name, data) in all.iter() {
            match name.as_str() {
                "boot/limine/limine.conf" | "boot/limine/limine-bios.sys" => {
                    assert_eq!(data.as_slice(), payload_a.as_slice());
                }
                "boot/rinjani-kernel.elf" => {
                    assert_eq!(data.as_slice(), payload_b.as_slice());
                }
                other => panic!("unexpected file {}", other),
            }
        }
    }

    #[test]
    fn fat_test_02_bpb_fields() {
        let mut buf = esp_buf();
        let files = [EspFile { path: "boot/x.txt", data: b"hi" }];
        let stats = format_esp(&mut buf, &files).unwrap();
        assert_eq!(&buf[3..11], b"MSWIN4.1");
        assert_eq!(u16::from_le_bytes([buf[11], buf[12]]), 512);
        assert_eq!(buf[13], 1);
        assert_eq!(u16::from_le_bytes([buf[14], buf[15]]), 32);
        assert_eq!(buf[16], 2);
        assert_eq!(buf[21], 0xF8);
        assert_eq!(u32::from_le_bytes(buf[32..36].try_into().unwrap()), ESP_SECTORS as u32);
        assert_eq!(u32::from_le_bytes(buf[44..48].try_into().unwrap()), 2);
        assert_eq!(&buf[82..87], b"FAT32");
        assert_eq!(buf[510], 0x55);
        assert_eq!(buf[511], 0xAA);
        assert_eq!(buf[6 * SECTOR_SIZE + 511], 0xAA);
        assert_eq!(u32::from_le_bytes(buf[SECTOR_SIZE..SECTOR_SIZE + 4].try_into().unwrap()), 0x4161_5252);
        assert_eq!(stats.fat_sectors as u64, compute_fat_len(ESP_SECTORS));
    }

    #[test]
    fn fat_test_03_lfn_checksum_microsoft_vector() {
        // Reference value from an independent implementation of the MS
        // "FAT: Generality" checksum pseudocode over SFN bytes TEXTFILETXT.
        let sfn = *b"TEXTFILETXT";
        assert_eq!(lfn_checksum(&sfn), 0x79);
        // Deterministic cross-check: rotate-add over bytes equals reference loop.
        let mut reference: u8 = 0;
        for b in &sfn {
            let lsb = reference & 1;
            reference = (reference >> 1) + (lsb << 7);
            reference = reference.wrapping_add(*b);
        }
        assert_eq!(lfn_checksum(&sfn), reference);
    }

    #[test]
    fn fat_test_04_sfn_generation() {
        let used = [sfn_spaces(); MAX_ENTRIES_PER_DIR];
        let (s, exact) = make_sfn("boot", &used, 0);
        assert!(exact);
        assert_eq!(&s[..], b"BOOT       ");
        let (s2, exact2) = make_sfn("limine-bios.sys", &used, 0);
        assert!(!exact2);
        assert_eq!(&s2[..], b"LIMINE~1SYS");
        let mut used2 = used;
        used2[0] = s2;
        let (s3, _) = make_sfn("limine.conf", &used2, 1);
        assert_eq!(&s3[..], b"LIMINE~1CON");
    }

    #[test]
    fn fat_test_05_geometry_math() {
        let f = compute_fat_len(ESP_SECTORS);
        let clusters = cluster_count(ESP_SECTORS, f);
        assert!((clusters + 2) * 4 <= f * 512);
        assert!(f >= 2016 && f <= 2100, "fat_len={}", f);
        assert!(clusters >= 65_525);
        // total accounting must fill the volume: reserved + 2*FAT + clusters (+2 slack) <= total
        assert!(RESERVED_SECTORS + NUM_FATS * f + clusters <= ESP_SECTORS);
    }

    #[test]
    fn fat_test_06_bad_paths_rejected() {
        let mut buf = esp_buf();
        let f = [EspFile { path: "/abs/path.txt", data: b"x" }];
        assert_eq!(format_esp(&mut buf, &f), Err(FatError::BadPath));
        let f2 = [EspFile { path: "trailing/", data: b"x" }];
        assert_eq!(format_esp(&mut buf, &f2), Err(FatError::BadPath));
        let small = [EspFile { path: "a.txt", data: b"x" }];
        let mut tiny = vec![0u8; 8 * SECTOR_SIZE];
        assert_eq!(format_esp(&mut tiny, &small), Err(FatError::VolumeTooSmall));
        let mut misaligned = vec![0u8; ESP_SECTORS as usize * SECTOR_SIZE + 3];
        assert_eq!(format_esp(&mut misaligned, &small), Err(FatError::BufferNotSectorAligned));
    }

    #[test]
    fn fat_test_07_multisector_file_chain() {
        let payload: Vec<u8> = (0u32..4096).map(|i| (i % 256) as u8).collect();
        let mut buf = esp_buf();
        let files = [EspFile { path: "boot/big.bin", data: &payload }];
        format_esp(&mut buf, &files).unwrap();
        let r = Reader::open(buf);
        let boot = r.dir_cluster_at(&r.root_chain(), b"BOOT", ATTR_DIRECTORY).unwrap();
        let boot_chain = r.chain(boot);
        // locate via LFN list instead of SFN tag
        let listed = r.list_files("boot", &boot_chain);
        assert_eq!(listed.len(), 1, "names: {:?}", listed.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
        assert_eq!(listed[0].0, "boot/BIG.BIN");
        assert_eq!(listed[0].1, payload);
    }
}
