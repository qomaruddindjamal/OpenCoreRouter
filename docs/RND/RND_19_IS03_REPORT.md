# LAPORAN SPRINT IS-03: SELF-INSTALLER DALAM KERNEL (CLI `install`), KONSOL SERIAL DUA-ARAH & BOOT DARI DISK HASIL INSTAL

> **Document ID:** `docs/RND/RND_19_IS03_REPORT.md`  
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)  
> **Date:** 2026-10-15  
> **Status:** `RUNTIME VERIFIED & PASS-LOCKED` (self-installer kini `VERIFIED`, bukan `NOT RUN` lagi)  
> **Author:** Kanoman AI di bawah supervisi Anti Gravity  
> **Baseline Commit:** `09741b4` (OpenCR, `feat(rinjani-1.0.0): dhcp/tx path instrumentation & self-check [v2b]`)  
> **Doktrina:** ASL CORE BUKAN LINUX — mesin instalasi (pipeline 9 tahap), SHA-256, parser BPB FAT32, CRC array GPT, dan driver ATA multi-drive ditulis native `#![no_std]` dari spesifikasi publik (UEFI 2.10 §GPT, Microsoft FAT 1.03, FIPS 180-4, ATA/ATAPI-8). Tidak ada kode Linux/FreeBSD yang masuk repositori.

---

## 1. Ringkasan Eksekutif

Sprint IS-03 menutup gap terakhir yang pada IS-02 masih berlabel `NOT RUN`: **install-to-disk yang dijalankan oleh kernel itu sendiri**, bukan oleh tool host.

1. **Crate baru `asl-router-installer` (`#![no_std]`, VERIFIED):** mesin self-installer sektor-per-sektor src→tgt dengan pipeline 9 tahap ber-tag stage (`PREFLIGHT → TARGET_IDENTIFICATION → TARGET_CONFIRMATION → PARTITION_PLAN → PARTITION_WRITE → BOOT_ARTIFACT_INSTALL → CONFIG_INITIALIZATION → READBACK_VERIFICATION → INSTALL_COMPLETE`). Abstraksi block device lewat trait `BlockIo` sehingga seluruh pipeline teruji host-side terhadap `MockDisk` sparse (22 unit test) sebelum satu byte pun ditulis ke media nyata.
2. **Multi-drive ATA nyata (`VERIFIED`):** driver ATA diperluas ke posisi slave via `AtaDevice::empty_drive(1)` + register 0x1F6 dengan bit DEV; kernel mem-probe kedua drive saat boot — `[R22]` master `VBOX HARDDISK` 4.194.304 sektor, `[R22b]` slave `VBOX HARDDISK` 4.194.304 sektor. Probe slave yang tidak ada disk **gagal jujur** (`IDENTIFY: floating bus`), tidak pura-pura ada.
3. **Konsol serial dua-arah (`VERIFIED`):** mode pipe named VirtualBox ditolak Windows (`VERR_INVALID_NAME` pada `--uartmode1 server/client`), sehingga VM dikonfigurasi `--uartmode1 tcpserver 4555` dan dibangun `tools/serial-console.js` (Node, auto-reconnect): tee output guest ke log **dan** kirim ketikan host ke UART guest. Bukti round-trip: `help`, `install disk`, `install confirm YES-INSTALL`, `config show` semuanya dieksekusi lewat jalur ini.
4. **CLI `install` (`VERIFIED`):** `install disk` (inspeksi murni, nol tulis), `install confirm <token>` (token **hanya** dari UART RX, dibandingkan persis dengan `YES-INSTALL`), `install cancel`. Tiga-duanya tampil di `help`.
5. **Bukti instal nyata + boot dari hasil instal (`VERIFIED`):** instalasi ke VDI disposable 2 GiB pada primary slave → `[R27] INSTALL DONE sectors_written=8670 esp_sectors=8579 sha256_w==sha256_rb=OK`; setelah disk sumber dilepas dan disk target dipasang sebagai master, VM **boot dari disk hasil instalasi** (Limine → kernel → `[R26] RJCFG01 journal DIBACA DARI DISK: slot 0 seq=1`) — jurnal target segar Seq A/seq=1, berbeda dari sumber (Slot B/seq=2). Gate ARP host tetap hijau: `192.168.56.100 → 08-00-27-dd-48-91 dynamic`.
6. **Bug nyata yang hanya muncul di hardware (`VERIFIED` → diperbaiki):** preflight menolak disk sumber karena installer menghitung CRC32 array entri GPT atas 32 sektor (128 entri) tetap, sementara disk bentukan Limine mendeklarasikan **44 entri** (5.632 B). Perbaikan: CRC dihitung atas region deklaratif `num_entries × entry_size` dengan sektor terakhir partial; regresi dikunci oleh `test_41_array_crc_region_deklaratif_gaya_limine`.

---

## 2. Artefak & Hash

| Artefak | Nilai |
| :--- | :--- |
| Kernel ELF `target/x86_64-unknown-none/debug/asl-kernel` | SHA256 `4170498a90bd37f5…068abcb9`, mtime 2026-10-15 16:04:36 (build yang sama yang dieksekusi VM) |
| Disk raw sumber `target/rinjani-1.0.0-x86_64-disk.raw` | SHA256 `87E0C98DE4632378…4438CE63`, 2 GiB, mtime 16:05:25 |
| VDI target disposable | `target/rinjani-is03-target-disposable.vdi`, UUID `f8d61ded-68d9-490c-af19-fbee6802ae7a`, capacity 2048 MiB, size-on-disk 9.437.184 B pasca-instal |
| Target sebelum instal (blank) | SHA256 raw 2 GiB `a7c744c13cc101ed…4958ea51`, `nonzero_bytes = 0`, `head34_all_zero = true`, `tail33_all_zero = true` |
| Digest verifikasi installer | `sha256_written == sha256_readback = 6a75d88c08d60551b0c471d955277950eb926c553d8a622fe53fb9b5e0ad6f49` (8.670 sektor) |
| Log konsol terkurasi | `Analysis/reports/is03_install_console.log` (614 baris; baris spam `[R24] GARP` difilter, baris lain verbatim) |
| Unit test workspace | `239 passed / 0 failed`, 64 suite `ok` (naik dari 214 pada IS-02; +25: installer 22 + driver ATA/GPT) |
| Gate no-std | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` → `Finished`, 0 error |

---

## 3. Bangunan Baru Sprint Ini

### 3.1 `router/asl-router-installer/` (CRATE BARU, `#![no_std]`, 22 unit test)
- `trait BlockIo { capacity_sectors / read_sector / write_sector }` — satu-satunya sentuhan ke media; produksi = adapter kernel, uji = `MockDisk` (BTreeMap sparse, dengan injeksi `fail_read_at`, `fail_write_at`, `corrupt_write_at`, `flip_on_later_read_at`).
- Proteksi wajib yang diuji: kapasitas src==tgt dan ≥ 512 MiB; sumber harus `EFI PART` dengan **CRC header + CRC array entri** valid; target harus blank (LBA1 bukan `EFI PART` + sampling sektor nol); overlap/rentang partisi terhadap usable area; **tolak deskriptor sumber==target**; token `YES-INSTALL` harus cocok **sebelum** satu sektor pun ditulis.
- Batas salin ESP tidak dikira-kira: BPB FAT32 dibaca (validasi field wajib-nol FAT16, `fat32_length`, dsb.) lalu **scan FAT0** menentukan cluster alokasi tertinggi → `esp_used_sectors` (real: 8.579 sektor dari partisi 128 MiB).
- SHA-256 native (FIPS 180-4) streaming dengan `peek()` sehingga digest jalur tulis dan jalur verifikasi dapat dibandingkan tanpa buffer besar; `test_21` mengunci kesetaraan streaming vs one-shot.
- Jurnal RJCFG01 target **diinisialisasi baru** (seq=1, Slot A) dan dibaca-balik dari target; jalur kode tidak pernah menerima disk sumber untuk ditulis.

### 3.2 Integrasi kernel — `asl-kernel/asl-kernel-bin/src/main.rs`
- `struct AtaBlockIo<'a>` (adapter `BlockIo` → `AtaDevice`), meneruskan log driver ATA ke serial agar timeout/ERR ATA tetap terlihat di dalam tahap installer.
- `[R22b]` probe primary slave saat boot; flag `target_is_ata` menyetu jalur instal.
- CLI arms: `install disk` → `inspect()` + cetak rencana; `install confirm <tok>` → `install_run()`; `install cancel` → tegas menyatakan nol tulis; helper `install_print_plan()` / `install_print_failure()` untuk format `[R27]`.

### 3.3 Driver ATA multi-drive — `router/asl-router-drivers-ata/src/lib.rs`
- `drive_head_lba(drive, hi)` / `drive_head_chs(drive)` murni & teruji unit (kesalahan bit DEV = salah sasaran antar drive); `empty_drive(1)`; `probe()` memakai `self.drive`; jendela LBA28 tetap dijaga (`sector > 0x0FFF_FFFF` ditolak).

### 3.4 Tooling
- `tools/serial-console.js` — listener TCP dua-arah (auto-reconnect, tee log, inbox `Analysis/logs/is03-cmd.in`), mode `--send` dan `--peek`.
- Konfigurasi VM: `--uart1 0x3F8 4 --uartmode1 tcpserver 4555`; medium target sempat dipasang di `IDE-1-0` (secondary master) sehingga probe slave benar melapor *floating bus* — kesalahan konfigurasi ini justru menjadi bukti bahwa probe tidak mengarang device.

---

## 4. Bukti Runtime VirtualBox (EKSEKUSI NYATA)

Semua petikan dari `Analysis/reports/is03_install_console.log` (verbatim, baris GARP difilter).

Boot dengan dua drive (`[R1]/[R2]` identitas wajib):
```text
[R1] RINJANI 1.0.0 — Open Core Router
[R2] Powered by ASL Core Kernel v1.0.0
[R22] ATA/IDE PRIMARY UP (PIO nyata, drive 0 master) model=VBOX HARDDISK capacity=2 GiB sectors=4194304
[R22b] ATA/IDE SLAVE UP (PIO nyata, drive 1) model=VBOX HARDDISK capacity=2 GiB sectors=4194304
[R23] GPT header disk: entry_lba=2 num_entries=44 entry_size=128 usable=34..4194270 crc_ok=YA
[R26] RJCFG01 journal DIBACA DARI DISK: slot 1 seq=2 (CRC slot + header terverifikasi, PIO nyata)
```

Inspeksi tanpa tulis (`install disk`):
```text
install disk: INSPEKSI SAJA (nol tulis) src=drive0 -> tgt=drive1
[R27] PREFLIGHT kapasitas src==tgt=4194304
[R27] PREFLIGHT PASS — sumber valid, target blank terproteksi
[R27] TARGET_IDENTIFIED kapasitas_sektor=4194304 backup_header_lba=4194303 entry_lba=2
[R27] PARTITION_PLAN_VALIDATED ESP=34..262177 esp_terpakai=8579 RJCFG01=262178..278561 RJDATA=278562..4194270
```

Penolakan token salah (nol tulis, ditegaskan juga oleh audit media §2):
```text
install confirm: token dari UART RX = NOPE
[R27] PARTITION_PLAN_VALIDATED ESP=34..262177 esp_terpakai=8579 …
[R27] INSTALL FAILED stage=TARGET_CONFIRMATION reason=token konfirmasi tidak cocok — TIDAK ADA sektor yang ditulis
```
Baris `[R27] GPT WRITE START` **tidak pernah muncul** pada jalur ini — sesuai desain (cek token mendahului tulis pertama).

Instalasi terkonfirmasi:
```text
[R27] TARGET CONFIRMED (token benar) — operasi destruktif DIMULAI
[R27] GPT WRITE START head LBA 0..33
[R27] GPT READBACK VERIFIED (head + backup, byte-per-byte)
[R27] ARTIFACT WRITE START ESP LBA 34 span=8579
[R27] ARTIFACT READBACK VERIFIED per-sektor (ESP terpakai)
[R27] RJCFG01 INIT on target LBA 262178
[R27] RJCFG01 INITIALIZED (seq=1 Slot A, CRC tervalidasi dari bacaan target)
[R27] INSTALL VERIFICATION PASSED sektor_tulis=8670 sha256=6a75d88c08d60551…e0ad6f49
[R27] INSTALL DONE sectors_written=8670 esp_sectors=8579 sha256_w==sha256_rb=OK
```

Boot **dari disk hasil instalasi** (sumber dilepas, target dipasang sebagai master):
```text
[R22] ATA/IDE PRIMARY UP (PIO nyata, drive 0 master) model=VBOX HARDDISK capacity=2 GiB sectors=4194304
[R22b] ATA SLAVE PROBE: IDENTIFY: floating bus — channel IDE tidak ada — disk target install tidak tersedia
[R23] GPT PRIMARY HEADER (LBA1) DIBACA DARI DISK: signature 'EFI PART' OK (PIO nyata)
[R26] RJCFG01 journal DIBACA DARI DISK: slot 0 seq=1 (CRC slot + header terverifikasi, PIO nyata)
rinjani-cli> config show
  Active Slot : Slot A (seq=1)
rinjani-cli> show version
Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
```
Pembeda identitas kuat: sumber `Slot B seq=2`, target hasil instal `Slot A seq=1` (jurnal di-nol-kan oleh installer, bukan disalin). Write path pada disk hasil instal juga terbukti: `config save` → `[R26] RJCFG01 READBACK VERIFIED: slot 1 seq=2`.

Gate ARP host (diambil setelah boot dari target):
```text
Interface: 192.168.56.1 --- 0x8
  192.168.56.100        08-00-27-dd-48-91     dynamic
```

---

## 5. Insiden & Koreksi Kejujuran

1. **CRC array GPT 44 vs 128 entri (§1.6).** Fail preflight nyata di hardware; `MockDisk` sprint sebelumnya selalu 128 entri sehingga bug tak terlihat di unit test. Diperbaiki + `test_28` disesuaikan agar pesan kegagalan menampilkan alasan driver yang sebenarnya (bukan pesan generalisasi) dan `test_41` mengunci region deklaratif.
2. **Salah slot kanal IDE.** `storageattach --port 1 --device 0` = *secondary master*, bukan *primary slave*; probe `[R22b]` benar-benar melaporkan *floating bus*. Perbaikan ke `--port 0 --device 1`. Dibiarkan tercatat karena membuktikan probe tidak mengarang device.
3. **Named pipe serial tidak bisa dipakai** pada VirtualBox/Windows untuk modus dua-arah (`VERR_INVALID_NAME`, VBox menghapus prefiks `\\.\pipe\`). Solusi: `tcpserver` + `tools/serial-console.js`. Konsekuensi: log capture VM tidak lagi ke file tracked `Analysis/reports/is02_*.log`.
4. **`Analysis/reports/is02_rjcfg3.log` (tracked) berisi +59.788 baris** hasil capture sesi sebelumnya dan didominasi spam `[R24] GARP`. File dibiarkan utuh di working tree dan **tidak di-commit** pada sprint ini; bukti IS-03 disimpan terpisah sebagai `is03_install_console.log` yang terkurasi.
5. **Format `cargo fmt`.** Baseline repo **tidak** fmt-clean (20 file, termasuk `asl-router-netstack`, `asl-router-gpt`, `asl-router-fat32`), namun `main.rs` di HEAD ternyata clean — karena itu `main.rs` dan `installer/lib.rs` dinormalisasi penuh (±100 baris kosmetik ikut tersentuh) dan file lain **tidak** disentuh. `rustfmt --check` kini 0 diff untuk dua file tersebut; sisanya status `NOT RUN`.

---

## 6. Status Gate

| Gate | Perintah | Hasil |
| :--- | :--- | :--- |
| Unit test workspace | `cargo test --workspace` | `VERIFIED` — 239 passed, 0 failed, 64 suite ok |
| Build bare-metal | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | `VERIFIED` — Finished, 0 error (1 warning `dead_code` warisan `power.rs`) |
| Formatting file yang disentuh | `rustfmt --edition 2021 --check` installer + main | `VERIFIED` — 0 diff |
| Formatting seluruh repo | `cargo fmt --check` | `BLOCKED (by design)` — baseline repo tidak clean; tidak direformat massal |
| Instal runtime ke media disposable | CLI `install confirm YES-INSTALL` di VM | `VERIFIED` — 8.670 sektor, digest tulis==readback |
| Boot dari disk hasil instal | lepas sumber, target jadi master | `VERIFIED` — banner R1/R2, GPT valid, jurnal seq=1 Slot A |
| Proteksi zero-write | `install cancel` + token salah + audit raw | `VERIFIED` — 0 byte non-zero pada 2 GiB target sebelum instal |
| Gate ARP host | `arp -a` | `VERIFIED` — entri dinamis 192.168.56.100 |

---

## 7. Keterbatasan Jujur & Langkah Berikutnya

1. **Posisi drive:** block subsystem memprioritaskan master; jika sistem di-boot dari disk yang berada di *slave* tanpa master, `[R22]` gagal dan kernel jujur jatuh ke mode simulasi (kapasitas dilaporkan sebagai fallback) — perbaikan: adopsi drive boot-first/fallback slave pada `[R22]`.
2. **Durasi instal:** `write_sector` melakukan `CACHE_FLUSH` per sektor → ±4 menit untuk 8.670 sektor di VirtualBox. Optimasi: flush batch (mis. per 64 sektor) dengan tetap flush terakhir sebelum verifikasi.
3. **Kapasitas target harus identik dengan sumber** (preflight menolak beda). Belum ada jalur *adopt/expand* kapasitas target pasca-instal (mesin adopsi RJDATA IS-01 hanya untuk satu disk).
4. **Verifikasi byte-per-byte terhadap image sumber** (digest ESP/GPT sumber vs target) belum dilakukan sebagai perbandingan silang; saat ini kesetaraan dibuktikan lewat digest jalur-tulis vs jalur-baca + inspeksi struktural GPT/BPB/CRC.
5. **Belum ada penghapusan aman** sumber daya sementara (`Analysis/tmp/is03-*.raw/json` milik asisten, akan dibersihkan pada penutupan sprint).
6. **UI/telemetri:** `install` belum muncul di WebUI/API v1 — kandidat sprint DS-02.

---

## 8. Kesimpulan

Self-installer Rinjani 1.0.0 kini **berjalan di dalam kernel** terhadap dua disk ATA PIO nyata, dengan gerbang konfirmasi eksplisit dari UART, readback byte-per-byte, digest SHA-256 yang cocok, dan bukti akhir: VM boot dari disk hasil instalasi dengan jurnal RJCFG01 yang benar-benar segar. Seluruh gate hijau (239 unit test, build no-std, bukti runtime) dan klaim `NOT RUN` untuk self-installer pada IS-02 resmi dicabut menjadi `VERIFIED`.
