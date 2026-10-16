# LAPORAN SPRINT AUD-01: AUDIT RANTAI KENDALI WEBUI & PERBAIKAN KEJUJURAN STATUS

> **Document ID:** `docs/RND/RND_24_AUD01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Status:** `VERIFIED` untuk 8 dari 9 cacat yang diperbaiki (bukti runtime peramban + konsol serial); `FIXED-UNIT-ONLY` untuk D05 karena VM dev hanya punya satu NIC; `BLOCKED` untuk konfirmasi rupa visual oleh mata operator
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `8d0009f` (sprint UI-01 modular WebUI)
> **Mandat:** MASTER R&D, AUDIT, DIRECT IMPLEMENTATION & CONTINUOUS VERIFICATION — §2 kontrak navigasi, §3 inventaris, §4 artefak penemuan, §5 rantai kendali, §14 aksi daya, §16 lingkaran perbaikan, §17 status
> **Doktrina:** ASL CORE BUKAN LINUX; tidak ada kode vendor disalin; tidak ada API kernel yang dikarang.
> **Redaksi:** IP lab & MAC tersamar (`10.10.MM.NNN`, `08:00:27:XX:XX:XX`) sesuai kebijakan Zero Secrets in Repository.

---

## 1. Ringkasan eksekutif

Sprint ini tidak menambah fitur baru. Isinya menelusuri rantai kendali **UI → handler → API → backend Rust → validasi → subsistem jaringan → hasil nyata → UI** untuk 28 item navigasi, lalu memperbaiki **sembilan cacat** yang terbukti dari penelusuran itu. Tidak ada satu pun perbaikan yang berupa mock, placeholder, atau pemindahan fitur ke navigasi aktif.

| # | Cacat | kelas | Perbaikan | Status |
|---|---|---|---|---|
| D01 | Footer menulis "sinkron" pada 14 modul yang tidak punya satu pun endpoint | kejujuran | `ui.js` menandai jalur nol-sumber, `app.js` menulis "halaman rupa: 0 endpoint kernel" | `VERIFIED` |
| D02 | Uptime Dashboard musnah permanen oleh Find/Filter | fungsi | uptime pindah ke `.wf-info`, readout dibiarkan khusus pesan toolbar | `VERIFIED` |
| D03 | Submenu layar sentuh terbuka lalu tertutup dalam satu ketukan | navigasi | `ev.stopImmediatePropagation()` pada cabang sentuh `nav.js` | `VERIFIED` |
| D04 | Konsol kernel mencatat 304/403/429 sebagai 200 | kejujuran | `response_status()` membaca baris status respons; kedua jalur memakainya | `VERIFIED` |
| D05 | Baris interface LAN hardcoded `up` | kejujuran | status dibaca dari alamat IPv4 yang benar-benar terpasang | `FIXED-UNIT-ONLY` |
| D06 | "WAN terlindungi: YA" padahal classifier tidak pernah ditanya | keamanan/kejujuran | field kernel `firewall_wired` + `rules_enforced` + `datapath` | `VERIFIED` |
| D07 | `/tool ping <ip>` kehilangan sebab penolakan teknis | fungsi CLI | tabel NO dicocokkan per kata dari depan | `VERIFIED` |
| D08 | Kepala sesi terminal membeku pada "menghubungkan…" | fungsi | `greet()` dipanggil ulang saat sesi lama dibangun ulang | `VERIFIED` |
| D09 | Terminal dilabeli "halaman rupa" padahal tiap perintah adalah HTTP nyata | kejujuran | penanda `data-cli` + footer "halaman interaktif: 0 endpoint pasif" | `VERIFIED` |

Delapan dari sembilan cacat ditemukan **karena** rantai ditelusuri sampai kernel, bukan karena membaca tampilan. D04 dan D06 adalah contoh paling tajam: keduanya hanya bisa terlihat dengan bertanya "siapa di kernel yang bisa membuktikan angka ini?".

## 2. Kepatuhan mandat §2 (navigasi tidak didesain ulang)

Tidak ada label yang diganti, tidak ada urutan yang diubah, tidak ada item yang dipindah atau dihapus. Yang diubah di `nav.js` hanya **penghentian propagasi** pada perangkat sentuh. Penjagaan tetap: `.sidebar/.nav-parent/.nav-sub/.nav-item`, `data-view`, `data-ico`, `data-power`, `href` hash, dua grup navigasi.

Tabrakan `data-view` yang dipermasalahkan mandat §2 diverifikasi **bukan** salah routing: `Firewall`/`Filter Rules` memang alias induk→anak pertama (di sinilah halaman Filter Rules berada), `Services`/`Service List` memakai `services` vs `serviceslist` yang berbeda, `System`→`appearance` karena grup System tidak punya halaman sendiri (Reboot/Shutdown adalah dialog), dan `Tools`→`speedtest` karena Speed Test adalah anak pertama Tools. Empat induk memakai `href="#"` sehingga tidak pernah menavigasi lewat tautan.

Bukti sweep 26 view: 0 error JS, hash dan judul view selalu cocok dengan item yang diklik.

## 3. Kepatuhan mandat §3 (inventaris 28 + backlog jujur)

Terverifikasi dari DOM: **26 `data-view` + 2 `data-power` = 28**. Tiga belas modul rupa-saja tetap berada di tempatnya dan menampilkan "Modul belum aktif di build ini" dengan alasan teknis, bukan angka kosong yang menyamar. Tidak ada fitur yang dipromosikan ke navigasi aktif hanya untuk menutup backlog.

## 4. Artefak §4 (disimpan mengikuti konvensi repo, bukan direktori karangan)

`docs/audit/` tidak ada dan tidak diciptakan (AGENTS.md §4.2 mewajibkan dokumen di `docs/CANONICAL|RND|ROADMAP|RELEASE`), jadi lima artefak mandat diletakkan sebagai keluarga `docs/RND/AUD01_*`:

| Artefak mandat | Berkas nyata |
|---|---|
| `PROJECT-DISCOVERY.md` | `docs/RND/AUD01_PROJECT_DISCOVERY.md` |
| `ARCHITECTURE-MAP.md` | `docs/RND/AUD01_ARCHITECTURE_MAP.md` |
| `MENU-FUNCTION-MATRIX.csv` | `docs/RND/AUD01_MENU_FUNCTION_MATRIX.csv` (28 baris × 12 kolom) |
| `DEFECT-REGISTER.csv` | `docs/RND/AUD01_DEFECT_REGISTER.csv` (9 cacat diperbaiki + 6 tertunda + 1 artefak harness) |
| `TEST-RESULTS.md` | `docs/RND/AUD01_TEST_RESULTS.md` |
| `SECURITY-REVIEW` / `REGRESSION-REPORT` / `FINAL-AUDIT-REPORT` (§19) | **belum dibuat** — lihat §7 Item Terbuka; menyebut laporan "final" sementara 13 modul masih `BLOCKED` akan melanggar mandat §17 |

## 5. Detail teknis perbaikan

**D01 — jalur nol-sumber.** `ui.js`: bila `urls.length === 0`, callback `done(true, null, "kosong")` dan tidak ada permintaan yang dikirim. `app.js stamp()` memakai argumen ketiga itu. Alasannya: klaim "sinkron" adalah pernyataan tentang kernel, dan pernyataan itu tidak boleh keluar tanpa permintaan ke kernel.

**D02 — pemisahan simpul.** `table.js applyFilter()` menulis `textContent` ke `.wf-readout`. Selama uptime terikat di simpul yang sama, setiap Find menghapus nilai kernel secara permanen sampai reload. Solusi terkecil: `.wf-info` (terikat) untuk angka, `.wf-readout` (kosong) untuk pesan, plus `.wf-info { text-align:left }` agar keduanya tetap terlihat berbeda.

**D03 — sentuhan.** Pada perangkat `hover:none`, induk `.nav-item` punya dua listener (toggle submenu di `nav.js`, navigasi+`closeSubs()` di `app.js`). Tanpa penghentian, hasil akhirnya submenu selalu tertutup. `stopImmediatePropagation()` membuat ketukan pertama membuka dan ketukan kedua menutup — terverifikasi dengan stub `matchMedia` lewat `initScript`, empat ketukan berurutan menghasilkan `false→true→false→true` dan `location.hash` tidak berubah.

**D04 — status dibaca, bukan ditulis.** `dispatch`/`dispatch_static` mengembalikan `Ok(n)` untuk 200, 304, 403, dan 429 sekaligus. Sebelumnya angka yang dicatat adalah literal. Sekarang `webcore::response_status(buf: &[u8]) -> u16` mem-parse baris `HTTP/1.1 SSS` (dan mengembalikan `0` untuk buffer yang bukan respons), dipakai di kedua jalur `webui`. Histogram sesi ini: `992×200 160×304 37×429 4×400 2×403`. Ini menutup catatan §4.27/§7.22 laporan UI-02 yang sebelumnya dibiarkan "DICATAT, TIDAK diperbaiki".

**D05 — status LAN.** Kernel tidak punya sensor carrier link. Satu-satunya fakta yang sah adalah alamat IPv4 yang terpasang, sehingga `lan_up = rt.lan_ip != [0,0,0,0]`. Tidak diklaim terverifikasi runtime: VM dev satu NIC bridged, cabang LAN tidak pernah tereksekusi.

**D06 — penegakan vs keberadaan.** TCAM berdiri dan lulus self-test (`[R6] TCAM SELFTEST PASS`), tetapi tidak ada satu pun titik di poll loop yang menanyakannya pada frame yang lewat. Karena itu `WebRuntime` mendapat `firewall_wired`, diisi `false` oleh kernel dengan komentar, dan `/api/v1/firewall/rules` kini memisahkan `wan_protected` (butuh `wan_up && firewall_wired`), `rules_enforced`, dan `datapath:"engine-only"`. UI menampilkan "WAN terlindungi **TIDAK**". Sebelum AUD-01 halaman itu menyatakan router terlindungi padahal tidak.

**D07–D09 — terminal.** Pencocokan tabel NO dipindah dari "sama persis dengan seluruh perintah atau kata pertama" menjadi "sama kata-per-kata dari depan", sehingga `/tool ping 10.10.MM.NNN` dan `/tool traceroute 1.1.1.1` mendapat sebab teknisnya. `greet()` dipanggil pada cabang sesi-ada agar kepala jendela membaca `/api/v1/system` lagi. `terminal.html` menandai dirinya `data-cli="1"` dan footer membedakan halaman kerja dari halaman rupa.

## 6. Verifikasi & sinkronisasi

- Gerbang: `cargo fmt --all` exit 0; `cargo test --workspace` exit 0 **303 passed / 0 failed**; `cargo check --target x86_64-unknown-none -p asl-kernel-bin` exit 0.
- `scripts/build-disk.ps1 -Profile debug` dua kali (build pertama untuk D01–D06, build kedua untuk D07–D09); UUID VDI dikembalikan ke `aedff65b-…` sebelum `startvm`.
- Sinkron project ↔ VM: **49/49 aset identik byte-per-byte** (mis. `/js/core/term.js` → `"rinjani-js-term-ui02i-7875"`).
- Aksi daya: hanya gerbang arming yang diuji (`403 power_not_armed` → `arm` → `200 valid_for_s:20`). Restart/shutdown nyata tidak dieksekusi ke guest pada sesi ini, sesuai mandat §14.

## 7. Item Terbuka (tidak dihilangkan, tidak diklaim)

**Kelas blocker keamanan/fungsi** — butuh keputusan owner sebelum dikerjakan:
1. Tidak ada model autentikasi pada endpoint mana pun; jalur tulis terjangkau dari jaringan bridged.
2. `Content-Length` tidak divalidasi terhadap byte yang benar-benar tiba; tidak ada batas ukuran body pada `POST /api/v1/power`.
3. Satu ember rate-limit global untuk seluruh sistem — pemindaian cepat 26 view menghasilkan 37 kejadian `429`.
4. Tab Ethernet pada Interfaces memakai daftar antarmuka tanpa filter tipe.
5. Find di Dashboard hanya menyaring tabel data, kartu ringkasan tidak ikut (UX, bukan data palsu).

**Kelas butuh mesin kernel** (mandat §6–§13; tidak bisa ditutup dengan mengubah UI):
tabel ARP tetangga + `GET /api/v1/arp` · penerusan IPv4 di FIB + endpoint rute · server DHCP nyata + lease · pemancar gema ICMP keluar (membuka Ping & Trace Route) · 802.1Q/VLAN dan bukti trafik · QoS shaper yang benar-benar mengubah throughput · WireGuard/XRay/packet capture · autentikasi/otorisasi untuk User Manager.

**Belum dibuat:** `SECURITY-REVIEW`, `REGRESSION-REPORT`, `FINAL-AUDIT-REPORT` (§19). Alasan: 13 modul masih `BLOCKED` dan laporan "final" akan melanggar mandat §17.

## 8. Jejak bukti

Konsol serial sesi ini: `Analysis/logs/aud01-console.log` (tidak di-commit). Image disk: `target/rinjani-1.0.0-x86_64-disk.raw` SHA256 `87CF9CE2…154B3766` lalu `C4737817…620F1F5F`. ELF: `target/x86_64-unknown-none/debug/asl-kernel` 3 139 344 B. Semua tangkapan peramban berupa DOM/`getComputedStyle`/`evaluate_script`, bukan citra layar.
