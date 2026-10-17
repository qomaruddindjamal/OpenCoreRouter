# LAPORAN SPRINT IFACE-01 TAHAP A: WEBUI MELAYANI PADA SETIAP ALAMAT LOKAL YANG NYATA

> **Document ID:** `docs/RND/RND_26_IFACE01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `01ba8d4` (registry 54 aset + penjaga dispatch login/session.css [ui-02j])
> **Mandat:** AUD-02 MASTER EXECUTION PROMPT — mengubah implementasi yang rusak/belum tersambung/masih tampilan menjadi nyata, terintegrasi, teruji, terbukti; §17 kosakata status.
> **Permintaan Owner yang mengikat sprint ini:** aturan login harus mengikuti keadaan antarmuka — "jika interface cuma 1 berarti kita login menggunakan ip yang terdapat pada interface tersebut … tetapi jika terdapat beberapa interface contoh 2-5 saat saya ingin login baik via WAN dan interface dengan ip range tersebut apakah juga bisa?" dan penegasannya "aturan seperti itu harus dibuat … jika belum ada harusnya diperbaiki agar bisa". Owner memilih cakupan **Tahap A** (himpunan alamat lokal + eth1 melayani; registrasi NIC 3–5 ditunda) dan mengizinkan **NIC2 host-only diaktifkan** di VM lab sebagai bukti runtime.
> **Doktrina:** ASL CORE BUKAN LINUX; tidak ada kode vendor disalin; tidak ada kemampuan kernel yang dikarang; tidak ada mock/stub/data karangan sebagai bukti.
> **Redaksi:** IP dan MAC lab tersamar. `10.10.MM.RRR` = alamat WAN hasil lease di VM, `10.10.MM.GGG` = gerbang/DNS dari lease, `10.10.MM.CCC` = klien di jaringan bridged yang sama, `192.168.56.100` / `192.168.56.1` = templat host-only VirtualBox yang sudah ada di kode, MAC `08:00:27:XX:XX:XX`. Kredensial lab tidak dicatat di dokumen ini (AGENTS.md §4.5).

---

## 1. Ringkasan eksekutif

Sebelum sprint ini, WebUI hanya mengakui **satu** alamat: `rt.wan_ip`. Gerbangnya ada di dua tempat — lapisan TCP (`on_frame` menolak frame yang `ip.dst_ip != rt.wan_ip`) dan lapisan API (`same_origin` membandingkan header `Host` hanya dengan alamat WAN). Akibatnya, meskipun eth1 secara fisik ada dan beralamat, port 80 di alamat itu mati total, dan kalaupun hidup, permintaan tulis akan ditolak 403.

Yang dikerjakan sekarang bukan menambah tampilan, melainkan memindahkan aturan "ini alamat saya?" dari satu konstanta ke **himpunan alamat lokal yang benar-benar ada di mesin**, lalu merangkai jalur terima eth1 ke mesin TCP/HTTP yang sama.

| Kelompok pekerjaan | Isi | Status akhir |
|---|---|---|
| Kontrak data | `WebLocalAddr { ip, mac, ifindex }` + `WEB_LOCAL_MAX = 2` + `WebRuntime::local` / `local_count` + `local_match(dst)` yang hanya menghitung entri hidup | `UNIT-TESTED` (test_47) |
| Gerbang TCP per alamat | `on_frame` memilih alamat lokal tujuan, mencatat `local_ip`/`local_mac` pada sesi, seluruh balasan (SYN/ACK, data, FIN, RST) bersumber dari sesi itu; kunci sesi naik ke 4-tuple | `RUNTIME-VERIFIED` (test_27–32 + bukti VM) |
| Jalur RX eth1 nyata | Frame TCP/80 dari eth1 masuk ke mesin yang sama; TX keluar lewat eth1 (`pump(lan_ip)` terpisah dari `pump(wan_ip)`); aging sesi sekali per putaran poll | `RUNTIME-VERIFIED` |
| Gerbang transit | Alamat eth1 ikut dihitung "milik kita" supaya tidak diteruskan sebagai trafik transit | `IMPLEMENTED` (tersentuh di jalur yang sama) |
| Same-origin API | `Host` wajib sama dengan **salah satu** alamat lokal hidup atau hostname router | `RUNTIME-VERIFIED` (test_48 + login nyata dari dua alamat) |
| Cacat jurnal/berita LAN | Irama GARP dipacu PIT (bukan hitungan putaran poll) dan alamat dicetak dari nilai nyata | `RUNTIME-VERIFIED` |

Gerbang akhir: `cargo fmt --all -- --check` exit 0; `cargo test --workspace` exit 0 **400 passed / 0 failed / 0 ignored**; `cargo check --target x86_64-unknown-none -p asl-kernel-bin` exit 0 (1 warning `dead_code` warisan di `power.rs:120`). Tidak ada tes yang dihapus, diturunkan kriterianya, atau dinonaktifkan; tidak ada pemeriksaan keamanan yang dilonggarkan.

## 2. Aturan yang diminta owner, dinyatakan sebagai kode

Sebelum:

```
layani hanya bila ip.dst_ip == rt.wan_ip            (satu alamat, keras)
Host wajib == rt.wan_ip                              (satu alamat, keras)
```

Sesudah:

```
layani bila ip.dst_ip ∈ { a : a ∈ rt.local, a.ifindex ≠ 0 }   → sesi diikat ke a
Host     wajib ∈ { str(a) : a ∈ rt.local, a.ifindex ≠ 0 } ∪ { hostname router }
selain itu                                                  → RST (jalur TCP), 403 host_mismatch (jalur API)
```

Entri `local` **tidak pernah dikarang**. Di kernel, himpunan itu dibangun dari keadaan nyata: eth0 masuk bila NIC ada ∧ taut up ∧ alamat ≠ `0.0.0.0`; eth1 masuk bila NIC ada ∧ beralamat. Mesin dengan satu NIC menghasilkan `local_count = 1` dan kelakuannya identik dengan sebelum sprint; mesin tanpa alamat hanya menghasilkan himpunan kosong, dan himpunan kosong berarti **diam total** — bukan melayani di alamat karangan.

## 3. Cacat yang hanya muncul saat diuji di VM

Kedua cacat di bawah lolos dari rancangan dan baru kelihatan setelah rantai kendali dijalankan di perangkat nyata. Keduanya diperbaiki, bukan dicatat lalu dibiarkan.

### 3.1 `same_origin` masih satu alamat → login dari eth1 dijawab 403

Bukti mentah dari jurnal kernel, sebelum perbaikan:

```
[R28] TCP SYN dari 192.168.56.1:56935 pada alamat lokal 192.168.56.100#2 -> SYN/ACK (SYN-RECEIVED)
[R28] TCP handshake selesai (ESTABLISHED)
[R28] HTTP /api/v1/auth/login -> 403 (271 bytes dibangun)
```

Sesi TCP-nya sudah benar-benar hidup di eth1 — artinya bagian TCP selesai — tetapi lapisan API menolak `Host: 192.168.56.100` karena membandingkannya hanya dengan alamat WAN. Perbaikan: `same_origin` membaca himpunan `rt.local` dan menolak entri mati (`ifindex == 0`), sehingga alamat yang belum dikonfigurasi tidak bisa dipakai membuka jalur tulis.

### 3.2 GARP dipacu hitungan putaran poll → banjir broadcast

`[R24]` sebelumnya memakai syarat `loop_count == 10 || (loop_count - 10) % 150 == 0`. Poll loop berjalan ribuan putaran per detik, jadi irama efektifnya **±60 ms**: terukur **10.040 baris "GARP announced" dalam ±10 menit** pada jurnal konsol, membanjiri segmen manajemen dan mengubur baris bukti lain. Baris itu juga mencetak alamat sebagai teks tetap (`"GARP announced 192.168.56.100"`), yang akan berbohong begitu alamat LAN bisa dikonfigurasi.

Perbaikan: irama dipacu `pit.elapsed_ms()` dengan batas 5.000 ms, dan alamat dicetak lewat `write_ip(lan_ip_real)` dari nilai nyata. Hasil pada boot sesudah perbaikan: **102 baris GARP** pada uptime ±10 menit (≈ 5 s/spasi, sesuai rancangan), dengan format baru `[R24] GARP eth1 announced <alamat> broadcast (42 bytes)`.

## 4. Penjaga unit yang ditambahkan (8 tes)

| Tes | Yang dipegang |
|---|---|
| api `test_47_himpunan_alamat_lokal_hanya_mengenali_entri_hidup` | `local_match` hanya mengembalikan slot hidup; alamat asing dan `0.0.0.0` → `None`; `local_count` membatasi bacaan; entri `ifindex = 0` tidak pernah melayani |
| api `test_48_login_ke_alamat_lokal_kedua_lolos_same_origin` | `POST /api/v1/auth/login` dengan `Host` = alamat lokal kedua → `200` + token; `Host` asing → `403 host_mismatch`; slot mati dengan IP yang sama dengan `Host` → tetap `403` |
| webui `test_27_syn_ke_alamat_wan_menjawab_dengan_alamat_wan` | SYN ke alamat #1 dijawab dari MAC/IP #1 |
| webui `test_28_syn_ke_alamat_lan_menjawab_dengan_mac_lan` | SYN ke alamat #2 dijawab dari MAC/IP #2 (bukan MAC eth0) |
| webui `test_29_syn_ke_alamat_asing_dijawab_rst_ingress` | RST dibangun dengan MAC sumber = MAC antarmuka yang **menerima** frame |
| webui `test_30_tanpa_alamat_lokal_server_diam_total` | `wan_ip` terisi tetapi `local_count = 0` → nol frame keluar, nol RST |
| webui `test_31_peer_sama_ke_dua_alamat_lokal_punya_slot_terpisah` | satu peer yang menyentuh dua alamat punya dua sesi terpisah (`active_sessions() == 2`) |
| webui `test_32_pump_tidak_mengirim_data_sesi_antarmuka_lain` | `pump(wan)` tidak boleh mengirim data sesi LAN; `pump(lan)` menyelesaikannya lewat MAC/IP LAN |

Perincian jumlah: baseline UI-02J 392 → 399 sesudah aturan alamat lokal → **400** sesudah `same_origin` ikut dijaga.

## 5. Bukti runtime di VM lab dua antarmuka

### 5.1 Keadaan yang harus dibereskan lebih dulu (dilaporkan apa adanya)

1. **UUID medium tidak cocok lagi setelah rebuild.** `startvm` gagal `E_FAIL … component MediumWrap` karena registry menahan UUID image sebelumnya. Pulih dengan urutan yang sudah terdokumentasi: lepas medium (`--medium none --type hdd`) → `closemedium disk <uuid-basi>` **tanpa** `--delete` → pasang ulang VDI → `startvm`. UUID medium yang diboot: `db37b6a8-e1f1-4a81-81f2-10a7fbfef17b`.
2. **RAM guest pernah turun ke 4 MB.** Boot pertama pada sesi ini **gagal** dan screenshot VGA menunjukkan `PANIC: High memory allocator: Out of memory` di dalam `freadall` saat Limine memuat `boot():/boot/rinjani-kernel.elf` (ELF debug 3.703.992 B). `VBox.log` dua run sukses terakhir mencatat MTRR `63 MB [64 MB]`, jadi nilai 4 MB adalah penyimpangan dari baseline lab; RAM dikembalikan ke **64 MB** sebelum bukti diambil. Tidak ada perubahan konfigurasi lain selain NIC2 host-only yang diizinkan owner.
3. **Rebuild disk menghapus RJCFG01.** Boot bersih menjawab `[R30] Auth: 0 kredensial dimuat dari RJCFG01` + `[R30] WebUI TERKUNCI` — fail-closed, bukan bug.

### 5.2 Boot & konfigurasi nyata

```
[R9]  ETH0 VIRTIO-NET DMA UP mac=08:00:27:XX:XX:XX
[R24] ETH1-LAN VIRTIO DMA UP mac=08:00:27:XX:XX:XX ip=192.168.56.100 qsz=1024 (arp+icmp responder armed, GARP periodic)
[R11] IP ADDR eth0 WAN: UNCONFIGURED — lease DHCP belum tiba
[R11] IP ADDR eth1 LAN: 192.168.56.100/24 (host-only)
[R18] TCP/HTTP ENGINE NATIF UP port=80 sesi_maks=8 (TCP native ASL, bukan smoltcp)
[R28] DHCP BOUND eth0=10.10.MM.RRR/24 (netmask sumber: option-1-ack) gw=10.10.MM.GGG (sumber: option-3-ack) …
[R30] WebUI TERKUNCI: belum ada kredensial.
```

Bootstrap kredensial lewat konsol serial (`auth set admin admin`, kata sandi pada prompt buta). Verifikasi senyap: prompt sandi dikirim dengan penanda `<direkam senyap>` dan **0** kemunculan kata sandi di seluruh jurnal (`grep -c` pada berkas log). Hasil:

```
[R26] RJCFG01 READBACK VERIFIED: slot 0 seq=3 (CRC header + slot cocok dari disk, PIO nyata)
% auth: akun 'admin' role admin, iterasi PBKDF2 1024, persistensi: tertulis dan terbaca kembali pada RJCFG01
```

### 5.3 Login dari dua alamat pada satu boot yang sama

Permintaan HTTP nyata dari host (WAN lewat antarmuka bridged klien `10.10.MM.CCC`, LAN lewat `192.168.56.1` pada adapter host-only):

| Alamat tujuan | `POST /api/v1/auth/login` | `GET /api/v1/system` | `GET /api/v1/interfaces` | `GET /` | `GET /js/core/api.js` |
|---|---|---|---|---|---|
| `10.10.MM.RRR` (eth0, ifindex 1) | `200` + token 64 hex | `200` | `200` | `200` 7.765 B | `200` 5.987 B |
| `192.168.56.100` (eth1, ifindex 2) | `200` + token 64 hex | `200` | `200` | `200` 7.765 B | `200` 5.987 B |

Jurnal kernel yang menjadi saksi (baris `#<ifindex>` adalah keluaran baru IFACE-01):

```
[R28] TCP SYN dari 10.10.MM.CCC:56933 pada alamat lokal 10.10.MM.RRR#1 -> SYN/ACK (SYN-RECEIVED)
[R28] HTTP /api/v1/auth/login -> 200 (379 bytes dibangun)
[R28] TCP SYN dari 192.168.56.1:51870 pada alamat lokal 192.168.56.100#2 -> SYN/ACK (SYN-RECEIVED)
[R28] HTTP /api/v1/auth/login -> 200 (379 bytes dibangun)
[R28] HTTP /api/v1/system -> 200 (440 bytes dibangun)
[R28] HTTP /api/v1/interfaces -> 200 (841 bytes dibangun)
```

Sesi dari kedua alamat berjalan **bersamaan** pada satu boot; dasbor membacanya sebagai `Sesi TCP aktif 3`.

### 5.4 Login WebUI nyata di peramban lewat alamat eth1

Peramban dihost-kan diarahkan ke `http://192.168.56.100/` → diarahkan ke `/login.html` (halaman masuk mandiri, logo dan CSS termuat dari antarmuka kedua) → form diisi → masuk → halaman `/` ter-render dengan nilai dari kernel, bukan contoh: judul `RINJANI 1.0.0 — Open Core Router Web Console`, `rinjani`, dropdown `AD · admin`, `uptime 7 menit 40 detik`, `WAN mode dhcp`, `Alamat bind HTTP 10.10.MM.RRR`, `Antrean tulis 0`, `Tulis ditolak 0`, `sinkron 16:08:45 (segar 5 s)`. Rail `MODUL BELUM AKTIF DI BUILD INI` tetap berisi lima modul yang memang belum lulus uji — tidak ada yang dipromosikan oleh sprint ini.

### 5.5 Jalur daya yang ikut teruji ulang

Perintah serial `shutdown` mengubah `VMState` menjadi `"poweroff"` (ACPI `PM1_CNT`), dipakai untuk mematikan VM sebelum rebuild disk.

## 6. Batas bukti (dinyatakan, bukan disembunyikan)

| Batas | Status | Alasan |
|---|---|---|
| Penolakan alamat asing (jalur RST) | `UNIT-TESTED` saja | Tidak dapat diproduksi di kabel nyata: NIC hanya menerima unicast untuk MAC-nya sendiri. Buktinya ada di test_29/test_30, bukan di VM |
| NIC ke-3 … ke-5 | **tidak dikerjakan** (Tahap B) | `WEB_LOCAL_MAX = 2`; registry antarmuka banyak belum ada |
| Alamat serving tidak muncul di JSON/Dashboard | **tidak dikerjakan** (Tahap B) | Dashboard masih menampilkan satu "Alamat bind HTTP" (eth0). Nilainya benar tetapi tidak lengkap sejak eth1 ikut melayani. owner memilih menunda eksposur alamat serving di Tahap A; penjaga `test_11` pada `/api/v1/interfaces` tidak diturunkan |
| Alamat eth1 masih konstanta waktu-build | `IMPLEMENTED` | `lan_ip_real` ditetapkan di kode, belum dari RJCFG01; karena itu baris `[R11]`/`[R24]` yang mencetaknya sebagai teks tetap masih cocok dengan kenyataan — tetapi akan jadi salah begitu alamat bisa diubah |
| DHCP server pool & DNS proxy di eth1 | `BLOCKED` (jalur terima belum dirangkai) | Jurnal `[R20]`/`[R21]` tetap jujur: `BELUM ADA JALUR RX` |
| Transport | HTTP polos, bukan TLS | Batas yang sudah tercatat sejak AUD-02; permukaan kendali kedua kini ada di segmen host-only, mitigasinya gerbang sesi fail-closed + same-origin |

## 7. Artefak build & verifikasi

| Item | Nilai |
|---|---|
| Kernel ELF (debug) | `target/x86_64-unknown-none/debug/asl-kernel`, 3.703.992 B |
| Disk RAW SHA256 | `3D24E2A54A0E9F5AE89D0860090708D7BE6866D7400FEC50FF04B22580FA9290` |
| VDI | `target/rinjani-1.0.0-x86_64-disk.vdi` → 9.437.184 B, UUID medium `db37b6a8-…-10a7fbfef17b` |
| Konsol serial | listener `SERIAL_TAG=iface01`, tee ke log di luar tree; 0 kebocoran kata sandi |
| Gerbang | fmt exit 0 · test 400/0 · check no-std exit 0 |

## 8. Status akhir pekerjaan (kosakata mandat §17)

- `IMPLEMENTED` — kontrak `WebLocalAddr`/`local_count`, gerbang TCP per alamat, pemisahan `pump`/`age_sessions`, pra-pemeriksa `frame_tcp_to_port`, same-origin atas himpunan lokal.
- `UNIT-TESTED` — 8 penjaga baru (test_47, test_48, test_27…test_32).
- `RUNTIME-VERIFIED` — login + `/api/v1/system` + `/api/v1/interfaces` + aset statis dari **dua** alamat pada satu boot; render penuh WebUI hasil masuk dari alamat eth1; irama GARP terukur 5 s dengan alamat tercetak nyata; kredensial tertulis dan terbaca kembali di RJCFG01.
- `BLOCKED` — DHCP server pool & DNS proxy di eth1 (jalur terima UDP belum dirangkai); lima modul rail "belum aktif" tidak disentuh.
- `NOT RUN` — boot dari ISO (hanya jalur VDI yang dipakai); pengujian 24 jam/tekanan.
- `OWNER-DECISION` — cakupan Tahap B (registry NIC 3–5, eksposur alamat serving di API/Dashboard, konfigurasi alamat eth1 lewat RJCFG01).

## 9. Rekomendasi langkah berikutnya

1. **IFACE-01 Tahap B:** registrasi antarmuka 3–5 (`WEB_LOCAL_MAX` naik bersama tabel), ekspos daftar alamat serving di `/api/v1/system` + sel "Alamat bind HTTP" di dasbor, dan pindahkan alamat eth1 dari konstanta build ke RJCFG01 agar baris `[R11]`/`[R24]` tidak lagi bergantung pada teks tetap.
2. **Rangkaikan jalur terima UDP/67 dan UDP/53 di eth1** supaya `[R20]`/`[R21]` bisa naik dari `BLOCKED`.
3. Primitive ICMP egress (#81) untuk membuka Ping/Trace Route — satu-satunya cara menguji jangkauan antar-antarmuka dari dalam mesin.
