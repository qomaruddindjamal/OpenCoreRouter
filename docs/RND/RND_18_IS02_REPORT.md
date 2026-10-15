# LAPORAN SPRINT IS-02: BLOCK DEVICE NYATA (ATA PIO), PERSISTENSI RJCFG01 DI DISK & GATE ARP HOST

> **Document ID:** `docs/RND/RND_18_IS02_REPORT.md`  
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)  
> **Date:** 2026-10-15  
> **Status:** `RUNTIME VERIFIED & PASS-LOCKED` (dengan 1 gate jujur: self-installer masih `NOT RUN`)  
> **Author:** Kanoman AI di bawah supervisi Anti Gravity (sebagian modul dikerjakan mitra agent, direview diff + gate diulang oleh Kanoman)  
> **Baseline Commit:** `0e9dd92` (OpenCR)  
> **Doktrina:** ASL CORE BUKAN LINUX — driver ATA PIO, parser GPT, dan format disk RJCFG01 ditulis native `#![no_std]` dari spesifikasi hardware (ATA/ATAPI-8, UEFI 2.10), bukan porting kode sumber Linux/FreeBSD. Tidak ada kode OS eksternal yang masuk repositori.

---

## 1. Ringkasan Eksekutif

Sprint IS-02 mengeksekusi poin 1–4 perintah pemilik dengan gate keras "host rinjani harus terlihat di table ARP":

1. **Block device nyata (`VERIFIED`):** Karena VirtualBox tidak mengekspos virtio-blk PCI ke guest, sprint ini membangun **driver ATA/IDE PIO native** (crate baru `asl-router-drivers-ata`) yang mem-probe disk virtual nyata via port 0x1F0/0x3F6: `VBOX HARDDISK`, IDENTIFY sukses, **kapasitas nyata 2 GiB = 4.194.304 sektor** dibaca dari drive, bukan angka karangan.
2. **Koreksi kejujuran data lama (`VERIFIED`):** Klaim "[R22] 20 GiB" pada sprint-sprint sebelumnya adalah **data tersimulasi (fabricated)** — fakta `showhdinfo` dan IDENTIFY drive hari ini: VDI hanya 2048 MiB. Sejak build ini kernel hanya melaporkan kapasitas yang dibaca langsung dari disk.
3. **Verifikasi GPT nyata dari disk (`VERIFIED`):** Header LBA1 dibaca via PIO, signature `EFI PART` OK, CRC32 header terverifikasi (`crc_ok=YA`), array entri LBA2 di-parse: 44 entri, dan partisi **RJCFG01 nyata terdeteksi di LBA 262178..278561**.
4. **Persistensi RJCFG01 on-disk (`VERIFIED`):** Format on-disk journal (header sektor-0 + Slot A/B 4 KiB, CRC32) diimplementasikan di `asl-router-persistence`; boot path `[R26]` membaca journal dari disk, `[R26c]`/`config save` menulis commit A/B ke disk (header ditulis LAST) lalu **baca-balik verifikasi CRC**. Bukti lintas-boot: boot #1 menulis seq=2 Slot B → boot #2 memuat `slot 1 seq=2 DIBACA DARI DISK`, `config show` menampilkan Slot B (seq=2) dari keadaan disk.
5. **Gate ARP host (`VERIFIED`):** Setelah seluruh perubahan blok, `arp -a` host Windows tetap menunjukkan `192.168.56.100 → 08-00-27-dd-48-91 dynamic`; ping 3/3 reply 0% loss. Gate pemilik **TERPENUHI**.
6. **Power-off ACPI (`VERIFIED`, poin 2):** Handler PM1_CNT via FADT/Limine HHDM sudah masuk trunk (`7df20e2`); log `is02_poweroff_probe7b.log` mencatat tulis `SLP_EN|S5` (outw 0x4004=0x2400) dan sesi VM langsung berakhir. CLI `shutdown` tersedia di rinjani-cli.
7. **Push remote (poin 4, lihat §9):** commit pendahulu IS-02 sudah berada di `origin/main`; commit penutup sprint ini di-push setelah gate §2 hijau.

---

## 2. Artefak & Hash

| Artefak | Nilai |
| :--- | :--- |
| Kernel ELF `target/x86_64-unknown-none/debug/asl-kernel` | SHA256 `43bcbd5d…3cddd9`, 1.836.040 B (build 14:31, yang dieksekusi VM) |
| Disk raw `target/rinjani-1.0.0-x86_64-disk.raw` | SHA256 `5562a490…db34d592`, 2 GiB — berisi byte-for-byte kernel ELF di ESP (dicari & ditemukan full-match di offset 2.100.736) |
| VDI medium VM | UUID registry `aedff65b…` disinkronkan ulang ke file (insiden §5.4) |
| Unit test workspace | `total passed: 214` (dari 196 pada IS-01; +18: driver ATA, parser entri/header GPT, format disk RJCFG01), 0 failed, 62 suite `ok` |
| Gate no-std | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` → `Finished`, 0 error |

---

## 3. Bangunan Baru Sprint Ini

### 3.1 Driver ATA/IDE PIO — `router/asl-router-drivers-ata/` (CRATE BARU, `#![no_std]`)
- Kanal primary 0x1F0–0x1F7 + alt-status 0x3F6; reset SRST + poll BSY/DRDY + klarifikasi latch ERR + retry sekali (CHS→LBA select) — lewat jalur inilah probe ABRT pada `is02_ata1.log` berhasil jadi OK pada `is02_ata2.log`.
- `probe()` — soft reset, deteksi bus mengambang (st 0x00/0xFF), tolak ATAPI (signature 0x14EB), IDENTIFY 0xEC, ekstrak model (bytes 27–46 byte-swapped), bit LBA (word 47), kapasitas (words 60–61).
- `read_sector`/`write_sector` LBA28 (0x20/0x30) 16-bit PIO 256 word + `CACHE_FLUSH` 0xE7 setelah tulis; delay 400 ns via baca alt-status.
- `is_gpt_header()` untuk verifikasi `EFI PART` di LBA1. 4 unit tests (host stub, tanpa port I/O).

### 3.2 Parser GPT on-disk — `router/asl-router-gpt/src/lib.rs` (MODIFIKASI)
- `parse_gpt_header_lba1()` — termasuk **verifikasi CRC32 header** (field CRC dinolkan sesuai spec) → `header_crc_ok`.
- `parse_gpt_entry()` — offset benar start/end LBA (32/40 — sebelumnya salah 16/24, lihat §5.3), name UTF-16LE→ASCII, deteksi entri used via type GUID null.
- Round-trip test terhadap serializer IS-01 (`write_head`/`entry_array_bytes`).

### 3.3 Format disk RJCFG01 — `router/asl-router-persistence/src/lib.rs` (MODIFIKASI, modul dikerjakan mitra agent, direview)
- Konstanta layout: header = sektor 0 partisi; Slot A = sektor 8..16; Slot B = 16..24; `slot_first_sector()`.
- `serialize_header_to_sector` / `deserialize_header_from_sector` (validasi magic + versi + header CRC, pesan error diskriminatif) / `load_from_disk` (validasi CRC slot aktif).
- 8 unit tests round-trip; tidak ada perubahan API in-memory yang dipakai CLI.

### 3.4 Integrasi boot — `asl-kernel/asl-kernel-bin/src/main.rs`
- `[R22]` kini percabangan nyata: device virtio-blk PCI bila ada, **else driver ATA probe**; tanpa disk nyata kernel melaporkan fallback secara jujur (tanpa angka kapasitas karangan).
- `[R23b/c]` baca LBA1 + array entri dari disk, deteksi `RJCFG01`, set `rjcfg01_lba`.
- `[R26]` load journal dari disk saat boot (header → slot aktif → `load_from_disk`); `[R26c]` auto-persist sekali bila disk ada tapi journal belum pernah ditulis; `config save` CLI menulis slot → header (LAST) → **readback verifikasi** via helper `rjcfg_write_and_verify()`.

---

## 4. Bukti Runtime VirtualBox (EKSEKUSI NYATA)

VM `"Open Core Router"`, UART1 → file. Petikan apa adanya:

`is02_ata2.log` / `is02_rjcfg2.log` (probe + tulis pertama):
```text
[R22] ATA/IDE PRIMARY UP (PIO nyata, drive 0 master) model=VBOX HARDDISK capacity=2 GiB sectors=4194304
[R23] GPT PRIMARY HEADER (LBA1) DIBACA DARI DISK: signature 'EFI PART' OK (PIO nyata)
[R23] GPT header disk: entry_lba=2 num_entries=44 entry_size=128 usable=34..4194270 crc_ok=YA
[R23] RJCFG01 partisi NYATA terdeteksi di disk (LBA 262178) — jalur persistence on-disk diaktifkan
```

`is02_rjcfg2.log` (boot #1 — tulis ke disk + baca-balik):
```text
[R26] RJCFG01 belum ada di disk (RJCFG01: magic tidak valid) — boot bersih dengan konfigurasi default
[R26c] RJCFG01 belum ada di disk — auto-commit + tulis (bukti write path persisten)...
[R26] RJCFG01 READBACK VERIFIED: slot 1 seq=2 (CRC header + slot cocok dari disk, PIO nyata)
[R26c] RJCFG01 TERSIMPAN DI DISK (VERIFIED) — reboot akan memuat dari disk.
```

`is02_rjcfg3.log` (boot #2 — persistensi LINTAS BOOT terbukti):
```text
[R26] RJCFG01 journal DIBACA DARI DISK: slot 1 seq=2 (CRC slot + header terverifikasi, PIO nyata)
rinjani-cli> config show
  Active Slot : Slot B (seq=2)
```

Gate ARP host setelah perubahan (14:4x, `is02_rjcfg3` berjalan):
```text
Interface: 192.168.56.1 --- 0x8
  192.168.56.100        08-00-27-dd-48-91     dynamic
Packets: Sent = 3, Received = 3, Lost = 0 (0% loss)
```

---

## 5. Catatan Kegagalan Nyata & Perbaikannya (Evidence-First)

1. **Probe ATA ABRT pertama** (`is02_ata1.log`): reset lunak saja tidak cukup; ERR latch tidak dibersihkan. Perbaikan: SRST via 0x3F6 + poll DRDY + clear latch (baca 0x1F1/0x1F7) + retry dengan 0xE0 LBA-select → sukses (`is02_ata2.log`).
2. **Compile error `log(stage, log)` (E0057)** — closure logger hanya menerima 1 argumen; terjadi dua kali; pola kini dibakukan `log(stage)`.
3. **Offset entri GPT salah** (start/end dibaca di 16/24, seharusnya 32/40) — tertangkap oleh round-trip test baru; diperbaiki sebelum bukti runtime.
4. **Insiden boot dari ISO lama (`is02_rjcfg1.log`):** setelah rebuild VDI, VM tetap boot dari DVD (`is02-poweroff.iso`, kernel lama tanpa [R26]) karena boot1=DVD; ditambah UUID file VDI baru tidak cocok dengan registry VirtualBox (`convertfromraw` pasca-replace) sehingga medium "inaccessible". Diagnosis: `config show` masih Slot A seq=1 padahal image memuat kode baru. Perbaikan: detach ISO (`storageattach --medium none`), `--boot1 disk`, sinkronkan UUID medium (`internalcommands sethduuid`) → boot berikutnya (`is02_rjcfg2`) menjalankan kernel baru. **Pelajaran operasional: selalu verifikasi identitas kernel yang berjalan, bukan hanya identitas file image.**
5. **Template lama "20 GiB" adalah data simulasi** — dikoreksi total ke kapasitas nyata hasil IDENTIFY (2 GiB). Semua jalur yang mencetak angka tanpa disk nyata kini melabeli eksplisit "simulated/fallback".

---

## 6. Matriks Gate Governance

| Gate ID | Deskripsi | Status | Bukti |
| :--- | :--- | :---: | :--- |
| `OPCR-77` | Driver block nyata ATA/IDE PIO (`no_std`, IDENTIFY + LBA28 rw) | **PASS-LOCKED** | `is02_ata2.log`, 4 unit tests |
| `OPCR-78` | Parse & CRC-verify GPT langsung dari disk | **PASS-LOCKED** | `is02_rjcfg2/3.log` `[R23] crc_ok=YA`, RJCFG01 LBA 262178 |
| `OPCR-79` | RJCFG01 write-path persisten di partisi disk + readback | **PASS-LOCKED** | `is02_rjcfg2.log` READBACK VERIFIED seq=2; `is02_rjcfg3.log` DIBACA DARI DISK (lintas boot) |
| `OPCR-80` | Gate ARP host: rinjani terlihat dinamis + ping 3/3 | **PASS-LOCKED** | §4, `is02_arp5.log` + cek ulang pasca-perubahan blok |
| `OPCR-76` | In-kernel self-installer (`install disk`) | `REGISTERED-NOT-ATTEMPTED` | Jujur: belum dikerjakan; kini sudah ada prasyarat block device tulis |
| `OPCR-75b` | Power-off ACPI PM1_CNT | **VERIFIED** (probes) | `7df20e2`, `is02_poweroff_probe7b.log`; CLI `shutdown` belum diuji interaktif sesi ini (`NOT RUN`) |

Label epistemik: §2/§4 `VERIFIED` (eksekusi aktif); §3 `VERIFIED` via unit tests + runtime; klaim kapasitas lama "20 GiB" = dinyatakan `FABRICATED (simulasi)` dan dicoret; penundaan self-installer = `NOT RUN`.

---

## 7. Perubahan Konfigurasi VM (Akhir Sprint)

- IDE-0-0: `rinjani-1.0.0-x86_64-disk.vdi` (UUID file diset = `aedff65b…` sesuai registry).
- IDE-1-0: ISO power-off **di-detach** (`--medium none`) agar tidak ada boot terselubung dari kernel lama.
- Boot order persisten: `disk` → none. UART1 file → log sprint (`is02_rjcfg2.log`, `is02_rjcfg3.log`).
- Keadaan akhir VM pada saat laporan: running (boot #2, journal Slot B seq=2 termuat dari disk).

---

## 8. Catatan Governance Delegasi (Perintah Pemilik)

Modul §3.3 (format disk RJCFG01) didelegasikan ke mitra agent dengan prompt kontrak-API eksplisit (fungsi, offset byte, pesan error, larangan mengubah file lain). Hasil **dipantau**: `git diff` diperiksa (hanya `router/asl-router-persistence/src/lib.rs`, +218 B baris), gate test/check diulang oleh Kanoman sebelum integrasi boot. Klaim "sudah diverifikasi mitra" tidak dipakai sebagai pengganti gate sendiri.

---

## 9. Poin 4 — Push Remote

Commit sprint (berurutan): `7df20e2` [is02-a], `0e9dd92` [is02-net], lalu commit IS-02-b (driver ATA + GPT parse + persistensi RJCFG01 + laporan ini) — di-push ke `origin/main` setelah seluruh gate §2 hijau (aturan AGENTS.md §4.8).

---

## 10. Prompt Sprint Berikutnya (Anti Gravity / mitra)

### Prompt A — IS-03: In-Kernel Self-Installer (OPCR-76, kini feasible)
```text
1. Modul `asl-router-installer` (no_std, streaming 8 KiB via ATA write_sector).
2. CLI `install disk` dari boot ISO: tulis GPT+ESP dari stream, verifikasi baca-balik SHA per file.
3. Bukti serial [R27] INSTALL START..DONE + boot dari hasil install tanpa tooling host. DILARANG klaim PASS tanpa log VM nyata.
```

### Prompt B — CFG-02: `config set hostname=…` On-Disk
```text
1. Perluas CLI: `config set hostname=<str>` lalu commit ke disk (jalur [R26] sudah ada).
2. Bukti lintas-boot: hostname baru bertahan setelah reboot; seq naik; Slot berganti A↔B terverifikasi CRC.
```

### Prompt C — Ketahanan
```text
1. Uji corrupt header primer (dd ke copy VDI) → kernel wajib jatuh ke backup GPT LBA akhir.
2. QEMU OVMF/UEFI boot path sebagai kedua setelah BIOS Limine.
```
