# LAPORAN SPRINT BENCH-01: UJI KECEPATAN, BENCMARK, DAN JAWABAN ATAS PERTANYAAN "RAM 4 MB vs BATAS BANDWITH MAKSIMAL"

> **Document ID:** `docs/RND/RND_28_BENCH01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `3a4a1cc` (penutup sprint MEM-01 Tahap B2) — diverifikasi `git status` bersih sebelum pengukuran, kecuali `Analysis/reports/is02_rjcfg3.log` (milik owner, tidak disentuh)
> **Mandat owner yang dijawab:** "uji test kecepatan dan benchmark, dengan aturan yang sekarang apakah ram 4 MB mampu melewati batas bandwidth maksimal"
> **Doktrina:** ASL CORE BUKAN LINUX; tidak ada kemampuan kernel yang dikarang; tidak ada mock/stub/simulasi sebagai bukti; setiap angka berasal dari eksekusi nyata pada guest bare-metal.
> **Redaksi:** alamat lease WAN router `10.10.MM.RRR`, host penguji `10.10.MM.HHH`, gerbang/DNS lab `10.10.MM.GGG`; MAC host `98:EE:XX:XX:XX:XX`. Sandi akun TIDAK ada di dokumen ini (AGENTS.md §4.5).

---

## 1. Jawaban langsung (ringkasan eksekutif)

Pertanyaan owner punya dua kemungkinan acuan "batas bandwidth maksimal", dan keduanya saya ukur terpisah.

| Acuan "batas bandwidth maksimal" | Apakah 4 MB melewatinya? | Status taksonomi |
|---|---|---|
| **(A) Batas yang sama dicapai ketika RAM lapang (64 MB)** — yaitu pengujian apakah pemotongan RAM menurunkan kinerja | **YA, dan tanpa penalti terukur.** Rata-rata tertimbang throughput sustained 4 MB = **718,2 KiB/s** vs 64 MB = **701,1 KiB/s** (4 MB justru 2,4 % lebih tinggi, jauh di dalam kebisingan antarjalur). Latensi, pola 429, dan biaya jurnal per permintaan identik. | `VERIFIED` |
| **(B) Angka kanonik 842,6 Mbps (transit L3) / 326,4 Mbps (terminasi TCP lokal)** yang tercatat di `docs/CANONICAL/ADR-011_TCP_STRATEGY.md` §1–2 dan `docs/RND/RND_12_FWD01_REPORT.md` §4 | **TIDAK.** Jalur data yang hari ini benar-benar ada dan bisa disentuh dari luar (kontrol-plane HTTP WebUI) berkisar **5,0–10,2 Mbit/s** dengan puncak **815,4 KiB/s ≈ 6,7 Mbit/s**, yaitu ~1 % dari 842,6 Mbps. RAM **bukan** penyebabnya — pada 64 MB hasilnya sama — penyebabnya adalah plafon arsitektur jalur (lihat §6 dan §8). | `VERIFIED` (pengukuran) · klaim 842/326 sendiri `UNKNOWN` (tidak dapat direplikasi, lihat §9) |

Satu kalimat: **aturan 4 MB tidak menjadi penghambat bandwidth**, tetapi "batas bandwidth maksimal" produk hari ini memang rendah (orde megabit, bukan gigabit) dan batas itu ditentukan oleh tabel sesi 8 slot, plafon respons 8 KiB, waktu-layanan per permintaan ~9,5 ms pada satu poll loop, serta ember rate-limit — bukan oleh RAM.

| Kelompok pekerjaan | Isi | Status akhir |
|---|---|---|
| Suites runtime | 4 suite benchmark (per-connection, sustained keep-alive, isolasi byte-vs-permintaan, ICMP RTT idle & under load) dijalankan **dua kali** pada konfigurasi RAM berbeda dengan skrip identik | `RUNTIME-VERIFIED` |
| Kontrol RAM | Guest 4 MiB (`[R3]` usable 708 KiB dan 712 KiB pada dua boot) vs guest 64 MiB (`[R3]` usable 62 148 KiB dan 62 152 KiB) | `RUNTIME-VERIFIED` |
| Fungsi autentikasi di bawah beban pada 4 MiB | login `200` → `/api/v1/system` `200` → logout `200` → sistem `401` | `RUNTIME-VERIFIED` |
| Koreksi komentar rate limiter | "(200/menit per klien)" adalah salah label; tick = 100 ms sehingga 20 token/tick = 200 **per detik**. Komentar diperbaiki dan kini menyebut plafon terukur | `IMPLEMENTED` (komentar saja, nol perubahan perilaku) |
| Plafon transit L3 (forwarding) dengan beban iperf | **Tidak diukur** — lab hanya punya satu host; tidak ada harness transit | `NOT RUN` |
| Shaper/policer bandwidth di kernel | Belum ada (task #83 terbuka), jadi tidak ada "batas bandwidth" yang bisa dikonfigurasi di produk | `BLOCKED` (fitur belum diimplementasi) |

---

## 2. Lingkungan dan metodologi

**Subjek uji.** VM dev `"Open Core Router"` di VirtualBox 7.2 headless, disk VDI hasil sprint MEM-01 (profil release, ELF 707 168 B), NIC1 virtio-net pada jaringan lab (DHCP, leases `10.10.MM.RRR`), konsol serial UART1 `tcpserver 4555`.RAM diubah **hanya** lewat `VBoxManage modifyvm --memory`, boot/matinya selalu lewat jalur nyata: `shutdown` dari konsol serial (ACPI PM1_CNT, `[R25]`) lalu `startvm --type headless`. Tidak ada snapshot, tidak ada restore state.

**Penuntut (client).** Node.js 24 di host yang sama, memakai modul `http` mentah. Dua mode koneksi diuji terpisah karena keduanya menjawab pertanyaan berbeda:
- *koneksi-baru-per-permintaan* — memuat biaya tiga-salam TCP ke dalam angka; mewakili browser sungguhan yang membuka koneksi untuk tiap aset.
- *sustained keep-alive* — satu socket dipakai berulang selama 10 detik per fase; mengisolasi biaya pemrosesan server dari biaya setup.

**Definisi satuan.** `thruput` = total byte body yang diterima ÷ **dinding jam fase** (bukan penjumlahan latensi per permintaan) dalam KiB/s; `Mbit/s` = KiB/s × 1024 × 8 ÷ 10⁶; `req/s` = jumlah permintaan ÷ dinding jam; persentil dihitung dari latensi per permintaan (mulai request → byte terakhir). Semua angka di laporan ini adalah hasil cetak langsung skrip, tidak ada yang dihitung ulang manual.

**Beban latar yang jujur.** Sepanjang seluruh pengukuran, satu tab peramban di host `10.10.MM.HHH` menanyakan `/api/v1/services`, `/api/v1/interfaces`, `/api/v1/system`, `/api/v1/auth/status` setiap ±5 detik (terlihat jelas pada jurnal sebagai `[R28] HTTP /api/v1/auth/status -> 200`). Beban ini **sama pada kedua konfigurasi RAM**, sehingga tidak membatalkan perbandingan, tetapi menaikkan absolute angka latensi ekor.

**Batasan metodologi (WAJIB dibaca sebelum mengutip angka).** Host penuntut dan guest berjalan pada mesin fisik yang sama, sehingga trafik melewati virtual switch VirtualBox, bukan kawat nyata. Angka dengan demikian adalah **plafon jalur perangkat lunak guest**, bukan klaim kinerja perangkat keras atau klaim "carrier-grade". Guest 4 MiB menerima ±712 KiB RAM; tidak ada heap (`no GlobalAlloc`), jadi apa yang diukur di sini adalah perilaku buffer statis.

---

## 3. Suite 1 — biaya per permintaan dengan koneksi baru

Skrip yang sama, dijalankan pada dua konfigurasi RAM, tanpa perubahan lain.

| Fase | Isi | 4 MiB | 64 MiB |
|---|---|---|---|
| A | berurutan 50× `/js/core/term.js` (7 877 B) | 613,3 KiB/s (5,02 Mbit/s), 79,7 req/s, p50 10,1 ms, max 89,3 | 616,0 KiB/s (5,05), 80,1 req/s, p50 9,2, max 75,4 |
| B | berurutan 150× aset sama | 895,1 KiB/s (7,33), 116,4 req/s, p50 8,5 | 646,2 KiB/s (5,29), 84,0 req/s, p50 9,2 |
| C | berurutan 300× aset sama (banjir) | 782,8 KiB/s (6,41), 101,8 req/s, p50 9,0, **0 drop, 0 × 429** | 775,6 KiB/s (6,35), 100,8 req/s, p50 8,8, **0 drop, 0 × 429** |
| D1 | 8 koneksi paralel | 1 179,3 KiB/s (9,66), 153,3 req/s, p50 42,3, max 51,1 | 1 249,1 KiB/s (10,23), 162,4 req/s, p50 39,7, max 48,2 |
| D2 | 10 koneksi paralel `/login.html` | 53,0 KiB/s, p50 60,1, **p95/max 1 018 ms**, tetap `{"200":10}` | 53,3 KiB/s, p50 66,9, **p95/max 1 016 ms**, `{"200":10}` |
| E | 6× `POST /api/v1/auth/login` sandi salah | `{"401":2,"429":4}`, p50 12,2 ms | `{"401":2,"429":4}`, p50 11,1 ms |
| F | berurutan 50× `/login.html` (5 560 B) | 685,9 KiB/s (5,62), 126,3 req/s, p50 7,9 | 665,5 KiB/s (5,45), 122,6 req/s |
| G | berurutan 50× `/` shell (7 765 B) | 957,3 KiB/s (7,84), 126,3 req/s, p50 7,8 | 972,3 KiB/s (7,96), 128,2 req/s |

Pola: pada 8 fase, 4 MB lebih cepat di 5 fase dan lebih lambat di 3 fase, dengan selisih mayoritas < 10 %. Tidak ada satu pun fase di mana 4 MB gagal, panic, memutus koneksi, atau kehilangan respons; seluruh status `200`/`401`/`429` cocok persis antara kedua konfigurasi.

**Ekor D2 adalah temuan, bukan kebisingan.** Dua koneksi ke-9 dan ke-10 menunggu ±1 detik sebelum dilayani. Penyebabnya ditemukan di jurnal (§7), bukan di RAM.

---

## 4. Suite 2 — sustained keep-alive 10 detik per tingkat konkurensi

| Konkurensi | 4 MiB throughput | 4 MiB req/s | 64 MiB throughput | 64 MiB req/s | Ekor latensi (max) |
|---|---|---|---|---|---|
| 1 | 807,4 KiB/s (6,61 Mbit/s) | 105,0 | 615,8 KiB/s (5,04) | 80,1 | 84,6 ms vs 67,6 ms |
| 2 | 721,2 (5,91) | 93,8 | 640,1 (5,24) | 83,2 | 101,1 vs 229,8 |
| 4 | 657,4 (5,39) | 85,5 | 1 001,2 (8,20) | 130,2 | 183,0 vs 246,5 |
| 8 | 747,4 (6,12) | 97,2 | 709,3 (5,81) | 92,2 | 3 065,4 vs 1 135,1 |
| 12 (melebihi kapasitas) | 657,8 (5,39) | 85,5 | 539,2 (4,42) | 70,1 | **7 084,4 vs 15 020,7** |

Dua kesimpulan penting:

1. **Throughput agregat FLAT terhadap konkurensi** (657–815 KiB/s pada 4 MB; 539–1 001 KiB/s pada 64 MB) sementara latensi naik kira-kira linear dengan jumlah klien. Itu tanda klasik **satu pelayan bersama**: permintaan tidak pernah dilayani paralel, hanya mengantre. Menambah RAM tidak menambah pelayan.
2. **Melebihi 8 klien bersamaan membuat ekor memburuk drastis** (0,1 → 3 s pada 4 MB, 0,25 → 15 s pada 64 MB). Perilaku identik secara kualitatif di kedua RAM → sekali lagi bukan RAM.

Rata-rata kelima tingkat: **4 MiB = 718,2 KiB/s**, **64 MiB = 701,1 KiB/s**.

---

## 5. Suite 3 — pemisahan "biaya per permintaan" dari "biaya byte" (4 MiB, keep-alive, satu koneksi)

| Fase | Target | Ukuran respons | req/s | throughput |
|---|---|---|---|---|
| J1 | `/api/v1/auth/status` (anonim) | ~108 B | 98,1 | 9,8 KiB/s |
| J2 | `/login.html` | 5 560 B | 101,1 | 548,9 KiB/s |
| J3 | `/js/core/term.js` | 7 877 B | 106,0 | 815,4 KiB/s |
| J4 | `/tidak-ada` (404) | ~32 B | 87,9 | 2,7 KiB/s |

Ini pengukuran paling menentukan jawabannya. **Laju permintaan hampir tidak berubah (88–106 req/s) ketika ukuran body berubah 73 kali lipat** (108 B → 7 877 B), dengan p50 8,5–9,1 ms di keempatnya. Artinya:

- Yang habis adalah **anggaran waktu per permintaan (~9,5 ms)**, bukan kapasitas byte.
- Maka **batas bandwidth maksimal jalur ini = plafon req/s × plafon ukuran respons** = 105 × `WEB_RESP_MAX` (8 192 B) ≈ **840 KiB/s ≈ 6,9 Mbit/s**.
- Angka puncak terukur J3 = 815,4 KiB/s = **97 % dari atap teoritis itu.** Jadi WebUI memang sudah menabrak plafon rancangannya, dan plafon itu bukan RAM.

---

## 6. Batas struktural yang memotong bandwidth (dibaca dari kode, lalu dicocokkan dengan pengukuran)

| Konstanta | Nilai | Efek pada bandwidth | Bukti |
|---|---|---|---|
| `WEB_SESSIONS` | 8 | Klien ke-9 dan seterusnya **ditolak SYNC-nya**, bukan dilayani lebih lambat | `VERIFIED` — jurnal §7 |
| `WEB_RESP_MAX` | 8 192 B | Tidak ada satu pun respons bisa lebih besar dari 8 KiB, sehingga throughput = req/s × 8 KiB | `VERIFIED` (§5) |
| `TCP_MAX_PAYLOAD` × `flush_data` guard `< 8` | ≤ 11 680 B per sesi per pump | Cukup untuk respons 8 KiB, tetapi tidak ada mekanisme multi-sesi paralel per iterasi loop | `DOCUMENTED` (kode) |
| Drain RX `while serviced < 8` per NIC per iterasi | ≤ 8 bingkai/iterasi | Rate loop menentukan req/s | `INFERRED` |
| `weblog = \|s\| SERIAL.write_str(s)` sinkron | ±256 B jurnal per sesi | Biaya serial ikut memakan waktu loop | `VERIFIED` (§7) |
| Rate limiter READ `(60, 20, 1)` per tick 100 ms | 200 req/s per klien | **Tidak pernah tercapai** (maks 162 req/s) → bukan pembatas | `VERIFIED` (0 × 429 di semua fase baca) |
| Rate limiter AUTH `(4, 1, 30)` | 4 burst + 1 per 3 s | Bekerja persis sesuai rancangan: `401,401,429×4` | `VERIFIED` |
| Shaper/policer QoS | tidak ada | Tidak ada batas bandwidth yang bisa dikonfigurasi pada produk | `BLOCKED` (fitur #83) |

**Kenapa RAM struktural tidak masuk jalur throughput** (`INFERRED` dari kode, `VERIFIED` lewat kontrol 64 MB): kernel tidak punya heap sama sekali (tanpa `GlobalAlloc`), arena bump `DmaArena` hanya dipakai saat boot; buffer Web per sesi bersifat statis 2 048 B (`WEB_REQ_MAX`) + 8 192 B (`WEB_RESP_MAX`) = 10 240 B × 8 sesi = **81 920 B total, tidak bertambah oleh trafik**; pool TX virtio adalah **8 chain tetap** (`NBUF=8`, `PKTSZ=1536`) yang didaur ulang oleh `reap_tx()`, sehingga byte yang "sedang di udara" dibatasi oleh ring, bukan oleh lamanya transfer. Karena itu 64 MB tidak membeli apa pun untuk jalur data — dan itulah yang ditunjukkan angka di §3–§5.

---

## 7. Bukti mentah dari jurnal guest

Potongan persis dari `Analysis/logs/mem01-console.log` (untracked; IP/MAC telah disamar di dokumen ini, bukan di berkasnya):

```text
[R3] CORE INIT (RAM usable 712 KiB dari Limine memmap)      <- boot 4 MiB
[R3] CORE INIT (RAM usable 62152 KiB dari Limine memmap)    <- boot 64 MiB
[R28] TCP SYN dari 10.10.MM.HHH:54100 pada alamat lokal 10.10.MM.RRR#1 -> SYN/ACK (SYN-RECEIVED)
[R28] TCP handshake selesai (ESTABLISHED)
[R28] HTTP /js/core/term.js -> 200 (7877 bytes dibangun)
[R28] TCP sesi ditutup peer — slot dibebaskan
[R28] TCP SYN ditolak: tabel sesi penuh                      <- inilah penyebab ekor D2/conc=12
```

Kuantifikasi jurnal (dihitung dari selisih offset berkas log per jendela boot, bukan estimasi):

| Metrik | Boot 4 MiB | Boot 64 MiB |
|---|---|---|
| Sesi TCP terlayani | 5 571 | 4 928 |
| `HTTP -> 200` | 5 565 | 4 928 |
| `HTTP -> 401` / `-> 429` | 2 / 4 | 0 / 0 (fase E dijalankan sebelum jendela ini) |
| `TCP SYN ditolak: tabel sesi penuh` | **45** | **43** |
| Total byte jurnal pada jendela | 1 424 987 | 1 259 870 |
| **Biaya jurnal per sesi** | **255,8 B** | **255,7 B** |

Kesesuaian 255,8 vs 255,7 B/sesi dan 45 vs 43 penolakan SYN adalah bukti langsung bahwa **mekanisme jalur data identik pada kedua RAM**; tidak ada perilaku "hemat memori" yang muncul di 4 MB. Pada 105 req/s, jurnal serial menelan ±26,2 KiB/s keluaran UART — kandidat pemangkas waktu loop (`INFERRED`, belum diuji).

---

## 8. Latensi dan kehilangan paket pada 4 MB (ICMP nyata, terminasi oleh kernel)

CLI serial `ping` **dengan sengaja menolak mencetak angka** (`% ping: TIDAK DIIMPLEMENTASIKAN — tidak ada egress ICMP echo maupun reply-matcher di kernel, jadi setiap angka RTT akan palsu`), jadi RTT diambil dari host penuntut ke alamat router, yang dijawab jalur `WAN ARP/ICMP REPLY TX` nyata (bukan simulasi).

| Kondisi | Hasil |
|---|---|
| Idle | `ping -n 30` → **Sent 30 / Received 30 / Lost 0 (0 %)**, min 0 ms, max 0 ms, avg 0 ms (Windows menampilkan `time<1ms`) |
| Jalur HTTP **jenuh** (banjir keep-alive 2 797 permintaan / 22 031 969 B selama ±25 s, ~112 req/s) | `ping -n 40` → **Sent 40 / Received 40 / Lost 0 (0 %)**, min 0 ms, **avg 1 ms, max 21 ms** |

Artinya, bahkan ketika jalur kontrol-plane jenuh, guest 4 MiB tetap menjawab ICMP **tanpa satu pun paket hilang**, dengan jitter yang tetap di bawah 25 ms. Untuk perangkat manajemen jaringan, ini sifat yang layak: kontrol-plane tidak membuat management-plane mati. (Catatan: 0 % loss ≠ shaper; QoS tetap `BLOCKED`/belum ada.)

Autentikasi juga diverifikasi ulang pada kondisi 4 MB ini, dalam satu rangkaian: `POST /api/v1/auth/login` → `200` (24 ms, token 64 hex) → `GET /api/v1/system` dengan `Authorization: Bearer …` → `200` (211 B) → `POST /api/v1/auth/logout` → `200` (14 ms) → `GET /api/v1/system` token sama → `401 invalid_token`. Percobaan pertama saya yang mengirim logout **tanpa** header `Authorization` menjawab `401` dan sesi tidak tercabut — itu perilaku gerbang yang benar, bukan cacat (handler membaca `req.authorization`).

---

## 9. Klaim kanonik 842,6 Mbps vs pengukuran hari ini (kewajiban kejujuran)

`docs/CANONICAL/ADR-011_TCP_STRATEGY.md` §1 menyatakan "telah membuktikan forwarding IP carrier-grade (842 Mbps TCP / 915 Mbps UDP)" dan §2 K5 mengklaim "throughput socket lokal terminasi mencapai 326,4 Mbps"; sumbernya dirujuk ke `docs/RND/RND_12_FWD01_REPORT.md` §4 (gate OPCR-52, `PASS-LOCKED`) yang menunjuk berkas `Analysis/reports/benchmark_forwarding.txt`.

Hasil penelusuran saya, dilaporkan apa adanya:

1. Berkas rujukan itu **untracked** (di luar repo) dan berisi **ringkasan**, bukan output mentah `iperf3`: tidak ada baris per-interval, tidak ada alamat penyaji/pengambil, tidak ada counter antarmuka yang mendampingi.
2. Bagian §4 berkas tersebut berjudul **"SIMULASI BEBAN & CONNTRACK CAPACITY"** dan berisi 10 aliran/0 % drop — bukan angka throughput terukur.
3. Build kernel yang kini berjalan **tidak menyediakan jalur pengukuran transit** untuk direplikasi: egress ICMP echo belum ada (task #81), WebUI hanya melayani terminasi TCP sendiri, dan blok self-test tersimulasi sudah dicabut pada `[R19]` ("TIDAK ADA self-test tersimulasi ... satu-satunya bukti TCP/HTTP/ARP yang sah adalah baris `[R28]`").
4. Lab saat ini hanya memiliki **satu host**; pengukuran transit L3 memerlukan dua sisi (LAN + WAN) yang keduanya mengirim trafik.

Konsekuensi yang saya tetapkan, bukan yang saya karang: angka 842,6 / 915,2 / 326,4 Mbps berstatus **`UNKNOWN` — tidak dapat direplikasi dengan bukti yang tersedia di tree**, dan **tidak boleh** dipakai sebagai pembanding "lulus/tidak lulus" untuk pertanyaan owner. Karena ADR-011 berstatus RATIFIED milik owner, saya **tidak mengubah dokumen kanonik itu sendiri**; yang saya lakukan hanya mencatat temuan ini (§9) dan menaikkan keputusan ke owner (§12). Korespondensinya: jika pengukuran lama ternyata dilakukan di atas harness host-side (bukan guest bare-metal), maka judul gerbang OPCR-52 perlu dilabeli ulang — itu keputusan owner.

Yang **bisa** saya klaim terukur hari ini, dengan label `VERIFIED`: plafon kontrol-plane WebUI 6,7 Mbit/s (puncak 10,2 Mbit/s pada burst 8 koneksi), tidak bergantung pada RAM.

---

## 10. Rekomendasi teknis (urutan dampak per byte RAM)

| # | Tindakan | Gain terduga | Biaya RAM | Status |
|---|---|---|---|---|
| 1 | Naikkan `WEB_SESSIONS` 8 → 16 (hilangkan penolakan SYN; ekor 1–15 s hilang) | Konkurensi 2×, throughput agregat ±2× **bila** loop ikut diperluas | **+81 920 B** (11,5 % dari usable 712 KiB) → **wajib** dibuktikan dengan boot nyata, bukan dihitung di atas kertas | `OWNER-DECISION` |
| 2 | Kurangi biaya jurnal serial per sesi (256 B → ringkas/berbanding) untuk menaikkan plafon req/s | Plafon req/s naik langsung, karena §5 membuktikan req/s-lah batasnya | 0 B (runtime behavior) | `NOT RUN` (belum diuji) |
| 3 | Naikkan `WEB_RESP_MAX` 8 192 B | Tidak disarankan: gerbang kompilasi `assert!(largest_asset_len() + HTTP_HEADER_BUDGET <= WEB_RESP_MAX)` mengikatnya ke aset terbesar, dan §5 menunjukkan byte bukan batasnya | +2 048 B per sesi per 2 KiB | `INFERRED` (nilai rendah) |
| 4 | Implementasi shaper/policer agar "batas bandwidth maksimal" menjadi nilai yang bisa dikonfigurasi dan dibuktikan | Bukan gain mentah; menjadikan klaim carrier-grade dapat diuji | kecil | `BLOCKED` (fitur #83) |
| 5 | Bench transit L3 dua-host dengan iperf3 nyata terhadap guest bare-metal | Mengganti status `UNKNOWN` §9 dengan `VERIFIED` | 0 B | `NOT RUN` (butuh host kedua) |

Saya **sengaja tidak** melakukan perubahan kapasitas apa pun pada sprint ini. Menambah `WEB_SESSIONS` tanpa membuktikan boot 4 MiB justru akan menghasilkan angka tapak memori yang tidak bertanggung jawab — persis kebiasaan yang dihapus oleh MEM-01.

---

## 11. Gerbang verifikasi sprint ini

- `cargo fmt --all` → exit 0.
- `cargo test --workspace` → **400 passed / 0 failed** (66 baris `test result:`, semua `ok`). Peringatan yang muncul adalah warisan (`unused import` di `asl-router-tcpstub`, `dead_code` virtio/netstack) dan **bukan** akibat perubahan ini.
- `cargo check --target x86_64-unknown-none -p asl-kernel-bin` → selesai, 1 peringatan `dead_code` warisan.
- Satu-satunya berkas Rust yang berubah adalah **komentar** di `router/asl-router-api/src/lib.rs` (blok `policy()`), tanpa perubahan perilaku; karena itu disk lab **tidak** di-rebuild dan hasil pengukuran di atas memang diambil dari build MEM-01 yang sama, byte-identik.
- Harness benchmark dipindah ke `tools/bench-01-per-request.mjs` dan `tools/bench-01-sustained.mjs` sesuai aturan AGENTS.md §4.3 (`tools/` ter-ignore git, jadi keduanya **tidak** ada di repo dan tidak akan muncul saat `git clone`; angka di laporan ini dapat diulang dengan `node tools/bench-01-sustained.mjs http://<alamat-WAN-router>`). Tidak ada berkas scratch yang tertinggal di root repo maupun di `target/`.
- VM ditinggalkan dalam keadaan **jalan pada 4 MB** (keadaan produksi-sesuai-mandat), kredensial tidak diubah, `Analysis/logs/` tidak di-stage.

---

## 12. Keputusan owner yang diminta

1. **Apakah batas WebUI hari ini (~7 Mbit/s, 8 klien) dianggap cukup untuk Rinjani 1.0.0**, atau sprint BENCH-02 dibuka untuk menaikkan `WEB_SESSIONS` + memangkas biaya jurnal (§10 butir 1–2)? Kuncinya: butir 1 makan ±80 KiB dari anggaran 712 KiB dan harus dibuktikan boot, bukan dihitung.
2. **Nasib angka 842,6 / 915,2 / 326,4 Mbps di ADR-011 dan RND_12** (§9): label ulang sebagai "host-side/simulasi", cabut dari CANONICAL, atau sahkan sprint pengukuran transit dua-host agar menjadi `VERIFIED`. Saya tidak mengubah dokumen RATIFIED tanpa otorisasi.
3. Konfirmasi bahwa "batas bandwidth maksimal" yang owner maksud memang plafon **kontrol-plane** (WebUI/API), sebab jalur **forwarding/transit** L3 sampai hari ini tidak punya bukti terukur apa pun di tree ini (`NOT RUN`).
