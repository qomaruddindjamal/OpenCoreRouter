# AUD-01 TEST RESULTS — gerbang kompilasi, unit, dan bukti runtime

> **Document ID:** `docs/RND/AUD01_TEST_RESULTS.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Lingkup:** sprint AUD-01 (9 cacat diperbaiki) pada VM dev `"Open Core Router"`, satu NIC bridged, alamat WAN DHCP tersamar `10.10.MM.NNN`.

## 1. Gerbang wajib (semua dieksekusi, exit code nyata)

| Gerbang | Perintah | Hasil |
|---|---|---|
| Format | `cargo fmt --all` | `exit 0` |
| Unit workspace | `cargo test --workspace` | `exit 0` — **303 passed / 0 failed / 0 ignored**, 64 blok hasil |
| Bare-metal check | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | `exit 0`, 1 warning `dead_code` (pub width driver virtio, sudah ada sebelumnya) |
| Build kernel | `cargo build -p asl-kernel-bin --target x86_64-unknown-none` | `exit 0`, ELF 3 139 344 B |
| Build disk | `scripts/build-disk.ps1 -Profile debug` | `exit 0`, RAW SHA256 `87CF9CE2…154B3766` (build pertama) lalu `C4737817…620F1F5F` (build terminal) |

Per-crate untuk tiga crate yang disentuh: `asl-router-webcore` 10, `asl-router-api` 34, `asl-router-webui` 28.

## 2. Tes baru yang lahir dari penelusuran rantai (bukan dari fitur)

| Tes | Crate | Yang dijaga |
|---|---|---|
| `test_09_response_status_reads_the_emitted_line` | webcore | 9 status + `finish_not_modified` terbaca benar dari baris status |
| `test_09b_response_status_rejects_non_responses` | webcore | buffer bukan-respons → `0`, tidak pernah mengarang angka |
| `test_04b_firewall_protected_requires_datapath` | api | `wan_protected` hanya `true` bila classifier benar-benar ditanya |
| `test_04c_lan_status_datang_dari_alamat_bukan_literal` | api | `0.0.0.0` → `down/false`; beralamat → `up/true` |
| `test_05h_kejujuran_status_dan_submenu_layar_sentuh` | webui | token penjaga di JS_UI/JS_APP/VIEW_SYSTEM/CSS_WEBFIG/JS_NAV/VIEW_FIREWALL |
| `test_05i_status_log_dibaca_dari_baris_status_respons` | webui | melarang literal `last_status = 200`; wajib 2 panggilan `response_status` |
| `test_20_conditional_get_dilaporkan_304_bukan_200` | webui | TCP penuh + `If-None-Match` → `last_status == 304` dan body `HTTP/1.1 304` |
| `test_05j_penolakan_bertingkat_dan_footer_halaman_kerja` | webui | padanan kata-depan tabel NO, dua `greet(cur)`, pembeda footer halaman kerja |

## 3. Bukti runtime — kernel (konsol serial `Analysis/logs/aud01-console.log`, tidak di-commit)

| Bukti | Keluaran nyata |
|---|---|
| Banner doktrin | `[R1] RINJANI 1.0.0 — Open Core Router` / `[R2] Powered by ASL Core Kernel v1.0.0` |
| WAN nyata | `[R28] DHCP BOUND eth0=10.10.MM.NNN/24 gw=10.10.MM.1 lease=1800 s` |
| Status tercacat benar | histogram sesi: `992×200`, `160×304`, `37×429`, `4×400`, `2×403` |
| Conditional GET | `[R28] HTTP /views/firewall.html -> 304 (138 bytes dibangun)` (sebelum AUD-01 baris ini tertulis `-> 200`) |
| Jalur tulis | `[R28] POST jalur tulis -> 403` (tanpa arm), `-> 400` (validasi), `-> 200` (arm diterima) |

## 4. Bukti runtime — HTTP mentah

```
GET  /api/v1/firewall/rules -> {"wan_protected":false,"rules_enforced":false,"datapath":"engine-only",...}
GET  /api/v1/system        -> {"distro":"Rinjani 1.0.0","kernel":"ASL Core Kernel v1.0.0","hostname":"rinjani",...}
GET  /api/v1/interfaces    -> 1 baris eth0 nyata (mac 08:00:27:XX:XX:XX, 10.10.MM.NNN/24, dhcp, rx/tx riil)
GET  /api/v1/nat           -> {"masquerade_active":true,"forwarding_wired":false,"forwarding":"engine-only","active_sessions":0,...}
GET  /api/v1/olt           -> {"operational":false,"reason":"gpon/olt crates are markers without datapath"}
GET  /api/v1/wireless      -> {"radios":0,"stations":0,"operational":false,"reason":"no wireless driver in this build"}
POST /api/v1/power  action=restart (tanpa arm) -> 403 power_not_armed
POST /api/v1/power  action=arm                 -> 200 {"status":"armed","valid_for_s":20}
POST /api/v1/interfaces/eth0 mode=static&ip=…  -> 400 static_requires_netmask
POST /api/v1/interfaces/eth0 mode=bogus        -> 400 mode_invalid
```

Aksi restart/shutdown **tidak** dieksekusi ke guest pada sesi ini (mandat §14: tidak menjalankan daya pada host dev tanpa persetujuan); yang dibuktikan adalah gerbang penolakan dan jendela arming.

## 5. Bukti runtime — peramban (harness 831×785, `visibilityState=hidden`)

| Probe | Hasil |
|---|---|
| Sinkron project ↔ VM | **49/49 aset identik byte-per-byte** (`wc -c` lokal vs `size_download` kernel; ETag ikut cocok, mis. `/js/core/term.js` → `"rinjani-js-term-ui02i-7875"`) |
| Inventaris navigasi | 28 item (26 `data-view` + 2 `data-power`) — sama dengan mandat §3 |
| Sweep 26 view | 0 error JavaScript (`window.addEventListener('error')` kosong); tiap view berganti judul & sumber dengan benar |
| Footer jujur | `#arp`, `#qos`, `#speedtest`, `#appearance` → "halaman rupa: 0 endpoint kernel"; `#system`, `#firewall`, `#nat`, `#serviceslist` → "sinkron HH:MM:SS (segar 5 s)" |
| Uptime vs Find/Filter | sebelum "2 menit 13 detik" → setelah Find "2 menit 13 detik" (tetap) + readout "0/1 baris tampil untuk uptime" |
| Firewall jujur | baris "WAN terlindungi **TIDAK**" dan "Aturan ditegakkan di jalur data **TIDAK**" |
| Layar sentuh | `matchMedia('(hover: none)')` distubkan via `initScript`; empat tap induk → open/close bergantian, `location.hash` tidak berubah, submenu terlihat (`subVisible:true`) |
| Terminal CLI | `/system resource print` dan `/interface print` mencetak balasan kernel; `/tool ping 10.10.MM.NNN`, `/tool traceroute 1.1.1.1`, `/tool bandwidth-test …`, `/ip dns print` semuanya dibalas **sebab teknis**, bukan "tidak dikenal" |
| Kepala sesi terminal | `term-host` = "rinjani - Rinjani 1.0.0 - operational" setelah reload ignoreCache |
| Modal Interfaces | dobel-klik baris → `#iface-overlay.wf-overlay open`; field terisi dari kernel; mode dhcp → ip/mask/gw `disabled`; mode static → ketiganya aktif + hint aturan validasi |

## 6. Batas bukti (kejujuran)

- **D05 (status LAN)** hanya terverifikasi lewat unit test: VM dev punya satu NIC sehingga cabang LAN tidak tereksekusi di runtime.
- Konfirmasi rupa visual oleh mata operator tetap `BLOCKED`; harness tidak menghasilkan screenshot dan tab-nya tersembunyi, jadi bukti berupa DOM + computed style, bukan tampilan.
- Artefak harness yang **bukan** cacat produk: transisi CSS beku saat tab tersembunyi membuat `getComputedStyle(sidebar).transform` tidak pernah maju; dengan `transition:none` kelas `body.nav-open` terbukti menggeser laci ke `left:0` (dicatat sebagai `ART-01` agar tidak divonis bug).
