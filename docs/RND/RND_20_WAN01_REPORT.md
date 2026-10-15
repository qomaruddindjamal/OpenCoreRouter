# LAPORAN SPRINT WAN-01: WEBUI TERPASANG & DILAYANI DARI ALAMAT WAN (SATU NIC BRIDGED)

> **Document ID:** `docs/RND/RND_20_WAN01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-15
> **Status:** `RUNTIME VERIFIED` (WebUI benar-benar diakses dari alamat WAN eth0 pada hardware nyata)
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `8c17314` (`feat(rinjani-1.0.0): in-kernel self-installer, ATA multi-drive & two-way serial console [is03]`)
> **Doktrina:** ASL CORE BUKAN LINUX — TCP (RFC 9293), HTTP/1.1 (RFC 7230), DHCP client (RFC 2131), DAD (RFC 5227) dan driver virtio-net ditulis native `#![no_std]`; tidak ada smoltcp, tidak ada kode Linux yang diimpor.
> **Redaksi:** seluruh alamat IP internal dan oktet akhir MAC ditulis tersamar (`10.10.MM.NNN`, `08:00:27:XX:XX:XX`) sesuai kebijakan Zero Secrets in Repository.

---

## 1. Ringkasan Eksekutif

Permintaan owner: **satu adapter NIC yang di-bridge**, dan WebUI harus **terpasang serta berjalan dari alamat WAN** interface itu. Sebelumnya WebUI hanya "hidup" di alamat host-only eth1 yang pada topologi ini tidak ada, sehingga secara fungsional tidak tercapai dari jaringan nyata.

Sprint ini menutup gap tersebut dan membuktikannya di wire:

1. **Aset WebUI terpasang inside kernel image (`VERIFIED`):** seluruh frontend (`shell.html`, 3 CSS, 2 JS, `vendor/htmx.js`, `logo.svg` = 16.574 B) di-embed lewat `include_bytes!` ke `asl-kernel`, sehingga instalasi kernel ke ESP **adalah** instalasi WebUI — tidak ada stage filesystem terpisah. Bukti: GET `/` menghasilkan 2.582 B dan `/js/status.js` 7.165 B **byte-exact** terhadap asset di repositori (`cmp` = identik).
2. **DHCP client nyata sampai BOUND pada eth0 bridged (`VERIFIED`):** `DISCOVER → OFFER → REQUEST → ACK` terjadi pada wire nyata dengan CPE produksi. Alamat WAN yang diterima menjadi alamat bind HTTP: `[R28] DHCP BOUND eth0=10.10.MM.NNN/24 gw=10.10.MM.1 lease=1800 s — WebUI siap pada alamat WAN`.
3. **TCP/HTTP server native menyajikan dari alamat WAN (`VERIFIED`):** `[R18] TCP/HTTP ENGINE NATIF UP port=80 sesi_maks=4`, handshake nyata tercatat di konsol (`[R28] TCP handshake selesai (ESTABLISHED)`), dan API menjawab dengan data runtime kernel (`http_bind_ip: 10.10.MM.NNN`, `rx_packets`, `uptime_ms`, `requests_served`).
4. **Frontend dibongkar dari htmx menjadi JavaScript native `#![no_std]`-friendly (`VERIFIED`):** `assets/js/status.js` ditulis ulang (7.250 B) — XHR + `JSON.parse` + DOM builder eksplisit dengan `textContent` (tanpa sink HTML), routing client-side 4 view, auto-refresh 5 s. Header konsol kini menampilkan alamat bind aktual hasil baca kernel: `bind 10.10.MM.NNN:80`.
5. **Kejujuran log dan telemetri (`VERIFIED`):** blok fabrikasi pada `loop_count == 100` dihapus dan diganti mode bukti yang menghitung RX/TX nyata; `ping` diberi status `TIDAK DIIMPLEMENTASIKAN` (tidak ada egress ICMP echo maupun reply-matcher, jadi setiap angka RTT akan palsu); `show interface/route/nat/tcam/hardware/counter/dhcp pool/dns cache` dibaca dari struktur kernel aktual; `[R20]`/`[R21]` tidak lagi mengklaim `READY`/`UP` untuk mesin yang jalur RX-nya belum ada.
6. **CLI `wan show` / `wan static` (`VERIFIED` untuk `wan show`; `wan static` `DOCUMENTED` — jalur DAD tersedia tetapi belum dijalankan pada sprint ini):** `wan show` mencetak keadaan nyata dari UART:

   ```
   WAN (eth0) state:
     address : 10.10.MM.NNN/24
     gateway : 10.10.MM.1
     source  : dhcp
     link    : UP, dad_pending=no, conflict=no
     http    : listening=yes port=80 sessions=0 served=28
   ```

---

## 2. Artefak & Hash (build yang dieksekusi VM)

| Artefak | Nilai |
| :--- | :--- |
| Kernel ELF `target/x86_64-unknown-none/debug/asl-kernel` | SHA256 `34a791222543fb66…`, 2.652.200 B, mtime 2026-10-15 19:12:47 |
| Disk raw `target/rinjani-1.0.0-x86_64-disk.raw` | SHA256 `7380f62964dbfe0e…`, 2 GiB (GPT + FAT32 ESP + Limine) |
| VDI boot VM | `target/rinjani-1.0.0-x86_64-disk.vdi`, 9.437.184 B, UUID dikunci `aedff65b-…e3dd97a11af1` agar attachment IDE VM tetap valid |
| Topologi VM | 1 NIC: `nic1=bridged` pada `Realtek PCIe GbE Family Controller`, `nictype1=virtio`; konsol `--uart1 0x3f8 4 --uartmode1 tcpserver 4555` |
| Unit test workspace | `255 passed / 0 failed` (naik dari 239 pada IS-03) |
| Gate no-std | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` → `Finished`, 0 error, 1 warning pre-existing (`power.rs` field tak terpakai) |
| Gate format | `cargo fmt --all` → exit 0 |
| Log konsol mentah (tidak di-commit) | `Analysis/logs/webui-wan4-console.log` |

---

## 3. Bug Nyata Yang Hanya Muncul Di Hardware (dan akarnya)

### 3.1 `/js/status.js` menjawab 500 — bukan masalah cache browser

Dugaan awal pada sesi sebelumnya adalah "browser menampilkan versi lama karena cache". **Dugaan itu salah dan dikoreksi dengan bukti:** `curl -D` terhadap kernel mengembalikan

```
HTTP/1.1 500 Internal Server Error … Content-Length: 27
```

untuk `/js/status.js`, sementara `/` melayani 200. Akar masalah: `status.js` hasil rewrite berukuran **7.165 B**, sedangkan buffer respons per sesi `WEB_RESP_MAX = 4096`; `HttpResponseBuilder` mengembalikan `"Buffer capacity exceeded"` dan kernel menjawab 500. Panel statis yang terlihat sebelumnya adalah **gejala script gagal dimuat**, bukan cache.

Perbaikan:
- `WEB_RESP_MAX` dinaikkan ke **8192** (`router/asl-router-webui/src/lib.rs`). Alokasi struktur ikut menyesuaikan otomatis karena kernel menghitung halaman dari ukuran tipe: `(size_of::<TcpWebServer>() + 4095)/4096`.
- **Gerbang waktu compile** ditambahkan agar kelas bug ini tidak bisa kembali lolos ke wire:

  ```rust
  const fn largest_asset_len() -> usize { /* maks atas registry::ALL_ASSETS */ }
  const HTTP_HEADER_BUDGET: usize = 256;
  const _: () = assert!(largest_asset_len() + HTTP_HEADER_BUDGET <= WEB_RESP_MAX);
  ```

  Gerbang ini **diverifikasi negatif**: dengan `WEB_RESP_MAX = 4096` kompilasi gagal dengan `error[E0080]: evaluation panicked: assertion failed: largest_asset_len() + HTTP_HEADER_BUDGET <= WEB_RESP_MAX`; dengan 8192 kompilasi lolos. Jadi kegagalan kini terjadi di build, bukan di hadapan user.

### 3.2 `Cache-Control: public, max-age=3600` membuat ETag tidak pernah dilihat

Aset statis disajikan dengan `max-age=3600`, sehingga browser tidak merevalidasi selama 1 jam dan **bump ETag tidak pernah sampai ke klien**. Untuk konsol yang isinya ter-compile ke dalam image kernel (URL tetap, isi berubah tiap build), ini salah secara semantik.

Perbaikan di `asl-router-webcore`: aset ber-ETag kini dikirim `Cache-Control: no-cache, must-revalidate` — browser selalu merevalidasi, kernel menjawab `304` bila ETag sama. Bukti runtime:

```
GET /js/status.js            -> 200, ETag "rinjani-js-status-v102", Content-Length 7165, Cache-Control: no-cache, must-revalidate
GET /js/status.js (INM sama) -> 304 Not Modified, Content-Length: 0
```

### 3.3 `#mgmt-bind` terhapus oleh aplikasinya sendiri

`loadManagementBind()` menulis ke `#mgmt-bind`, tetapi elemen itu berada **di dalam** `#main-panel` yang di-clear oleh `show()` pada render pertama; hasil pemeriksaan DOM: `mgmtBind: null`. Elemen dipindah ke header (`.system-badge`, flex dengan gap) sehingga persisten; verifikasi: `mgmtBind: "bind 10.10.MM.NNN:80"`. Label antarmuka juga dibuat jujur — tidak menyebut nama interface yang tidak diamati, hanya alamat bind dari kernel.

### 3.4 DHCP Option 54 — router nyata men-NAK REQUEST kita

Transeksi DORA pertama selalu mati di `REQUEST → NAK`. Root cause: `build_dhcp_request_with_hostname` tidak menyertakan **Option 54 (Server Identifier)**, yang menurut RFC 2131 §4.3.2 wajib ada pada REQUEST dalam fase `SELECTING`; server nyata memakai opsi itu untuk memverifikasi bahwa klien masih memilih offer-nya. Setelah opsi ditambahkan (buffer 240 + 16 + host_opt_len), BOUND tercapai pada boot berikutnya. Regresi dikunci di `test_19` dengan `assert_eq!(dhcp.server_ip, Some(srv_ip));`.

### 3.5 Klaim service yang tidak jujur

- `services.dhcp_client` sebelumnya `!is_bound() && !is_halted()`, sehingga klien yang **berhasil binding** justru dilaporkan `false` (panel menampilkan "DHCP CLIENT: tidak aktif" padahal lease dipegang). Kini `!wan_static_mode && !dhcp_client.is_halted()`: aktif selama mesin memegang siklusnya (termasuk lease BOUND), `false` saat fail-closed atau saat alamat diambil dari `wan static`. Verifikasi runtime: `"dhcp_client":true` bersamaan dengan `wan_source:"dhcp"`.
- `[R20]`/`[R21]` diubah dari `READY`/`UP` menjadi `SIAP … eth1=absent, BELUM ADA JALUR RX — pool tidak melayani` / `proxy tidak melayani query`; `dns_proxy:false` kini konsisten dengan log.
- `handle_system` di `asl-router-api`: status `"carrier-grade"` (klaim mutu) → `"operational"` (keadaan terukur).

---

## 4. Matriks Bukti Runtime (host 10.10.MM.170 → VM 10.10.MM.NNN pada LAN nyata)

| Permintaan | Hasil |
| :--- | :--- |
| `GET /` | `200`, 2.582 B, ETag `rinjani-shell-v102` |
| `GET /css/base.css`, `/css/layout.css`, `/css/components.css` | `200` 1.171 / 1.536 / 1.536 B |
| `GET /js/common/opcr.js` | `200` 398 B |
| `GET /js/status.js` | `200` **7.165 B**, byte-identik dengan asset repo (`cmp` bersih) |
| `GET /images/logo.svg` | `200` 820 B |
| `If-None-Match` ETag sama | `304 Not Modified`, `Content-Length: 0` |
| `GET /api/v1/system` | `200` 211 B JSON |
| `GET /api/v1/interfaces` | `200` 271 B JSON |
| `GET /api/v1/services` | `200` 187 B JSON |
| `GET /api/v1/firewall/rules` | `200` 132 B JSON |

Cuplikan JSON (kernel nyata, bukan fixture):

```json
{"ok":true,"data":[{"name":"eth0","mode":"wan_bridged","ip":"10.10.MM.NNN","netmask":"255.255.255.0",
"gateway":"10.10.MM.1","source":"dhcp","status":"up","rx_packets":837,"tx_packets":68},
{"name":"eth1","mode":"lan_hostonly","status":"absent"}]}

{"ok":true,"data":{"dhcp_client":true,"http_server":true,"dns_proxy":false,"serial_cli":true,
"http_port":80,"tcp_sessions_active":1,"http_bind_ip":"10.10.MM.NNN"}}

{"ok":true,"data":{"default_policy":"deny","tcam_rules":1,"dnat_rules":1,
"wan_protected":true,"http_port_open":true,"http_port":80}}
```

Bukti browser (bukan hanya curl) atas konsol yang dirender dari data kernel:

- Snapshot DOM: header `rinjani` + `bind 10.10.MM.NNN:80`; panel Dashboard berisi `UPTIME (PIT 8254)`, `WAN LINK: UP`, `ALAMAT BERSUMBER DARI: dhcp`, `HTTP DILAYANI: 15`, `STATUS: operational`.
- Routing client-side (event `click` pada tiap `.nav-item[data-view]`) menghasilkan panel nyata:
  - `interfaces` → dua kartu, eth0 `up` dengan MAC/IP/GW/sumber/`RX PAKET 837`, eth1 `absent` dengan `-`.
  - `firewall` → `deny`, TCAM 1, DNAT 1, `WAN TERLINDUNGI: aktif`, port 80 `terbuka untuk manajemen`.
  - `services` → HTTP `aktif`, bind `10.10.MM.NNN:80`, sesi TCP 1, DHCP client `aktif`, DNS proxy `tidak aktif`.
- Console browser: hanya satu pesan log (`Rinjani 1.0.0 WebUI Status Driver Initialized`), **nol error JS**, nol request gagal.

Konsol serial mencatat sisi kernel atas sesi yang sama:

```
[R28] TCP handshake selesai (ESTABLISHED)
[R28] HTTP /api/v1/services -> 200 (415 bytes dibangun)
[R28] TCP sesi ditutup peer — slot dibebaskan
[R28] WAN ARP/ICMP REPLY TX (42 bytes) wan_rx=2117 wan_tx=96
```

---

## 5. Taksonomi Bukti

| Klaim | Label |
| :--- | :--- |
| WebUI tercapai dan ter-render dari alamat WAN eth0 (browser + curl) | `VERIFIED` |
| Aset terpasang dalam image kernel di ESP dan terkirim byte-exact | `VERIFIED` |
| DORA lengkap dengan Option 54 pada CPE produksi; lease 1800 s | `VERIFIED` |
| Revalidasi ETag → 304 dan tidak ada lagi UI basi | `VERIFIED` |
| Gerbang compile ukuran aset (gagal saat 4096) | `VERIFIED` (negatif-tes `E0080`) |
| `wan static` + DAD probe | `DOCUMENTED` (kode + jalur DAD ada; belum dijalankan pada topologi ini) |
| Forwarding eth1→eth0 (`show counter Forward`) | `BLOCKED` — eth1 tidak ada pada topologi satu NIC; dicetak `0 (path eth1->eth0 belum diwire; bukan angka karangan)` |
| `ping` dari CLI kernel | `NOT RUN` / tidak diimplementasikan secara sengaja (tidak ada egress ICMP echo) |
| DHCP server pool & DNS proxy melayani klien | `BLOCKED` — belum ada jalur RX; dilaporkan apa adanya di log dan API |

---

## 6. Gates

| Gate | Perintah | Hasil |
| :--- | :--- | :--- |
| Format | `cargo fmt --all` | exit 0 |
| Unit test | `cargo test --workspace` | **255 passed / 0 failed** |
| Kontrak no-std | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | `Finished`, 0 error |
| Gerbang ukuran aset | `cargo check -p asl-router-webui` dengan buffer sengaja dikecilkan | `E0080` (diharapkan), lolos pada 8192 |
| Runtime | Boot VM dari VDI build ini, DHCP BOUND, HTTP tercapai dari host di LAN | `VERIFIED` |

---

## 7. Catatan Tata Kelola & Keamanan (tindak lanjut, bukan klaim)

1. **`Analysis/reports/is02_rjcfg3.log` adalah file tracked dan kini membengkak ±3,7 MB** berisi capture konsol yang memuat IP/MAC internal riil. Versi di `HEAD` hanya memuat MAC lokal VirtualBox (`08:00:27:XX:XX:XX`), belum IP. File ini **sengaja tidak di-commit** pada sprint ini; perlu keputusan tata kelola: pindahkan capture mentah ke `Analysis/logs/` yang tidak tracked dan tambahkan `.gitignore`, atau buat kebijakan redaksi sebelum capture apa pun boleh masuk repo.
2. **Direktori asing `Python/` ada di root** (`_cache`, `_pythoncore-3.15-64`, `pythoncore-3.15-64.12.deleteme`) dan tidak tracked. Ini melanggar aturan "Root Project Wajib Bersih"; bukan artefak sprint ini, sehingga dibiarkan dan dilaporkan, tidak dihapus tanpa izin owner.
3. **`docs/ROADMAP/CHECKLIST_STATE.json` gate `OPCR-76` masih `REGISTERED-NOT-ATTEMPTED`** padahal IS-03 memuat bukti self-installer `VERIFIED`. Needsinkronisasi gate dilakukan oleh proses governance, bukan diam-diam oleh sprint ini.
4. Gerbang baru yang diajukan untuk PASS-LOCKED: `OPCR-77` (WebUI tersaji dari alamat WAN, satu NIC bridged), `OPCR-78` (DHCP client DORA + Option 54 pada CPE nyata), `OPCR-79` (revalidasi ETag & gerbang ukuran aset statis).

---

## 8. Langkah Berikutnya

1. Wire jalur RX untuk `dhcp_server_pool` dan `dns_proxy` agar klaim `[R20]`/`[R21]` bisa berbalik menjadi `UP` secara jujur — saat ini keduanya sengaja dilaporkan tidak melayani.
2. Implementasikan egress ICMP echo request + reply-matcher supaya `ping` bisa dibuka kembali dengan angka yang berarti.
3. Uji `wan static <ip> <mask> <gw>` pada LAN nyata (jalur DAD sudah ada, buktinya belum dicetak).
4. Sinkronkan `CHECKLIST_STATE.json` dan `DECISION_REGISTER.md` dengan temuan Option 54 (RFC 2131 §4.3.2) yang sempat membuat DORA gagal senyap.
