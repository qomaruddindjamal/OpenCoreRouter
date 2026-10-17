# AUD-01 TEST RESULTS — gerbang kompilasi, unit, dan bukti runtime

> **Document ID:** `docs/RND/AUD01_TEST_RESULTS.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Lingkup:** sprint AUD-01 (9 cacat diperbaiki) + bagian AUD-02 (audit ulang sampai WebUI berfungsi) pada VM dev `"Open Core Router"`, satu NIC bridged, alamat WAN DHCP tersamar `10.10.MM.NNN` / gerbang `10.10.MM.GGG`.

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

---

# BAGIAN AUD-02 — audit ulang sampai berfungsi (bukti di bawah berasal dari eksekusi nyata, bukan dari bagian AUD-01)

## 7. Gerbang akhir AUD-02 (semua dieksekusi ulang pada kondisi repo terakhir)

| Gerbang | Perintah | Hasil nyata |
|---|---|---|
| Format | `cargo fmt --all -- --check` | `exit 0` |
| Unit workspace | `cargo test --workspace` | `exit 0` — **390 passed / 0 failed / 0 ignored** (33 blok hasil; tidak ada tes yang dihapus, diturunkan, atau di-`#[ignore]`) |
| Bare-metal check | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | `exit 0`, 1 warning `dead_code` warisan |
| Sinkron project ↔ VM | bandingkan byte tiap aset terhadap aset yang dikirim kernel | aset terkirim identik; `shell.html` 7 731 B, `js/core/ui.js` 7 711 B |

Per-crate milik tiga crate yang disentuh AUD-02: `asl-router-api` **78**, `asl-router-webui` **36**, `asl-router-webcore` **10**.

## 8. Tes baru AUD-02 dan cara anti-vakumnya

| Tes | Crate | Yang dijaga | Validasi bukan-tes-vakum |
|---|---|---|---|
| `test_35b_seluruh_jalur_baca_menghasilkan_json_berimbang` | api | 18 jalur baca wajib menghasilkan sampul `{"ok":…}` dengan kurung berimbang | cacat `]}}` dimasukkan kembali ke sumber → tes **FAILED** dengan pesan `"/api/v1/auth/users JSON tidak berimbang: {'ok':true,'data':[{...}]}}"`; sesudah dikoreksi → lulus |
| `test_05bb_fmt_addr_membedakan_rute_default_dari_belum_terikat` | webui | pasangan `ui.js` punya pengubah `addr` **dan** `gateway.html` memakainya di kolom Tujuan; melarang kembali `data-fmt="ip"` di sel itu | pengubah `addr` dihapus sementara → tes gagal pada assertion pertama |

## 9. Rantai asal-usel lease DHCP — bukti tiga permukaan (IP intern disamar: klien `10.10.MM.NNN`, gerbang/DNS `10.10.MM.GGG`)

Permintaan owner: IP, netmask, gateway, dan DNS harus berasal dari sumber nyata saat mode DHCP client, tanpa nilai tambal, dan DNS harus tampak di modul DNS Server.

**a. Konsol serial** (`Analysis/logs/aud02-console.log`, tidak di-commit):
```
[R28] DHCP OFFER dari 10.10.MM.GGG ip=10.10.MM.NNN opsi1=ada opsi3=ada opsi6=ada
[R28] DHCP BOUND eth0=10.10.MM.NNN/24 (netmask sumber: option-1-ack) gw=10.10.MM.GGG (sumber: option-3-ack) dns=10.10.MM.GGG (sumber: option-6-ack)+1 8.8.8.8 lease=1800 s
[R21] DNS PROXY upstream=10.10.MM.GGG (sumber: option-6-ack)
[R30] Auth: 0 kredensial ... WebUI TERKUNCI: belum ada kredensial.
```
Baris `[R30]` adalah bukti fail-closed: boot tanpa kredensial menolak menyajikan sesi, bukan menyamar sebagai sistem terbuka.

**b. API kernel (bearer, curl nyata):**
```
GET /api/v1/dns          -> config_mode=dhcp, lease_source=dhcp, resolver_1=10.10.MM.GGG, resolver_2=8.8.8.8,
                            resolver_src=option-6-ack, from_dhcp=true, proxy_upstream=10.10.MM.GGG,
                            proxy_upstream_ready=true, serving=false, cache_entries=0, static_hosts=1
GET /api/v1/interfaces   -> baris eth0: netmask_src=option-1-ack, gateway=10.10.MM.GGG gateway_src=option-3-ack,
                            dns1=10.10.MM.GGG dns_src=option-6-ack
GET /api/v1/routes       -> 10.10.MM.0/24 connected  +  0.0.0.0/0 gw 10.10.MM.GGG static metric 100 (JSON.parse OK)
GET /api/v1/routes/stats -> default_active=true, default_gw=10.10.MM.GGG
GET /api/v1/arp/stats    -> entries=1, capacity=16
GET /api/v1/auth/users   -> {"ok":true,"data":[{"user":"admin","role":"admin","rounds":1024}]}
```
Tidak ada angka yang diwarisi dari sumber lain: `resolve_gateway` hanya membaca opsi-3 ACK lalu opsi-3 OFFER, dan penjaga DNS melarang fallback ke resolver publik bila lease tidak membawa opsi-6 (8.8.8.8 di atas memang datang dari opsi-6 lease, bukan tambalan).

**c. Peramban (DOM, bukan asumsi):**
| Modul | Nilai terikat yang terbaca di DOM |
|---|---|
| Interfaces | baris eth0 menampilkan netmask/gateway/DNS beserta label sumber `option-*-ack` |
| DNS Server | `resolver_1=10.10.MM.GGG`, `resolver_src=option-6-ack`, `proxy_upstream=10.10.MM.GGG` **AKTIF**, `serving=TIDAK AKTIF` (jujur: terima UDP/53 belum dirangkai) |
| Gateway | `["10.10.MM.0","24","belum terikat","eth0","0","connected"]` dan `["0.0.0.0","0","10.10.MM.GGG","eth0","100","static"]` — rute default tidak lagi dilabeli kosong |
| User Manager | baris `[, admin, admin, 1024]` muncul setelah cacat sampul JSON diperbaiki |
| Rail "belum aktif" | `#nav-offgroup` = **5** entri, semua memang belum terpasang; Rute & Gateway serta Tabel ARP dikeluarkan karena terbukti hidup |
| Terminal CLI | `/ip dns print` mencetak `resolver_1 : 10.10.MM.GGG`, `proxy_upstream_ready: yes`, `serving : no` |

Sapuan penuh **23 view SPA** menghasilkan 0 galat JavaScript. Modul berdata nyata: system, interfaces, wireless (jujur TIDAK), firewall (jujur "WAN terlindungi TIDAK"), nat, services, serviceslist, arp, dnsserver, gateway, olt (jujur TIDAK), usermanager, terminal. Modul rupa-saja dengan footer jujur: wireguard, vpn, xray, qos, dhcpserver, appearance, packages, speedtest, telnet, traceroute.

## 10. Batas bukti AUD-02 (kejujuran)

- Status **RUNTIME-VERIFIED** hanya diberikan untuk hal yang terbaca dari boot VM di atas (rentetan §9). Perbaikan yang hanya terbukti lewat unit test berstatus **UNIT-TESTED**.
- UDP/53 belum dirangkai ke mekanisme tangkap, karena itu `dns.serving=false` dan `dns_proxy=false` di `/api/v1/system`; proxy hanya terbukti pada hulu (`proxy_upstream_ready`), bukan pada pelayanan kueri.
- `/api/v1/leases` menjawab `not_found` — halaman DHCP Server tetap rupa-saja dan berada di rail.
- ICMP echo egress (#81), 802.1Q VLAN / shaper QoS / tangkap paket (#83), serta DEFER-05 & DEFER-06 (#84) belum dikerjakan; modul yang bergantung pada primitive itu **tidak** diaktifkan.
- Boot disk hasil build sengaja kosong kredensialnya; akun lab dibuat lewat bootstrap serial untuk pengujian dan tidak pernah di-commit. Sandi lab tidak dicatat di dokumen ini.
- Persistensi kredensial **lintas reboot tanpa rebuild disk** kini terukur (§11.3); klaim lama hanya bersandar pada satu siklus boot.
- Konfirmasi rupa oleh mata operator tetap `BLOCKED` (harness tanpa permukaan layar); bukti berupa DOM + computed style + byte aset, dan memerlukan reload `ignoreCache` untuk melihat aset baru karena cache peramban mengabulkan ETag lama.

## 11. AUD02-D05 & D06 — CRUD User Manager dan persistensi lintas reboot

Gerbang pada build ini: `cargo fmt --all -- --check` exit 0; `cargo test --workspace` **391 lulus / 0 gagal** (webui 37); `cargo check --target x86_64-unknown-none -p asl-kernel-bin` exit 0 dengan 1 warning `dead_code` warisan. Aset baru terbesar `js/core/useredit.js` **7 782 B**, di bawah anggaran 7 936 B dan terlayani utuh (`ETag "rinjani-js-useredit-aud02-7782"`, `Content-Length 7782`); `views/user-modal.html` 3 554 B; `views/usermanager.html` 4 021 B.

### 11.1 Putaran CRUD di VM (sesi admin, tanpa nilai karangan)

| Aksi | Permintaan nyata | Yang dibaca dari kernel | Status modal |
|---|---|---|---|
| Cipta | `POST /api/v1/auth/users action=set role=viewer` | baris baru `[nama, viewer, 1024]` pada `GET` daftar | `kernel: kredensial: tertulis dan terbaca kembali pada RJCFG01` (`wf-status good`), kedua isian sandi kembali kosong |
| Validasi pra-kirim | tidak ada POST dikirim | — | `kata sandi ditolak browser: 10-64 karakter` — browser menahan bentuk yang pasti ditolak, tidak menebak |
| Ubah | `action=set` dengan nama terkunci | role baris berpindah `viewer` → `admin` | verdict RJCFG01 yang sama; nama tidak dapat diedit karena ia kunci tabel kredensial |
| Hapus | `action=del` lewat tombol "Hapus akun" | baris hilang dari daftar kernel | verdict RJCFG01 dikutip, bukan disimpulkan |
| Penjaga | `action=del` pada akun terakhir | baris `admin` **tetap ada** | `ditolak kernel: last_account` (`wf-status bad`) — prose halaman kini menyebut kode ini |
| Cabut sesi | `action=revoke` | token sesi mati di kernel | `kernel: mencabut sesi (status applied, sesi cabut 1)`; `sessionStorage` token = 0, `role = null`, gerbang login muncul kembali |
| Peran lemah | `GET`/`POST` sebagai viewer | `403 insufficient_role` pada keduanya | footer `(insufficient_role)`, isian disabled, tombol tulis disembunyikan + teks kunci statis |

Dobel-klik baris dan tombol New/Edit/Remove membuka modal yang sama lewat `data-modal="user"`; Remove sengaja hanya membuka modal agar penghapusan menuntut penegasan tombol. `table.js` tidak diubah (7 790 B, mepet anggaran) dan tetap meneruskan aksi ke `OPCR.wf.actions`.

### 11.2 Perbaikan kejujuran yang lahir dari bukti runtime

Balasan `revoke` dari kernel **tidak** membawa `persist` (ia aksi sesi, bukan perubahan kredensial), tetapi modal dahulu mencetak `jurnal ?`. Kalimat status sekarang bercabang: `set`/`del` menampilkan `jurnal <persist>` lalu membaca ulang `/api/v1/writes/last`, `revoke` menampilkan `sesi cabut <N>` apa adanya. Penjelasan ini juga dipindah ke markup modal sehingga JS hanya mengutip, tidak menyusun klaim.

### 11.3 D06 — reboot tanpa rebuild disk

Prosedur: `POST /api/v1/power action=arm` → `action=restart` (penjaga dua langkah, `exec_in_ms 1500`), **tanpa** menjalankan `scripts/build-disk.ps1` sehingga RJCFG01 tidak tersentuh. Boot berikutnya pada konsol serial:

```
[R1] RINJANI 1.0.0 — Open Core Router
[R2] Powered by ASL Core Kernel v1.0.0
[R26] RJCFG01 journal DIBACA DARI DISK: slot 1 seq=6 (CRC slot + header terverifikasi, PIO nyata)
[R30] Auth: 1 kredensial dimuat dari RJCFG01, entropi token diaduk dari TSC+MAC+jurnal
```

Login dengan sandi yang sama setelah reboot berhasil: `role = admin`, token 64 karakter, tabel menampilkan `[admin, admin, 1024]`. Ini memisahkan bukti "tertulis" dari bukti "termuat saat boot berikutnya" yang sebelumnya `NOT RUN`.


## 12. UI-02J — halaman masuk mandiri, dropdown pengguna, dan kontras chrome sesi

Gerbang pada build ini: `cargo fmt --all --check` exit 0; `cargo test --workspace` **392 lulus / 0 gagal** pada **66** baris hasil (crate `asl-router-webui` **38** tes, termasuk `test_05l` yang baru); `cargo check --target x86_64-unknown-none -p asl-kernel-bin` `Finished` dengan 1 warning `dead_code` warisan (`power.rs:120`). `ALL_ASSETS` **52 → 54**; dua aset baru terlayani utuh dan terikat ETag yang panjangnya == `wc -c` repo: `ETag "rinjani-login-page-ui02j-5560"` / `Content-Length 5560` dan `ETag "rinjani-css-session-ui02j-6457"` / `Content-Length 6457`. Sisa ukuran mata rantai: `js/core/login.js` 5 823 B, `shell.html` 7 765 B (lencana `Community Edition` dicabut, margin gerbang naik dari 156 → **171**), `css/layout.css` 7 461 B (aturan `.chip-badge` ikut dilepas). Pemegang gerbang ukuran aset kini **`term.js` 7 877 B (margin 59)**, bukan shell.

### 12.1 Rantai sesi di VM (image `FD9B0E91…96852EA`, tanpa nilai karangan)

| Langkah | Permintaan nyata | Yang dibaca dari kernel / DOM | Status |
|---|---|---|---|
| Fail-closed sebelum bootstrap | `GET /api/v1/system` tanpa token | `[R30] Auth: 0 kredensial` + `[R30] WebUI TERKUNCI`; `[R28] HTTP /api/v1/system -> 401` | halaman masuk tetap terlayani `200`, data kernel tidak — gerbang di lapisan server, bukan peramban |
| Sandi salah | `POST` jalur tulis login | `HTTP /api/v1/auth/login -> 401 (280 bytes)` | note `lg-note bad` bertuliskan kode kernel: `… [invalid_credentials]`; kolom sandi dikosongkan **sebelum** permintaan berangkat |
| Login admin | `POST /api/v1/auth/login` | `-> 200 (379 bytes)` | `sessionStorage`: token `/^[0-9a-f]{64}$/` = true, role `admin`; `location.replace("/")` |
| Dropdown | klik `#chip-user` (event terdisaptasi) | chip `80 × 30`, `right = 821` == tepi kanan `.header-right` (**gap 0**); menu `236 × 176`, `z-index 60`; **hit-test** `elementFromPoint` → `DIV.um-head` `insideMenu=true` | dua `.um-item` `222 × 46` (target sentuh ≥ 40 px) |
| Settings | item pertama | `location.hash = "#appearance"` → `#view-mount` melukis `H2 "Appearance"` + panel tema | navigasi nyata, bukan dead link |
| Peran lemah | tab 2 login `viewer`, lalu tulis via `OPCR.api.post('/api/v1/auth/users', …)` | `POST /api/v1/auth/users -> 403 (275 bytes)`, `ok=false why="insufficient_role"` | chip `class="chip-user deny"` + `title="kernel menolak: insufficient_role"`; token **tetap ada**, path **tetap** `/` — `BOUNCE` sengaja tidak memuat `insufficient_role` |
| Logout | item kedua | `POST /api/v1/auth/logout -> 200` | kembali ke `/login.html` dengan note sekali-pakai (`NKEY` habis baca), `UKEY=null`, token=false, `#chip-user` tidak ada |

Inventaris navigasi **tidak** berubah oleh build ini: `a[data-view]` **26**, `.nav-item` **18**, target `data-view` unik **23** — identik antara `grep shell.html` dan DOM runtime. Konsol peramban: **0 pesan** pada `/login.html` maupun `/` setelah `navigate_page(ignoreCache=true)`.

### 12.2 Kontras chrome sesi: hitungan → render

Pemindai token (`tools/check-contrast-session.mjs`, tidak ter-commit karena berada di `tools/`) menemukan **9 pasangan di bawah 4,5:1** pada minimal satu tema sebelum suntingan — terburuk `.lg-btn` **2,35:1** (senja), yaitu tombol satu-satunya untuk masuk konsol. Setelah substitusi token: **0 pasangan** di bawah ambang untuk ketiga tema, terketat `.um-role`/`.lg-sub` **4,52** (default) dan `.lg-btn` **4,77** (senja). Dua pasangan di antaranya **dirender** di peramban (tema `dark`) dan angkanya cocok persis dengan hitungan: `.lg-btn` **5,21**, `.chip-user.deny` **5,94**, `.um-caret` **7,80**, `.um-ava` **9,04**. Penjaga yang masuk suite: `test_02c` melarang **seluruh** karakter `#` di `session.css` (`lib.rs:1073`) dan `test_05l` menuntut 11 kopling nama kelas `login.js`↔`session.css` + 2 kopling kunci `sessionStorage` `login.js`↔`login.html` — cacat §4.30 (dropdown tak pernah tampil karena satu nama kelas beda) tidak bisa berulang diam-diam.

### 12.3 Batas bukti UI-02J (kejujuran)

- Status **RUNTIME-VERIFIED** hanya untuk yang terukur di atas image build J pada tema **dark**. Tema **default** dan **senja** berhenti di tingkat **UNIT-TESTED/hitungan token**: harness tidak mengganti `localStorage["rinjani-theme-mode"]` saat probe, dan pita senja bergantung jam browser operator.
- Breakpoint **≤ 560 px** hanya terbukti sebagai aturan CSSOM yang terparsai (`.lg-body`, `.lg-card`, `.user-menu`); `matchMedia('(max-width:560px)')` = false pada viewport harness 831 px dan jendela peramban tidak dapat diubah ukurannya → **belum terukur pada layar sungguhan**.
- `click`/`press_key` native peramban **tidak** sampai ke halaman pada harness ini (`visibilityState=hidden`); seluruh interaksi memakai `KeyboardEvent`/`MouseEvent` terdisaptasi ke handler milik berkas produksi. Yang terbukti adalah perilaku handler + geometri DOM, **bukan** rupa yang dilihat mata — konfirmasi visual operator tetap `BLOCKED`.
- `node tools/check-term.js` **NOT RUN** pada build J (tidak ada satu pun berkas terminal yang disunting; bukti CLI terakhir tetap sesi build I). Boot **dari ISO** juga `NOT RUN` — ISO `7 297 024 B` (`183BF504…4C21C0`) adalah artefak distribusi yang dibentuk dari biner yang sama, verifikasi nyata memakai disk image.
- Bootstrap meninggalkan **dua** akun lab di RJCFG01 (`admin` + `viewer2` untuk uji 403); store berkapasitas 4 slot. Tidak ada kredensial di dokumen ini, di aset, maupun di jurnal serial yang di-stage (`grep -c` pola sandi = 0; gema kernel atas baris sandi memang tidak ada — `main.rs:1142`). Kejadian pertama penggunaan `--send-secret` pada sesi ini **bocor** karena listener host masih proses lama; sudah diredaksi dan listener dijalankan ulang dari kode tertambal sebelum uji di atas (lihat laporan UI-02 §4.31).
