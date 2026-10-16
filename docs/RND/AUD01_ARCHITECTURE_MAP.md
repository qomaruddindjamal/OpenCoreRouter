# AUD-01 ARCHITECTURE MAP — satu rantai kendali dari klik ke silikon

> **Document ID:** `docs/RND/AUD01_ARCHITECTURE_MAP.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Kegunaan:** peta yang dipakai mandat §5 untuk menelusuri **setiap** kontrol: UI → handler → API → backend Rust → validasi → subsistem jaringan → hasil nyata → kembali ke UI. Baris-baris di bawah adalah `file:line` dari working tree saat audit, bukan diagram aspirasi.

## 1. Lapisan

```
Peramban (shell.html + views/*.html + js/core/*.js)
   │  fetch GET /api/v1/*  ·  POST /api/v1/power | /api/v1/interfaces/<nama>
   ▼
DMA RX eth0 (virtio-net)                      asl-kernel-bin/src/main.rs  [R9]/[R11b]
   ▼
TCP state machine native (bukan smoltcp)      asl-router-webui::TcpWebServer  port 80, 8 sesi
   ▼
Dispatcher API                                asl-router-api::dispatch_get :449-460 / dispatch_post :472
   ▼        (rate limiter :442, gerbang sama-origin, gerbang metode 405 :665)
WebRuntime snapshot                           asl-kernel-bin/src/main.rs `macro_rules! web_runtime` :1126
   ▼        dipanggil pada :1680 (per frame) dan :1757 (per tick)
Fakta kernel                                  srv.stats / tabel antarmuka / konter TCAM / konter NAT / RJCFG01
   ▼
Perakit respons JSON/HTTP                     asl-router-api handle_* :644-969 → asl-router-webcore build_response
   ▼
Baca baris status untuk log                   asl-router-webcore::response_status :284 → webui last_status :684/:709
   ▼
DMA TX eth0 → peramban mengikat nilai         js/core/ui.js binder (data-source/data-list/data-bind/data-fmt)
```

## 2. Kontrak yang harus dihormati setiap perubahan

| Kontrak | Pemegang | Catatan audit |
|---|---|---|
| Gerbang ukuran aset `7 936 B` | `WEB_RESP_MAX 8192 − HTTP_HEADER_BUDGET 256`, `const _: () = assert!(...)` di `asl-router-webui/src/lib.rs:113` | menambah komentar/fitur di JS/HTML dapat gagal kompilasi — bukan salah kode, salah ukuran |
| ETag = `"<label>-<panjang byte>"` | `serve_static` `asl-router-webcore/src/lib.rs:347` | validator berubah otomatis saat isi berubah; conditional GET → `finish_not_modified` :261 |
| Satu `Ok(n)` untuk 200/304/403/429 | `dispatch` jalur API & aset | angka yang dicatat **wajib** dibaca dari baris status (`response_status`), bukan ditulis sebagai literal |
| Aksi daya dua langkah | `handle_power` `asl-router-api/src/lib.rs:536` | tanpa `arm` → `403 power_not_armed`; jendela 20 s; tidak ada perintah arbitrer dari frontend |
| POST hanya dua permukaan | `dispatch_post` :472 | `/power` dan `/interfaces/<nama>`; selain itu `405`/`not_found` |
| WebRuntime = satu-satunya sumber fakta | `macro_rules! web_runtime` :1126 | tidak ada handler yang boleh mengarang angka; field baru harus diisi kernel (`firewall_wired` contoh AUD-01) |

## 3. Pemisahan tanggung jawab frontend

| Berkas | Tanggung jawab | Ukuran (gate 7 936 B) |
|---|---|---|
| `assets/shell.html` | kerangka, sidebar 28 item, dialog daya, gerbang aset | 7 882 B |
| `js/core/app.js` | router hash, muat parsial, footer status, pompana daya | 7 650 B |
| `js/core/ui.js` | binder deklaratif (`data-source/list/bind/fmt`) + formatter | 7 792 B |
| `js/core/nav.js` | induk/anak sidebar + grup layar sentuh | 4 954 B |
| `js/core/table.js` + `tblcols.js` | chrome tabel WebFig, Find/Filter, urutan kolom | 7 790 / 7 850 B |
| `js/core/term.js` + `termcmd.js` | mesin terminal + tabel perintah/sebab penolakan | 7 875 / 6 990 B |
| `js/core/system.js`, `ifedit.js`, `plot.js`, `theme.js`, `views.js`, `api.js`, `webfig.js` | chart, modal edit antarmuka, tema, tabel modul, klien fetch | 7 725 / 7 823 / 3 503 / 2 875 / 2 489 / 3 113 / 6 322 B |

Aturan yang dipegang tes: `innerHTML` tidak dipakai; binder hanya menulis `textContent`; view tanpa sumber data **tidak boleh** memicu klaim sinkron.

## 4. Cara audit rantai dijalankan (reusable)

1. `curl` mentah endpoint → catat JSON apa adanya (tanpa penyaring).
2. Cocokkan field dengan `handle_*` di `asl-router-api` dan dengan sumber faktanya di `web_runtime`.
3. Buka modul di peramban, baca `.wf-readout`, `#foot-status`, dan baris tabel — bandingkan angka DOM dengan JSON mentah.
4. Cari kata kerja klaim (`up`, `protected`, `active`, `enforced`, `sinkron`) dan tanyakan: **siapa di kernel yang bisa membuktikannya?** Kalau tidak ada, itu cacat kejujuran, bukan cacat tampilan.
5. Kalau bukti butuh kondisi yang tidak bisa dibuat VM dev (mis. NIC kedua), nyatakan `FIXED-UNIT-ONLY`, jangan klaim terverifikasi runtime.
