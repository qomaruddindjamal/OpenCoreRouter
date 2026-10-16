# LAPORAN SPRINT UI-01: WEBUI MODULAR & RUPA KONSOL CARRIER (REFERENSI `web/` READ-ONLY)

> **Document ID:** `docs/RND/RND_22_UI01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-16
> **Status:** `RUNTIME VERIFIED` (kernel boot nyata di VM + browser nyata membaca HTTP dari kernel)
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `09741b4` (`feat(rinjani-1.0.0): dhcp/tx path instrumentation & self-check [v2b]`) — kerja BOOT-01 + UI-01 masih uncommitted, lihat §7
> **Doktrina:** ASL CORE BUKAN LINUX & nol library pihak ketiga — HTML/CSS/JS ditulis ulang sendiri; direktori referensi `C:\OpenCR\web` bersifat **read-only** dan hanya dipelajari sebagai rupa, tidak ada satu baris kode referensi yang disalin.
> **Redaksi:** alamat IP lab dan MAC ditulis tersamar (`10.10.MM.NNN`, `08:00:27:XX:XX:XX`) sesuai kebijakan Zero Secrets in Repository.

---

## 1. Ringkasan Eksekutif

Permintaan owner: **buat tampilan web seperti pada referensi `C:\OpenCR\web` (read-only), terapkan ke web Open Core Router, dan buat versi HTML modular.**

Yang dikerjakan dan dibuktikan:

1. **WebUI dipecah menjadi HTML modular (`VERIFIED`).** Satu berkas `shell.html` berisi chrome saja (header, sidebar, footer, tempat mount), sedangkan tiap halaman menjadi parsial HTML kecil di `/views/<modul>.html` yang diminta client-side saat modulnya dibuka. Registry kernel kini melayani **12 aset** ter-embed (shell, 3 CSS, 3 JS core, 4 parsial view, logo SVG).
2. **Rupa mengikuti bahasa desain referensi (`VERIFIED` lewat snapshot aksesibilitas).** Chrome gelap (`#181b20`/`#1e222b`) + area data terang (`#f4f6f9`), panel berkepala gelap, header tabel kapital, pill status berwarnya, KPI grid, dan mini-rail sidebar. Nilai warna dipetakan ke token `--pf-*` milik proyek agar tidak ada warisan nama dari produk referensi.
3. **Nol data karangan (`VERIFIED`).** Hanya empat endpoint kernel nyata yang boleh tampil (`/api/v1/system`, `/api/v1/interfaces`, `/api/v1/firewall/rules`, `/api/v1/services`). Modul yang belum punya endpoint (rute & gateway, lease DHCP, tabel ARP, jingkar paket, ping/traceroute, QoS, konfigurasi) ditampilkan **abu-abu dengan keterangan "Modul belum aktif di build ini"**, bukan halaman kosong yang seolah valid.
4. **Dua cacat nyata ditemukan dan diperbaiki selama verifikasi runtime** — pengikat deklaratif melewatkan elemen akar, dan validator cache yang tidak berubah saat isi aset berubah. Detail di §4.

## 2. Arsitektur Aset

| Lapisan | Berkas | Isi |
|---|---|---|
| Chrome | `assets/shell.html` (3 594 B) | header + brand + chip hostname/bind + badge Community Edition, sidebar, `<main id="view-mount">`, footer baris sinkron |
| Parsial view | `assets/views/{system,interfaces,firewall,services}.html` (1 973 / 1 904 / 1 859 / 2 427 B) | hanya bentuk + atribut pengikat; tidak pernah memuat data |
| Gaya | `assets/css/{base,layout,components}.css` (2 2 / 4 6 / 3 9 KB) | token & tipografi, chrome & sidebar, panel/tabel/pill/KPI |
| Runtime client | `assets/js/core/{api,ui,app}.js` (1 971 / 6 838 / 5 103 B) | klien XHR + unwrap sampul `{ok,data}`; pengikat deklaratif; router hash + segarkan 5 s + mini-rail |

Pemecahan ini bukan kosmetik: tiap aset harus muat di satu buffer respons TCP kernel, sehingga gerbang konstan di `router/asl-router-webui/src/lib.rs` menahan `largest_asset_len() + HTTP_HEADER_BUDGET <= WEB_RESP_MAX` (8 192 B). Aset tetap di-`include_bytes!` ke image kernel, jadi **install kernel = install WebUI**; tidak ada daftar aset di ESP yang bisa busuk. `WEB_SESSIONS` dinaikkan ke 8 karena beban first-load modular (±10 permintaan) lebih besar daripada shell tunggal.

**Kontrak pengikat (tanpa `innerHTML`, tanpa library):**

```
data-source="/api/v1/x"   akar yang diisi objek JSON x
data-list="/api/v1/x"     tbody; satu <tr data-row> dipakai ulang sebagai templat
data-bind="nama_bidang"   selisih nilai ditulis lewat textContent
data-fmt="uptime|ip|…|pill-yes"   formatter / warna pill
```

Pemformat `empty()` menormalkan `""`, `0.0.0.0`, dan MAC all-zero menjadi `—`, sehingga antarmuka kedua yang tidak ada dilaporkan sebagai `absent`, bukan IP rekaan.

## 3. Bukti Runtime

Boot dari disk hasil build (`target/rinjani-1.0.0-x86_64-disk.raw`, SHA256 `597399F61C2E2A9B…D857091`), konsol serial `Analysis/logs/webui-boot6-console.log`:

```
[R28] DHCP BOUND eth0=10.10.MM.NNN/24 gw=10.10.MM.1 lease=1800 s — WebUI siap pada alamat WAN
[R28] HTTP /views/system.html     -> 200
[R28] HTTP /js/core/ui.js         -> 200
[R28] HTTP /api/v1/interfaces     -> 200 (504 bytes dibangun)
[R28] HTTP /api/v1/services       -> 200 (415 bytes dibangun)
```

Nilai yang benar-benar dirender browser (dibaca dari DOM, bukan asumsi):

| Modul | Bukti isi |
|---|---|
| System Overview | `status="operational"`, Kernel Base `ASL Core Kernel v1.0.0`, Distribusi `Rinjani 1.0.0`, Hostname `rinjani`, Arsitektur `x86_64`, Uptime `1 menit 10 detik`, Tautan WAN `UP`, Sumber `dhcp`, HTTP Dilayani `184` |
| Antarmuka Jaringan | `eth0 wan_bridged up 08:00:27:XX:XX:XX 10.10.MM.NNN 255.255.255.0 10.10.MM.1 dhcp 2232/791` dan `eth1 lan_hostonly absent — — — — — — —` (pil `pill-ok` / `pill-neutral`) |
| Kebijakan Firewall | note `YA`, `DENY (bawaan)`, Aturan TCAM `1`, DNAT `1`, WAN Terlindungi `aktif`, Port Manajemen `80`, Port Terbuka `YA` |
| Registri Layanan | Server HTTP `AKTIF 10.10.MM.NNN:80`, Sesi TCP aktif `5` (maks 8 slot), Klien DHCP `AKTIF`, Konsol Serial CLI `AKTIF`, Proksi DNS `TIDAK AKTIF`, Perespon mDNS `TIDAK AKTIF` |

Gerbang HTTP tambahan (probes `curl` langsung ke kernel):

```
validator cocok   -> 304        (If-None-Match: "rinjani-js-ui-ui01-6838")
daftar koma       -> 304        (If-None-Match: "x-1", "rinjani-js-ui-ui01-6838")
label lama        -> 200        (If-None-Match: "rinjani-js-ui-v103")  <- inti perbaikan §4.2
HEAD /            -> 200        POST /api/v1/system -> 405
```

Konsol browser: **nol pesan** (tidak ada error/throw). Toggle mini-rail: `body.rail` aktif/non-aktif benar dan bertahan lewat `localStorage["rinjani-rail"]`. Catatan alat: `take_screenshot` pada browser bawaan terhalang `NATIVE_BROWSER_VIEWPORT_UNAVAILABLE`, dan viewport harness 831 px membuat sidebar disembunyikan media query `max-width:900px` (perilaku desain, bukan cacat) — karena itu bukti visual diambil dari snapshot aksesibilitas + kueri DOM.

## 4. Cacat yang Ditemukan Saat Verifikasi (bukan sekadar gaya)

### 4.1 Pengikat melewatkan elemen akar — `VERIFIED`, DIPERBAIKI
Akar parsial adalah `<section class="view" data-source="…">`. `querySelectorAll` hanya memuat keturunan, sehingga `sources()` mengembalikan daftar kosong: `ui.refresh()` melaporkan **sukses tanpa menggambar apa pun**. Gejalanya menipu — chrome lengkap, footer menulis "sinkron", semua nilai kosong. Perbaikan: helper `listIn()` yang menyertakan akar bila akar sendiri cocok dengan selector, dipakai di `sources()` dan `paint()`. Test regresi: `test_05b` di `asl-router-webui/src/lib.rs` menagih tiap parsial tetap memuat pengikatnya.

### 4.2 Validator cache tidak berubah saat isi aset berubah — `VERIFIED`, DIPERBAIKI
ETag adalah label statis (`rinjani-js-ui-v103`). Setelah isi `ui.js` disunting, klien yang punya salinan lama mengirim `If-None-Match` dengan label yang **masih sama**, kernel menjawab 304, dan browser terus menjalankan JS basi — persis kegagalan §4.1 tidak hilang walau server sudah mengirim berkas baru. Perbaikan di `asl-router-webcore/src/lib.rs`:
- validator dikirim sebagai `"<label>-<panjang isi>"`, disusun di stack tanpa heap (`build_validator`, penyangga 96 B);
- `If-None-Match` kini dicocokkan sebagai **daftar koma** (RFC 7232 §3.2) lewat `validator_list_matches`;
- bila label terlalu panjang sehingga validator tidak tersusun, server **wajib melayani 200 penuh** — kegagalan penyusun validator tidak boleh bersembunyi di balik 304;
- tiga test baru: `test_04` (validator berpasang panjang), `test_04b` (daftar koma), `test_04c` (isi berubah ⇒ tidak ada 304).

Label aset sekaligus diganti `-v103` → `-ui01`: `v103` menyerupai klaim versi produk, padahal riwayat 1.0.3 sudah dipensiunkan dan versi resmi tetap **Rinjani 1.0.0**. Label kini murni identitas revisi aset; pembatalan cache ditanggung oleh bagian panjang isi, sehingga label tidak perlu dinaikkan setiap kali menyunting berkas.

### 4.3 Satuan uptime ambigu — DIPERBAIKI
Format lama `0j 4m 18d` menabalkan `m` (menit?) dan `d` (detik?). Operator membaca baris ini untuk menilai kesegaran mesin, jadi satuan ditulis penuh: `1 menit 10 detik` (bagian jam/menit hilang bila nol).

## 5. Gerbang Verifikasi (Semua Hijau)

| Gerbang | Perintah | Hasil |
|---|---|---|
| Format | `cargo fmt --all` | exit `0` |
| Unit/integration | `cargo test --workspace` | exit `0` — **258 lulus / 0 gagal** (bertambah 3 test webcore dari §4.2) |
| Bare-metal | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | exit `0` (hanya warning lama crate `virtio-blk`: `inb/inw/outd` tak terpakai, `cap`) |
| Image | `scripts/build-disk.ps1` | `DISK IMAGE BUILD SUCCESSFUL`, MBR `0x55AA`, PMBR `0xEE`, `EFI PART` di LBA1 |
| Runtime | boot VM headless + browser nyata | lihat §3, nol error konsol |

## 6. Item Terbuka

1. **Bukti visual penuh belum diambil (`NOT RUN`).** Tangkapan layar terhalang viewport harness; verifikasi memakai snapshot aksesibilitas dan kueri DOM. Perlu satu sesi pada viewport ≥ 1000 px untuk menilai sidebar, mini-rail, dan zebra tabel secara visual.
2. **Modul yang masih abu-abu (`BLOCKED` di kernel).** Rute & gateway, lease DHCP, tabel ARP, jingkar paket, ping/traceroute, QoS, dan konfigurasi menunggu endpoint `/api/v1/*` yang nyata; daftar di sidebar wajib tetap jujur, bukan halaman kosong.
3. **TTL cache parsial view.** `/views/*.html` hari ini divalidasi ulang per permintaan (no-cache). Bila operator menuntut 0 permintaan ulang saat offline, keputusan desainnya belum diambil.
4. Backlog sprint sebelumnya tidak berubah (jalur RX `dhcp_server_pool`/`dns_proxy`, egress ICMP echo agar `ping` benar, uji runtime `wan static` + DAD, mojibake em dash `[R1]`) — lihat `RND_20_WAN01_REPORT.md` §8 dan `RND_21_BOOT01_REPORT.md` §6.

## 7. Status Commit

Kerja BOOT-01 (`RND_21`) dan UI-01 (laporan ini) **masih uncommitted** dan menunggu izin owner. Staging wajib per-path (`router/asl-router-webui/assets/**`, `router/asl-router-webui/src/*`, `router/asl-router-webcore/src/lib.rs`, `asl-kernel/asl-kernel-bin/src/main.rs`, `docs/RND/*`) — **jangan** `git add -A` karena `Analysis/reports/is02_rjcfg3.log` ter-track dan memuat IP internal riil.
