# LAPORAN SPRINT AUD-02: AUDIT ULANG SAMPAI WEBUI BENAR-BENAR BERFUNGSI

> **Document ID:** `docs/RND/RND_25_AUD02_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `90a3207` (laporan + lima artefak AUD-01)
> **Mandat:** AUD-02 MASTER EXECUTION PROMPT — mengubah implementasi yang rusak/belum aman/belum tersambung/masih tampilan menjadi nyata, terintegrasi, teruji, terbukti; §4 artefak; §17 kosakata status.
> **Permintaan Owner yang mengikat sprint ini:** (1) aktifkan `gateway.html` + tes TCP untuk `/api/v1/routes`; (2) perbaiki pendeteksian gateway DHCP-client di WAN yang tidak sesuai gateway sebenarnya; (3) IP/netmask/gateway/DNS harus berasal dari sumber nyata tanpa nilai tambal, dan DNS harus tampak di modul DNS Server. Ditambah perintah berdiri: audit ulang semua menu & fungsi, urutan project utama → VM → build disk → push GitHub.
> **Doktrina:** ASL CORE BUKAN LINUX; tidak ada kode vendor disalin; tidak ada kemampuan kernel yang dikarang; tidak ada mock/stub/data karangan sebagai bukti.
> **Redaksi:** IP lab dan MAC tersamar (`10.10.MM.NNN` klien, `10.10.MM.GGG` gerbang/DNS, `08:00:27:XX:XX:XX`); kredensial lab tidak dicatat di dokumen ini (AGENTS.md §4.5).

---

## 1. Ringkasan eksekutif

AUD-01 berhenti pada "laporkan apa adanya". AUD-02 melanjutkan ke "perbaiki sampai jalan". Hasil kerja bukan tambahan fitur baru untuk pamer, melainkan menutup lubang yang ditemukan saat rantai kendali ditelusuri sampai kernel dan sampai VM.

| Kelompok pekerjaan | Isi | Status akhir |
|---|---|---|
| Keamanan lapisan server (DEFER-01) | crate `asl-router-crypto` (SHA-256/HMAC-SHA256/PBKDF2 1024 round, perbandingan waktu-konstan, hex) + `AuthStore`/sesi bearer + gerbang autentikasi & otorisasi peran di dispatcher | `RUNTIME-VERIFIED` (boot fail-closed + 401/403 nyata + User Manager terisi) |
| Framing HTTP (DEFER-02, DEFER-03) | urai `Content-Length` ketat + keharusan sama dengan byte yang tiba + `MAX_BODY_BYTES 1024` → 413 sebelum dispatch | `UNIT-TESTED` |
| Pembatas laju (DEFER-04) | ember per `(peer, kategori)` alih-alih satu ember global; baca/tulis/autentikasi punya kuota sendiri; ember idle dibuang | `RUNTIME-VERIFIED` (0 kejadian 429 pada sapuan 23 view; sebelumnya 37) |
| Modul Gateway + rute (permintaan 1) | primitive FIB/LPM jadi, `GET /api/v1/routes` + `/routes/stats`, `gateway.html` dipasang, tes dispatch TCP penuh | `RUNTIME-VERIFIED` |
| Asal-usul lease DHCP (permintaan 2 & 3) | model `LeaseParamSource` (ACK → OFFER → classful/turunan → absent) untuk netmask, gateway, DNS; tanpa pewarisan gerbang dari siaddr/option-54, tanpa resolver publik cadangan; propagasi ke modul DNS Server, log `[R21]/[R28]`, dan `/ip dns print` | `RUNTIME-VERIFIED` |
| Tabel ARP nyata (blocker §6–§13 AUD-01) | pembelajaran tetangga dari trafik + `GET /api/v1/arp` dan `/arp/stats` | `RUNTIME-VERIFIED` |
| Cacat baru yang lahir dari verifikasi runtime | `AUD02-D01` sampul JSON cacat, `AUD02-D02` rail basi, `AUD02-D03` rute default dilabeli kosong, `AUD02-D04` gerbang byte aset optimistis | `FIXED-RUNTIME-VERIFIED` (D04: `OBSERVED-MITIGATED`) |

Gerbang akhir: `cargo fmt --all -- --check` exit 0; `cargo test --workspace` exit 0 **390 passed / 0 failed / 0 ignored**; `cargo check --target x86_64-unknown-none -p asl-kernel-bin` exit 0. Tidak ada tes yang dihapus, diturunkan kriterianya, atau dinonaktifkan; tidak ada pemeriksaan keamanan yang dilonggarkan.

## 2. Peta temuan: 13 modul `BLOCKED` + 6 `DEFER-*` dari artefak AUD-01

Daftar diambil dari `docs/RND/AUD01_MENU_FUNCTION_MATRIX.csv` (kolom `status_bukti`) dan `AUD01_DEFECT_REGISTER.csv`, **bukan** dari asumsi. Baris "after" adalah keadaan setelah AUD-02 diverifikasi di VM.

| # | Modul | before (AUD-01) | after (AUD-02) | dasar after |
|---|---|---|---|---|
| 8 | WireGuard | BLOCKED | `BLOCKED` | belum ada antarmuka WG di jalur data; halaman tetap jujur "desain saja" |
| 9 | VPN | BLOCKED | `BLOCKED` | sama; tidak dipromosikan ke navigasi aktif |
| 10 | XRay-Core | BLOCKED | `BLOCKED` | belum ada mesin; halaman jujur |
| 11 | QoS Manager | BLOCKED | `BLOCKED` | mesin shaper belum ada (#83) |
| 13 | ARP Table | BLOCKED | `RUNTIME-VERIFIED` | `/api/v1/arp` entries=1 capacity=16; baris terikat di DOM |
| 16 | Gateway | BLOCKED | `RUNTIME-VERIFIED` | `/api/v1/routes` 2 rute nyata + `/routes/stats default_active=true` |
| 17 | DHCP Server | BLOCKED | `BLOCKED` | kernel hanya punya DHCP **client** terbukti DORA; `/api/v1/leases` -> `not_found` |
| 21 | Packages | BLOCKED | `BLOCKED` | tidak ada manajemen paket; halaman jujur |
| 22 | User Manager | BLOCKED | `RUNTIME-VERIFIED` | `/api/v1/auth/users` (sesudah AUD02-D01 diperbaiki) menampilkan `[kosong, admin, admin, 1024]` |
| 25 | Tools (induk) | BLOCKED | `BLOCKED` | alias ke Speed Test yang masih blocked |
| 26 | Speed Test | BLOCKED | `BLOCKED` | tidak ada pengukuran laju dari kernel |
| 27 | Telnet | BLOCKED | `BLOCKED` | layanan telnet sengaja tidak diaktifkan (mandat: tanpa spesifikasi & otorisasi) |
| 28 | Trace Route | BLOCKED | `BLOCKED` | gema ICMP keluar belum ada (#81) |
| — | DNS Server | PARTIAL | `RUNTIME-VERIFIED (parsial jujur)` | resolver/proxy hulu terisi dari lease; `serving=false` karena terima UDP/53 belum dirangkai |
| — | Interfaces | PASS | `RUNTIME-VERIFIED` | kini ikut menampilkan label sumber `option-*-ack` per field |

| ID | DEFER | after | catatan |
|---|---|---|---|
| DEFER-01 | tidak ada autentikasi | `FIXED-RUNTIME-VERIFIED` | HTTP polos, bukan TLS — tercatat sebagai batas |
| DEFER-02 | `Content-Length` tak divalidasi | `FIXED-UNIT-TESTED` | |
| DEFER-03 | tak ada batas body `POST /api/v1/power` | `FIXED-UNIT-TESTED` | |
| DEFER-04 | satu ember rate-limit global | `FIXED-RUNTIME-VERIFIED` | |
| DEFER-05 | tab Ethernet tanpa filter tipe | **masih `OBSERVED`** | tugas #84, belum dikerjakan |
| DEFER-06 | Find Dashboard hanya menyaring tabel | **masih `OBSERVED`** | tugas #84, belum dikerjakan |

Dari 13 modul `BLOCKED`, tiga naik ke `RUNTIME-VERIFIED` (ARP Table, Gateway, User Manager). Sisanya **sepuluh entri tetap blocked dan sengaja tidak diaktifkan** — mempromosikan modul yang belum lulus uji ujung-ke-ujung hanya akan mengganti angka kosong dengan angka bohong.

## 3. Permintaan 1 — Gateway dinyalakan + tes TCP untuk `/api/v1/routes`

- **Primitive kernel:** tabel rute dibangun dari LPM/FIB yang sudah ada (`MAX_ROUTES = 64`), bukan daftar karangan. Rute default hanya dipasang bila gateway lease benar-benar non-nol, sehingga sistem tanpa gateway tidak pernah mengaku punya rute lewat.
- **Endpoint:** `GET /api/v1/routes` (daftar prefix/tipe/gateway/antarmuka/metric) dan `GET /api/v1/routes/stats` (`default_active`, `default_gw`). Keduanya lewat dispatcher webcore, bukan fungsi C.
- **Tes TCP penuh:** bukan panggilan fungsi langsung — permintaan bytearray `GET /api/v1/routes HTTP/1.1 + Authorization: Bearer …` didispatch, respons diurai, dan isi JSON diperiksa; jalur baca ikut masuk penjaga keseimbangan JSON 18 jalur.
- **UI:** `gateway.html` memakai `data-list="/api/v1/routes"` + `data-fmt`, mengikuti chrome WebFig (tab strip + toolbar) seperti modul non-dashboard lain.
- **Bukti VM:** `10.10.MM.0/24 connected` dan `0.0.0.0/0 gw 10.10.MM.GGG static metric 100`, `default_active: true`, dua baris muncul di DOM.

## 4. Permintaan 2 & 3 — Asal-usel lease harus nyata, tanpa tambalan

Masalah inti AUD-01/awal AUD-02: gerbang WAN dilaporkan dari `siaddr` atau option-54 (pengidentifikasi server DHCP), yang **bukan** gerbang default. Nilai itu kebetulan sama di lab, jadi kesalahan tidak tampak.

Model yang dipakai sekarang:

| Field | Sumber yang diterima | Yang dilarang |
|---|---|---|
| Netmask | option-1 pada ACK → option-1 pada OFFER → turunan classful (dilabeli `classful-turunan`) | menebak /24 tanpa dasar |
| Gateway | option-3 pada ACK → option-3 pada OFFER → `Absent` | `siaddr`, option-54 server-id, mewarisi alamat klien |
| DNS | option-6 pada ACK/OFFER → `Absent` | mencadangkan 8.8.8.8 bila lease tidak membawa option-6 |

Setiap nilai membawa label sumber (`option-3-ack`, `option-6-offer`, `classful-turunan`, `none`) yang ikut keluar di API dan tampil di UI, sehingga operator bisa membedakan "dari server" dan "turunan".

Bukti satu rantai, tiga permukaan (boot VM, disk final):

```
[R28] DHCP OFFER dari 10.10.MM.GGG ip=10.10.MM.NNN opsi1=ada opsi3=ada opsi6=ada
[R28] DHCP BOUND eth0=10.10.MM.NNN/24 (netmask sumber: option-1-ack) gw=10.10.MM.GGG (sumber: option-3-ack)
        dns=10.10.MM.GGG (sumber: option-6-ack)+1 8.8.8.8 lease=1800 s
[R21] DNS PROXY upstream=10.10.MM.GGG (sumber: option-6-ack)
[R30] Auth: 0 kredensial … WebUI TERKUNCI: belum ada kredensial.
```

- **API:** `/api/v1/dns` -> `config_mode=dhcp, lease_source=dhcp, resolver_1=10.10.MM.GGG, resolver_src=option-6-ack, from_dhcp=true, proxy_upstream=10.10.MM.GGG, proxy_upstream_ready=true, serving=false`.
- **UI:** modul DNS Server mem-bingding `resolver_1`, `resolver_src`, `proxy_upstream` (AKTIF) dan `serving` yang tetap ditulis **TIDAK AKTIF**; Interfaces menampilkan label sumber per field; Terminal `/ip dns print` mencetak angka yang sama dari kernel.
- **Kejujuran:** `8.8.8.8` pada boot ini bukan tambalan — ia memang butir kedua option-6 dari lease lab. Ketika server tidak mengirim option-6, resolver tetap `none` dan modul tidak mengisi dirinya sendiri.

## 5. Keamanan lapisan server (karena "belum aman" adalah bagian mandat)

1. **Kredensial tidak pernah berada sebagai teks polos.** PBKDF2-HMAC-SHA256 1024 round dengan garam per akun; `AUTH_PASSWORD_MIN 10`, `AUTH_PASSWORD_MAX 64`; perbandingan waktu-konstan; buffer sandi di-`fill(0)` setelah dipakai.
2. **Sesi bearer 64 heks** dengan kedaluwarsa & pencabutan; `auth revoke`, penggantian sandi diri sendiri mencabut sesi sendiri.
3. **Otorisasi peran.** `admin` vs `viewer`; daftar akun, tulis, dan aksi daya adalah aksi admin → `403 insufficient_role`; anonim → `401 unauthenticated` (bukan `404`, supaya jalur tidak bisa dienumerasi).
4. **Anti-penebasan.** Kategori autentikasi punya ember sendiri (4 burst + 1 per 30 tick), sehingga tebak sandi menguras kuota penyerang tanpa mematikan modul lain.
5. **Bocor nol.** `/api/v1/auth/users` hanya mengembalikan nama/role/rounds; tes menuntut `salt`, `hash`, `password`, token, dan sandi tidak muncul di badan respons mana pun.
6. **Boot fail-closed.** Tanpa kredensial, WebUI menolak sesi dan konsol menyatakan sistem terkunci — bukan menyamar terbuka.
7. **Konsol serial tidak menggemakan sandi** (mode `auth_pw_wait`), dan byte sandi tidak pernah diserahkan ke pemindai perintah.
8. **Batas:** HTTP polos tanpa TLS — token bergerak terbaca di LAN lab; ini dicatat, tidak diklaim aman terhadap penyerang di jalur.

## 6. Cacat yang hanya terlihat karena verifikasi runtime (bukan karena membaca kode)

| ID | Cacat | Kenapa lolos dari pembacaan sumber |
|---|---|---|
| AUD02-D01 | `/api/v1/auth/users` mengirim 200 dengan tiga penutup kurung untuk dua pembuka → `JSON.parse` peramban menolak, User Manager kosong padahal footer sudah jujur "http_200" | jalur unit memakai pencocokan substring, jadi kurung tak seimbang tidak pernah terdeteksi; penjaga baru `test_35b` menelusuri 18 jalur baca |
| AUD02-D02 | Rail "modul belum aktif" masih mencantumkan Rute & Gateway serta Tabel ARP yang endpoint-nya sudah hidup | rail ditulis manual pada sprint UI-02E dan tidak punya penjaga yang membandingkannya dengan registry |
| AUD02-D03 | Rute default `0.0.0.0/0` dilabeli "belum terikat" | satu pengubah `ip` dipakai dua semantik berlawanan; pengubah `addr` baru memisahkan makna |
| AUD02-D04 | Assert waktu-compile mengizinkan aset 7 936 B padahal header riil `ui.js` menuntut > 258 B | konstanta anggaran header bersifat tetap; penjaga sejati adalah dispatch nyata `test_02b` |
| AUD02-D05 | User Manager hanya membaca: tidak ada jalur CRUD dari WebUI padahal kernel sudah menerima `action=set/del/revoke` | endpoint tulis dan konsol serial dibuat lebih dulu; tidak ada penjaga yang mengikat toolbar modul ke jalur tulis |
| AUD02-D06 | Persistensi kredensial lintas reboot belum pernah diukur | setiap verifikasi dikerjakan sesudah `build-disk` yang membentuk image 2 GiB baru, sehingga RJCFG01 selalu terhapus sebelum boot berikutnya |

Koreksi D01 diverifikasi anti-vakum: cacat `]}}` dimasukkan kembali dan `test_35b` **FAILED** dengan pesan `"… JSON tidak berimbang: {'ok':true,'data':[{…}]}}"`, lalu dikoreksi dan lulus.

## 7. Verifikasi, sinkronisasi, dan jejak bukti

| Gerbang | Hasil nyata |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace` | exit 0 — 390 passed / 0 failed / 0 ignored (per crate yang disentuh: api 78, webui 36, webcore 10, crypto 8) |
| `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | exit 0, 1 warning `dead_code` warisan |
| Build disk | `scripts/build-disk.ps1 -Profile debug` exit 0; RAW 2 GiB SHA256 `611B501C…647EA57`; VDI 9 437 184 B; MBR 0x55AA + PMBR 0xEE + "EFI PART" LBA1 terverifikasi |
| Sinkron project ↔ VM | aset terkirim identik byte-per-byte (`shell.html` 7 731 B, `js/core/ui.js` 7 711 B); `#nav-offgroup` = 5 |
| Sapuan DOM | 23 view, 0 galat JavaScript, 0 kejadian 429 |
| Konsol serial | tag `aud02`; log disimpan di `Analysis/logs/aud02-console.log` (tidak di-commit, sudah disaniter dari sandi) |

Urutan pekerjaan dijalankan sesuai perintah berdiri: perbaikan di project utama → gerbang hijau → power-off VM → rebuild disk → boot & bukti runtime → dokumentasi → commit/push.

## 8. Item terbuka (tidak dihilangkan, tidak diklaim)

1. **#81 Gema ICMP keluar** — membuka Ping & Trace Route; sampai primitive ada, kedua halaman tetap di rail.
2. **#83 802.1Q VLAN, mesin shaper QoS, tangkap paket** — tiga modul bergantung padanya.
3. **#84 DEFER-05 & DEFER-06** — filter tab Ethernet dan cakupan Find; UI-only dan belum dikerjakan.
4. **Terima UDP/53 belum dirangkai** — karena itu `dns.serving=false` dan `dns_proxy=false` di `/api/v1/system`; proxy terbukti hanya pada hulu.
5. **`/api/v1/leases` -> `not_found`** — butuh server DHCP nyata untuk halaman DHCP Server.
6. **TLS belum ada** — sesi bearer berjalan di HTTP polos.
7. **`SECURITY-REVIEW`, `REGRESSION-REPORT`, `FINAL-AUDIT-REPORT` (mandat §19) belum dibuat.** Laporan ini bukan laporan final: sepuluh entri modul masih `BLOCKED`, dan menyebut "final" akan melanggar mandat §17.
8. **Konfirmasi rupa oleh mata operator `BLOCKED`** — harness tanpa permukaan layar; bukti berupa DOM, `getComputedStyle`, dan byte aset, dan menuntut reload `ignoreCache` karena cache peramban mengabulkan ETag lama.

## 9. Catatan untuk owner (tindakan yang memerlukan keputusan Anda)

- **Kredensial lab.** Disk hasil build sengaja berisi **nol kredensial** (fail-closed). Agar WebUI bisa dibuka untuk pengujian, satu akun admin dibuat lewat bootstrap serial. Sandinya tidak dicatat di dokumen ini dan tidak di-commit; **segera ganti** lewat User Manager atau `auth set <nama> <admin|viewer>` di konsol serial, dan hapus akun test yang tidak dipakai.
- **Direktori `Python/` di root repo** (untracked, berisi runtime CPython 3.15) dan **`Analysis/logs/`, `Analysis/tmp/`** bukan buatan sprint ini; saya tidak menghapusnya. AGENTS.md §4.1 mewajibkan root bersih — mohon putuskan dipindah/di-gitignore/dibuang.
- **`Analysis/reports/is02_rjcfg3.log`** di-track git dan memuat IP intern riil; ia punya perubahan yang **sengaja tidak saya stage**. Perlu diputuskan: sanitize file track-an ini atau keluarkan dari version control.
- Push dan commit dilakukan per kelompok (aset sebelum kode, staging per jalur) setelah gerbang hijau, sesuai persetujuan "Commit + push setelah bukti VM".

## 10. Lanjutan sprint — perintah owner: "usermanager harus bisa merubah dan menambah dengan CRUD"

| Lapisan | Status kosakata mandat | Bunyi bukti |
|---|---|---|
| Modal `js/core/useredit.js` + `views/user-modal.html` (aset 51 & 52) | **RUNTIME-VERIFIED** | `ETag "rinjani-js-useredit-aud02-7782"` dan `Content-Length 7782` terlayani utuh; modal terbuka dari New, dobel-klik baris, Edit, dan Remove |
| Cipta akun (`action=set`) | **RUNTIME-VERIFIED** | baris kernel bertambah `[nama, viewer, 1024]`; status modal mengutip `kredensial: tertulis dan terbaca kembali pada RJCFG01` hasil baca ulang `/api/v1/writes/last`; isian sandi dikosongkan sehabis kirim |
| Ubah akun (`action=set` dengan nama terkunci) | **RUNTIME-VERIFIED** | role baris berpindah `viewer` → `admin`; nama tidak dapat diubah karena ia kunci tabel kredensial |
| Hapus akun (`action=del`) | **RUNTIME-VERIFIED** | baris hilang dari daftar kernel; Remove pada toolbar hanya membuka modal dan penghapusan menuntut tombol "Hapus akun" |
| Penjaga tabel (`last_account`, `last_admin`, `bad_user_or_table_full`, `password_too_short`) | **RUNTIME-VERIFIED** | `action=del` pada akun terakhir dijawab `ditolak kernel: last_account` dan barisnya tetap ada; prose halaman menyebut kode ini |
| Cabut sesi (`action=revoke`) | **RUNTIME-VERIFIED** | `kernel: mencabut sesi (status applied, sesi cabut 1)`, token sesi menjadi 0 panjang dan gerbang login kembali; kalimat `jurnal ?` yang dahulu mengarang janji persistensi untuk aksi sesi sudah dihapus |
| Gerbang peran | **RUNTIME-VERIFIED** | pada sesi viewer: `GET` dan `POST` keduanya `403 insufficient_role`, isian disabled, tombol tulis hilang; penjaga tetap di kernel, bukan di DOM |
| Persistensi kredensial lintas reboot tanpa rebuild disk (D06) | **RUNTIME-VERIFIED** | `arm` → `restart` lewat `/api/v1/power`; boot berikutnya mencetak `[R26] RJCFG01 journal DIBACA DARI DISK: slot 1 seq=6` dan `[R30] Auth: 1 kredensial dimuat dari RJCFG01`, lalu login berhasil |
| `table.js` (chrome toolbar) | **UNIT-TESTED** tidak diubah | 7 790 B mepet anggaran; penerusan aksi ke `OPCR.wf.actions` sudah terbukti di runtime sebelumnya sehingga tidak perlu disentuh |

Rincian langkah dan angka byte ada di `AUD01_TEST_RESULTS.md` §11. Item terbuka §8 tidak berubah: #81, #83, #84, UDP/53, `/api/v1/leases`, TLS, dan laporan §19 mandat tetap pada statusnya masing-masing; sepuluh modul `BLOCKED` belum diaktifkan.

