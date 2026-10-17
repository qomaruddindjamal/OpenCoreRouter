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
- Konfirmasi rupa oleh mata operator tetap `BLOCKED` (harness tanpa permukaan layar); bukti berupa DOM + computed style + byte aset, dan memerlukan reload `ignoreCache` untuk melihat aset baru karena cache peramban mengabulkan ETag lama.
