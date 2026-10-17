# LAPORAN SPRINT MEM-01: BOOT DAN WebUI PADA RAM MINIMAL 4 MB

> **Document ID:** `docs/RND/RND_27_MEM01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `ce93913` (sprint IFACE-01 Tahap A)
> **Mandat:** ubah implementasi yang rusak/belum tersambung menjadi nyata, terintegrasi, teruji, terbukti; tanpa mengarang kemampuan kernel; tanpa mock/stub sebagai bukti; kosakata status mandat.
> **Permintaan Owner yang mengikat sprint ini:** (1) "optimasikan bisa berjalan di ram 4MB minimal"; (2) "buat default pertama kali: user: admin [sandi tetap owner] jangan diubah-ubah lagi kecuali user yang mengubah sendiri"; (3) "lanjutkan perbaikan … lihat hasil perbaikan terakhir yang belum selesai, dan lakukan recovery kode terakhir yang berfungsi dengan normal".
> **Doktrina:** ASL CORE BUKAN LINUX; tidak ada kode vendor disalin; tidak ada kemampuan kernel yang dikarang.
> **Redaksi:** alamat lease WAN router ditulis `10.10.MM.RRR`, gerbang/DNS lab `10.10.MM.GGG`, tetangga ARP `192.168.C.CC`; MAC tersamar `08:00:27:XX:XX:XX` dan `98:EE:XX:XX:XX:XX`. Sandi default yang ditetapkan owner TIDAK dicatat di dokumen ini (AGENTS.md §4.5); ia hanya hidup di berkas memory lokal operator di luar repo.

---

## 1. Ringkasan eksekutif

Sprint ini menutup satu lubang yang selama ini disembunyikan oleh kemewahan: lab selalu berjalan di 64 MB, sehingga tidak ada satu pun angka tapak memori yang pernah dipertanggungjawabkan. Hasilnya bukan sekadar "muat di 4 MB", melainkan tiga hal yang saling mengunci.

| Kelompok pekerjaan | Isi | Status akhir |
|---|---|---|
| Pemulihan basis | Tree kode = `ce93913` = `origin/main`; gerbang diulang sebelum perubahan apa pun | `RUNTIME-VERIFIED` (fmt 0, 400/0, no-std selesai) |
| Kejujuran `[R3]` | `BootInfo` tidak lagi dikarang (dahulu basis fisik 256 MB, besar 1 GB); kini basis dan total RAM diambil dari Limine memmap, dan `None` => gagal boot secara eksplisit | `RUNTIME-VERIFIED` |
| Jalur build lab | `scripts/build-disk.ps1` default `-Profile release` (ELF 707 168 B, dulu debug 3 703 992 B) + gerbang ukuran ELF dengan **tes negatif** yang terbukti menolak profil debug | `RUNTIME-VERIFIED` |
| Jalur build ISO | `scripts/build-iso.ps1` disamakan (default release + gerbang ELF yang sama + fallback kompilasi yang kini menghormati profil); ELF di dalam ISO diverifikasi byte-identik dengan berkas release | `VERIFIED` (artefak) · boot ISO di VM `NOT RUN` |
| Boot pada 4 MB | Dua boot independen guest 4 MB: jurnal lengkap tanpa PANIC, ring virtio aktif, DHCP BOUND, GARP periodik, RJCFG01 terbaca | `RUNTIME-VERIFIED` |
| WebUI pada 4 MB | Login + `/api/v1/system` + `/api/v1/interfaces` + `/login.html` + seluruh aset rujukannya berbalas `200` dari **dua alamat** (`10.10.MM.RRR` eth0 dan `192.168.56.100` eth1); sandi salah => `401` | `RUNTIME-VERIFIED` |
| Kredensial default owner | Akun `admin` dipasang sekali lewat konsol serial, tertulis ke RJCFG01, dan **terbaca kembali tanpa bootstrap ulang** pada dua reboot berikutnya | `RUNTIME-VERIFIED` |
| Pemangkasan runtime (ring, pool, stack) | **Tidak diperlukan** untuk mencapai 4 MB; tidak ada satu pun kapasitas yang diturunkan | `NOT RUN` (sengaja, lihat §6) |

Gerbang akhir: `cargo fmt --all -- --check` exit 0; `cargo test --workspace` **400 passed / 0 failed / 0 ignored**; `cargo check --target x86_64-unknown-none -p asl-kernel-bin` selesai (1 peringatan `dead_code` warisan `power.rs:120`). Tidak ada tes yang dihapus, diturunkan kriterianya, atau dinonaktifkan. Perbaikan `scripts/build-iso.ps1` (§4) dibuat **setelah** gerbang ini dan hanya menyentuh PowerShell — tidak ada berkas Rust yang berubah sejak angka di atas diukur; kedua skrip (termasuk dua koreksi kosmetik di `build-disk.ps1`: nomor langkah "2." yang sempat hilang dan angka komentar 715 256 → 707 168 B) divalidasi lewat parser (`0 syntax errors`) lalu dieksekusi nyata (build disk §4, build ISO §4 termasuk satu penolakan gerbang).

## 2. Recovery: apa yang dianggap "keadaan terakhir yang berfungsi normal"

Pemulihan tidak diasumsikan dari catatan sesi; ia dibuktikan. `git rev-parse HEAD origin/main` menghasilkan `ce9391386e3c37b6559fd74e8df87d004dcd564d` pada keduanya, `git status --porcelain` kosong untuk `asl-kernel/`, `router/`, `docs/`, dan `Cargo.toml`, lalu gerbang dijalankan **sebelum** satu baris kode pun diubah dan menghasilkan 400/0. Jadi titik balik yang aman adalah `ce93913`, dan tidak ada pekerjaan setengah jadi yang tertinggal di tree.

Dua artefak sesi sebelumnya ditemukan menggantung dan dibersihkan tanpa menyentuh pekerjaan owner: (a) dua berkas inbox konsol bertag salah (`is03-cmd.in` + `.secret`) yang berisi perintah tampil dan payload buangan, bukan sandi; (b) listener serial yang loop reconnect. `Get-CimInstance Win32_Process` setelah pembersihan tidak menampilkan proses `serial-console.js` sama sekali.

## 3. Tapak memori yang terukur (bukan estimasi)

| Konstituen | debug | release | sumber |
|---|---|---|---|
| Berkas ELF yang dibaca Limine (`freadall`) | 3 703 992 B | 707 168 B | `Get-Item` pada `target/x86_64-unknown-none/<profil>/asl-kernel` |
| Jumlah segmen `PT_LOAD` | 4 | 4 | urai header program ELF |
| Citra residen (`sum memsz PT_LOAD`) | 1 503 219 B | 1 212 415 B | urai header program ELF |
| di antaranya `.bss`/NOLOAD (stack kernel 512 KiB, `main.rs` `global_asm` `.skip 524288`) | 528 384 B | 524 296 B | segmen ke-4 (`filesz = 0`) |
| Arena DMA hasil bump-allocator (`DmaArena`, `main.rs:218`) | — | ±164 KiB (4 ring) | `rx_pa=0x53000`…`tx_pfn=0x72` eth0, `0x6A000`…`0x7A000` eth1 |
| Buffer frame TX per sesi (`scratch`) | — | 1 518 B per pemanggilan | `router/asl-router-webui/src/lib.rs:311` |
| RAM USABLE terlapor kernel pada guest 4 MB | — | **708–712 KiB** | baris `[R3]` jurnal |
| RAM USABLE terlapor kernel pada guest 64 MB | — | **62 152 KiB** | baris `[R3]` jurnal |

Kesimpulan teknis: satu-satunya penyebab `PANIC: High memory allocator: Out of memory` di `freadall` adalah **ukuran berkas ELF profil debug**, bukan kekurangan desain alokasi kernel. Citra residen release 1,21 MB + buffer berkas 0,71 MB masih menyisakan arena usable ~0,7 MB pada guest 4 MB, dan seluruh jalur data (ring 1024 descriptor per antarmuka, DHCP, TCP, HTTP, PBKDF2 1024 round, PIO ATA) tetap berjalan.

Temuan ikutan yang jujur: angka `usable` berbeda 4 KiB antara dua boot 4 MB (708 vs 712 KiB). Perbedaannya nyata di Limine memmap, bukan di kernel; laporan ini tidak menyamarkannya menjadi satu angka tetap.

## 4. Perubahan kode

**`asl-kernel/asl-kernel-bin/src/main.rs`** — fungsi `usable_ram()` baru menjumlahkan entri `MEMMAP_USABLE` dari respons Limine dan mengambil basis terendah; `BootInfo::new(0x1000_0000, 0x4000_0000, 2)` yang mengarang 256 MB/1 GB diganti `BootInfo::new(mem_base, mem_bytes, 2)`. Bila memmap kosong atau tanpa region usable, kernel menulis `[R3] CORE INIT FAILED: Limine memmap kosong/tanpa region USABLE` dan berhenti, bukan meneruskan dengan angka karangan. Baris `[R3]` kini berbunyi `CORE INIT (RAM usable <N> KiB dari Limine memmap)`.

**`scripts/build-disk.ps1`** — default `$Profile` menjadi `release` dengan komentar yang menyebut angka; parameter baru `$ElfMaxBytes` (bawaan 1 048 576); langkah "1b" mengurai header program ELF untuk menghitung jumlah segmen `PT_LOAD` dan jumlah `memsz`, menolak build sebelum menyentuh disk bila berkas ELF melewati batas, lalu mencetak satu baris jejak.

Bukti bahwa gerbang itu menggigit, bukan seremonial (disengaja dijalankan pada profil debug):

```
kernel ELF 3703992 B > batas 1048576 B; guest 4 MiB tidak akan termuat (pakai -Profile release)
```

Bukti pada jalur yang benar:

```
BOOT FOOTPRINT: ELF 707168 B, PT_LOAD 4 segmen, residen 1212415 B (batas ELF 1048576 B)
=== DISK IMAGE BUILD SUCCESSFUL ===
RAW   : SHA256 D9B6882DB32B6948BB2A385CB23FFD11A268592555EF96DC0D141E381E7C6F35
VDI   : target/rinjani-1.0.0-x86_64-disk.vdi (6291456 bytes)
```

Dua build release berturut-turut menghasilkan SHA256 RAW yang identik, jadi perubahan ini deterministik dan bisa diaudit ulang.

**`scripts/build-iso.ps1`** (dilakukan pada sesi yang sama, setelah dua commit pertama) — jalur ISO dibujuk mengikuti anggaran yang sama: default `$Profile` menjadi `release`, gerbang `$ElfMaxBytes` + urai `PT_LOAD` disalin, danfallback kompilasi yang dahulu selalu `cargo build ... --target x86_64-unknown-none` (profile debug) kini memakai `--release` ketika `$Profile` release. Tanpa perbaikan fallback itu, ISO ber-profil release akan membangun ELF debug lalu gagal cari berkas.

```
REJECTED: kernel ELF 3703992 B > batas 1048576 B; guest 4 MiB tidak akan termuat (pakai -Profile release)   [-Profile debug]
BOOT FOOTPRINT: ELF 707168 B, PT_LOAD 4 segmen, residen 1212415 B (batas ELF 1048576 B)
=== ISO BUILD SUCCESSFUL ===
Size  : 4313088 bytes
SHA256: E7B8A41C5AA0EB4E2D126B2150AD1B54F0C77247A52B78FD0CAC16A41705642E
```

Sebelum dijalankan, berkas ISO di `target/` berukuran 7 297 024 B (build lama dari profil debug). ISO baru 4 313 088 B; selisihnya 2 983 936 B konsisten dengan selisih ukuran ELF debug−release (2 996 824 B). Klaim "yang lama membawa ELF debug" berstatus `INFERRED` dari aritmetika itu — berkas lamanya sudah tertimpa oleh build baru, jadi tidak bisa saya bandingkan isinya secara langsung.

Bukti bahwa artefak ISO benar-benar membawa ELF release (bukan sekadar klaim dari skrip): `/boot/rinjani-kernel.elf` diekstrak kembali dari ISO dengan `xorriso -osirrox on -extract`, menghasilkan 707 168 B dengan SHA256 `ee74ad73d64ec5116719e98d204d339671976faf04e4acc2f0d8a5a2c02f7b10` — sama persis dengan `target/x86_64-unknown-none/release/asl-kernel`. Bukti level artefak ini `VERIFIED`; eksekusi boot-nya di VM masih `NOT RUN`.

## 5. Kredensial default: keputusan owner dan eksekusinya

Keputusan owner (2026-10-17): akun pertama kali adalah `admin` dengan satu sandi tetap pilihan owner, dan **sandi itu tidak boleh diganti-ganti lagi kecuali owner sendiri yang mengubahnya**. Akar masalah yang owner keluhkan adalah proses saya, bukan kernel: setiap rebuild disk menghasilkan nol kredensial (`[R30] WebUI TERKUNCI`, memang fail-closed yang benar), dan saya selama ini mengisi bootstrap dengan sandi lab baru yang saya karang sendiri tiap sprint. Aturan anti-rotasi sekarang tercatat sebagai keputusan yang mengikat; plaintext-nya tidak masuk repo, dokumen, log, maupun laporan ini.

Eksekusi dan buktinya di konsol serial (tag `mem01`):

```
Kata sandi (10-64 karakter, tidak tampil; Enter kosong membatalkan):
[R26] RJCFG01 READBACK VERIFIED: slot 1 seq=4 (CRC header + slot cocok dari disk, PIO nyata)
% auth: akun 'admin' role admin, iterasi PBKDF2 1024, persistensi: tertulis dan terbaca kembali pada RJCFG01
```

Lintas reboot, tanpa mengetik ulang apa pun:

```
[R26] RJCFG01 journal DIBACA DARI DISK: slot 1 seq=4 (CRC slot + header terverifikasi, PIO nyata)
[R30] Auth: 1 kredensial dimuat dari RJCFG01, entropi token diaduk dari TSC+MAC+jurnal
```

Proteksi yang tetap berlaku terverifikasi di guest 4 MB: login dengan sandi salah dari alamat yang sama berbalas `401`, dan bearer yang sah sepanjang sesi hanya 64 heks.

## 6. Bukti runtime

Siklus lab: disk release baru → tarian UUID penyimpanan (detach `--medium none` → `closemedium disk <uuid-lama>` **tanpa** `--delete` → re-attach; berkas `db37b6a8…` digantikan `80ab336f-1fd2-43dc-840c-373d28e8dd95`) → listener serial start sebelum `startvm` supaya jurnal boot tidak bolong.

**Boot A — guest 4 MB (ELF release).** Jurnal 100 % tanpa `PANIC`/`guru`/triple fault. Penanda penting: `[R1]`/`[R2]` banner utuh, `[R3] CORE INIT (RAM usable 712 KiB dari Limine memmap)`, `[R24] queue size device q0=1024 q1=1024` untuk kedua antarmuka, `[R24] ETH1-LAN VIRTIO DMA UP … ip=192.168.56.100`, `[R28] DHCP BOUND eth0=10.10.MM.RRR/24 (netmask sumber: option-1-ack) gw=10.10.MM.GGG (sumber: option-3-ack) dns=10.10.MM.GGG (sumber: option-6-ack)+1 8.8.8.8 lease=1800 s`, `[R21] DNS PROXY upstream=10.10.MM.GGG (sumber: option-6-ack)`, `[R30]` fail-closed saat disk masih nol kredensial, lalu `[R24] GARP eth1 announced 192.168.56.100 broadcast (42 bytes)` pada irama PIT 5 detik.

**Matriks HTTP pada Boot A (kedua alamat lokal, tanpa kredensial tersisa dari sesi lama).**

| Alamat | `POST /api/v1/auth/login` | `GET /api/v1/system` (bearer) | `GET /api/v1/interfaces` | `GET /login.html` | aset rujukan `login.html` | sandi salah |
|---|---|---|---|---|---|---|
| `10.10.MM.RRR` (eth0 WAN) | `200` | `200` | `200` | `200` (5 560 B) | `200` semua | `401` |
| `192.168.56.100` (eth1 LAN) | `200` | `200` | `200` | `200` (5 560 B) | `logo.svg` 1 084 B, `base.css` 5 921 B, `session.css` 6 455 B, `theme.js` 2 873 B, `api.js` 5 985 B — semua `200` | `401` |

`/api/v1/system` pada guest 4 MB menjawab nilai hidup: `"distro":"Rinjani 1.0.0"`, `"kernel":"ASL Core Kernel v1.0.0"`, `"uptime_ms":196656`, `"wan_up":true`, `"wan_source":"dhcp"`, `"requests_served":7`, `"status":"operational"`.

**Boot B — regresi guest 64 MB (build yang sama).** `[R3] CORE INIT (RAM usable 62152 KiB dari Limine memmap)`, `[R30] Auth: 1 kredensial dimuat dari RJCFG01` (tanpa bootstrap ulang), login + `/api/v1/interfaces` `200` dari kedua alamat. Beralih RAM bolak-balik tidak mengubah kebenaran laporan memori.

**Boot C — guest 4 MB sekali lagi (persistensi di RAM minimum).** `[R3] … 708 KiB …`, `[R26] RJCFG01 journal DIBACA DARI DISK: slot 1 seq=4`, `[R30] Auth: 1 kredensial dimuat`, lalu login + `/api/v1/system` + `/login.html` `200` pada kedua alamat.

Penghentian daya dilakukan lewat perintah CLI kernel `shutdown` (ACPI) pada kedua siklus, dengan bukti `VMState="poweroff"` dan log konsol mencatat host disconnect — bukan `controlvm poweroff`.

## 7. Eksepsi, kesalahan saya, dan temuan yang belum diperbaiki

1. **Bukti tersamar pertama saya tidak sah dan sudah diulang.** Panggilan `--send-secret`/`--peek` pertama saya lupa menempel `SERIAL_TAG=mem01`, sehingga payload masuk ke inbox tag `is03` dan "0 kebocoran" yang saya laporkan saat itu tidak membuktikan apa pun. Setelah tag benar, payload buangan `zzz-mask-probe` dikirim dan `grep` jurnal mencatat 0 kemunculan teks aslinya; Sandi owner juga 0 kemunculan di jurnal dan 0 di berkas repo.
2. **Probe buangan sempat menjadi sandi `admin`.** Kernel masih dalam mode tunggu sandi ketika probe masuk, sehingga akun pertama terbentuk dengan sandi buangan. Saya deteksi dari baris hasil, lalu menimpanya dengan kredensial default owner dan memverifikasi ulang (`seq=4`, `1 kredensial dimuat`). Tidak ada sandi owner yang bocor: pengecekan plaintext = 0 di jurnal dan 0 di repo.
3. **Artefak gema konsol.** Baris `[host->guest] auth set admin adminauth set admin admin` adalah penanda tee + gema kernel atas satu kiriman, bukan duplikasi pengiriman; dan prompt `rinjani-cli>` muncul berdesah di sela mode tunggu sandi. Ini kegaduhan kosmetik jurnal, bukan cacat otorisasi — tetapi membuat urutan mudah salah baca, sehingga laporan ini menyebutnya apa adanya.
4. **Temuan warisan versi (belum saya ubah, menunggu keputusan owner).** Empat crate kernel masih membawa penanda `RINJANI_1_0_3` (`asl-bootloader`, `asl-hal`, `asl-memory`, `asl-afdx-ipc`), termasuk satu tes yang mengasersinya, padahal `AGENTS.md` §2 mengunci distro pada Rinjani 1.0.0 dan menyatakan histori 1.0.3 dipensiunkan. `asl-kernel-core` sudah benar (`ASL_CORE_KERNEL_v1_0_0`). Saya tidak menyentuhnya di sprint ini agar MEM-01 tetap satu topik.
5. **Pangkas RAM runtime sengaja TIDAK dikerjakan.** Ring 1024 descriptor, stack 512 KiB, dan tabel statis masih utuh karena 4 MB tercapai tanpa menyentuhnya. Menurunkan kapasitas hanya untuk angka yang lebih cantik akan melanggar aturan "jangan turunkan kriteria".
6. **Angka ini milik lab VirtualBox.** 708–712 KiB usable adalah hasil peta memori guest 4 MB di Hyper-V/NEM, bukan klaim perangkat keras carrier-grade mana pun.
7. **Satu scratch saya sendiri sempat mencemari root repo.** Saat mengekstrak ELF dari ISO, penulisan ulang path oleh MSYS membuat direktori literal bernama `C:` muncul di root `C:\OpenCR`. Saya temukan, periksa isinya (satu salinan `rinjani-kernel.elf` hasil ekstraksi saya, bukan pekerjaan owner), lalu buang; `git status` sesudahnya bersih dan `ls` root tidak lagi menampilkan direktori itu. `target/iso-check/` (scratch kedua) juga dihapus.

## 8. Batas yang masih jujur

| Hal | Keadaan |
|---|---|
| Uji di bawah 4 MB | `NOT RUN` — VirtualBox menolak konfigurasi lebih rendah untuk profil guest ini; 4 MB adalah batas bawah yang teruji |
| RAM yang dilaporkan ke WebUI | `BLOCKED` — `/api/v1/system` belum memuat kolom memori; nilai `[R3]` hanya terlihat di jurnal konsol (calon Tahap B, agar operator melihat RAM dari peramban) |
| Profil debug untuk pengembangan | `UNIT-TESTED` (gerbang menolak > 1 MiB); jalur `-Profile debug` tetap tersedia untuk guest ber-RAM besar |
| ISO | `VERIFIED` level artefak — `scripts/build-iso.ps1` kini berdefault `release` dengan gerbang ukuran ELF yang sama; `/boot/rinjani-kernel.elf` yang diekstrak kembali dari ISO terbukti identik byte dengan berkas release (§4). **Boot dari ISO di VM: `NOT RUN`** (menuntut pertukaran media optik VM yang sedang memegang disk lab 4 MB) |
| Modul `BLOCKED` (WireGuard, VPN, XRay, QoS, DHCP Server, Packages, Tools/Speed Test/Telnet/Trace) | tetap `BLOCKED` dan tidak dinaikkan ke navigasi aktif |

## 9. Kosakata status yang dipakai

`RUNTIME-VERIFIED` — teruji dari boot nyata di guest; `UNIT-TESTED` — terbukti pada level tes/gerbang host tanpa eksekusi guest; `NOT RUN` — direncanakan, belum dijalankan; `BLOCKED` — tertahan dependensi atau keputusan owner. Untuk taksonomi bukti: `VERIFIED` dipakai pada pemeriksaan byte-level artefak ISO di host (ekstraksi + SHA256) yang bukan eksekusi guest. Tidak ada `PASS` untuk sesuatu yang tidak dijalankan.

## 10. Rekomendasi lanjutan (urutan saran)

1. **MEM-01-B2 — SELESAI pada sesi ini:** `scripts/build-iso.ps1` disamakan ke profil release dengan gerbang ukuran ELF yang sama (lihat §4). Yang masih terbuka hanyalah **boot ISO di VM** (`NOT RUN`), karena menuntut melepas media optik yang sekarang memegang konfigurasi lab 4 MB; lakukan pada siklus lab berikutnya bila owner menghendaki bukti RINJANI-boot-dari-ISO.
2. **MEM-01-C:** tampilkan RAM usable nyata di `/api/v1/system` + sel Dashboard (sumber tetap Limine memmap), supaya klaim memori bisa dibaca operator tanpa konsol serial.
3. **IFACE-01 Tahap B** (tetap terbuka): registry NIC 3–5, himpunan alamat melayani di JSON, alamat eth1 dari RJCFG01, jalur terima UDP/67 + UDP/53.
4. **Keputusan owner:** penyeragaman penanda versi `RINJANI_1_0_3` → 1.0.0 (lihat §7.4).
5. Tugas lama yang belum bergerak: #81 gema ICMP keluar, #83 VLAN/QoS/capture, #84 DEFER-05/06.
