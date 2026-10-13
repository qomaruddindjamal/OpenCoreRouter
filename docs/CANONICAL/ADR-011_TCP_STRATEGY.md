# ADR-011: TCP STACK ARCHITECTURAL STRATEGY FOR OPEN CORE ROUTER

> **Status:** PROPOSED (Menunggu Ratifikasi Resmi Project Lead / Owner)  
> **Tanggal:** 2026-10-14  
> **Wilayah:** Control Plane, HTTP REST API, WebUI, and Transport Layer Networking  
> **Otoritas:** DECISION_REGISTER.md (ADR-001..010) & docs/RND/eval01_criteria.md  
> **Gerbang Mutu:** OPCR-53 (TCP Strategy Ratification)  

---

## 1. Konteks Masalah
Open Core Router (Code OS Rinjani 1.0.0 / ASL Core Kernel v1.0.0) telah membuktikan forwarding IP carrier-grade (842 Mbps TCP / 915 Mbps UDP) dengan zero-copy descriptor ring. Untuk mendukung kebutuhan fase berikutnya (`NET-03`), yaitu antarmuka manajemen berbasis HTTP REST API v1 GET, WebUI modular berbasis HTMX terbuka, dan CLI daemon remote, Rinjani memerlukan lapisan transport TCP yang andal dan aman.

Namun, mengimplementasikan stack TCP state-machine dari awal (*from scratch*) membawa risiko kompleksitas tinggi pada retransmission timer, congestion control, window scaling, dan penanganan out-of-order segment. Sementara itu, mengadopsi library pihak ketiga harus diuji secara ketat agar tidak melanggar Doktrina Platform (`#![no_std]`, lisensi GPL-3.0, zero-copy interop).

---

## 2. Kriteria Evaluasi & Hasil Uji Nyata (Sprint EVAL-01)

Berdasarkan matriks kriteria terukur pada `docs/RND/eval01_criteria.md`:
1. **K1 Doktrina (`#![no_std]` murni):** `PASS (100%)`. smoltcp dirancang native untuk embedded Rust `#![no_std]` tanpa asumsi libc atau Linux syscalls.
2. **K2 Lisensi Aktual:** `PASS (100%)`. Upstream smoltcp berlisensi **0BSD** (Zero-Clause BSD), yang secara mutlak kompatibel dengan GPL-3.0-or-later dan diizinkan oleh `deny.toml`.
3. **K3 Zero-Copy Interop:** `PASS (100%)`. Terbukti melalui crate `asl-router-tcpstub`, adapter `RingDeviceAdapter` mampu meminjamkan buffer `DescriptorRing` langsung via `RxToken` dan `TxToken` tanpa copy ganda payload paket.
4. **K4 Kelengkapan Fitur:** Handshake 3-way, retransmit timeout (RTO), sliding window, graceful shutdown (FIN/RST), dan TIME_WAIT didukung penuh.
   *Fitur yang belum ada / keterbatasan jujur:* Belum mendukung SACK (Selective Acknowledgment) penuh pada profil minimal, dan window scaling di-cap pada batas static buffer.
5. **K5 Performa & Degradasi:** Throughput socket lokal terminasi mencapai **326.4 Mbps** (degradasi terukur -61.2% dibanding 842 Mbps transit L3). Nilai ini melampaui ambang batas minimum lulus (150 Mbps) dan sangat memadai untuk traffic management control-plane.
6. **K6 Maintainability Upstream:** Komunitas aktif, didukung oleh ekosistem Rust embedded global, rilis stabil v0.11.x.
7. **K7 Ukuran Kode & Footprint:** Tambahan biner sekitar ~65 KB pada BSS/Text section, dengan buffer alokasi statis terikat.

---

## 3. Komparasi Strategi (Tiga Jalur Pilihan)

| Parameter Evaluasi | Opsi 1: VENDOR Penuh smoltcp | Opsi 2: NATIVE State Machine | Opsi 3: HYBRID BERTAHAP (Rekomendasi) |
| :--- | :--- | :--- | :--- |
| **Waktu Menuju API/WebUI Siap** | Sangat Cepat (1 sprint) | Lambat (3-4 sprint) | Cepat (1 sprint untuk MVP) |
| **Risiko Bug Protokol (RFC 793)**| Sangat Rendah (teruji di industri) | Sangat Tinggi (edge-cases retransmit)| Rendah (smoltcp sebagai engine awal) |
| **Beban Pemeliharaan** | Rendah (upstream fixes) | Tinggi (harus di-maintain sendiri) | Terkendali melalui Facade API |
| **Kontrol Arsitektur & Kedaulatan**| Bergantung upstream | 100% kontrol berdaulat | **100% Kontrol via Facade Netstack** |

---

## 4. Keputusan Arsitektural (Rekomendasi Tim R&D)
Tim R&D merekomendasikan **OPSI 3: HYBRID BERTAHAP (VENDOR SMOLTCP DI BALIK FACADE NATIVE)**:

1. **Facade Architecture (ADR-010 Open Modular Principle):**
   Konsumen di kernel dan aplikasi (seperti HTTP API, WebUI, CLI daemon) **DILARANG** mengimpor atau memanggil tipe smoltcp secara langsung. Seluruh interaksi wajib melewati abstraksi facade di `asl-router-netstack` (misalnya `TcpListener`, `TcpStream`, `TcpSocketPool`).
2. **Kedaulatan Jangka Panjang:**
   Dengan arsitektur facade ini, engine di balik layar dapat digantikan dengan *Native Carrier-Grade TCP Engine* pasca rilis GA (General Availability) tanpa mengubah atau memecah satu baris pun kode pada layer WebUI atau API manajemen.
3. **Pemberian Status:**
   ADR-011 berstatus **`PROPOSED`** hingga diratifikasi secara resmi oleh Project Lead / Owner.

---

## 5. Konsekuensi
- **Positif:** Menghemat 3 sprint rekayasa TCP, memungkinkan tim fokus langsung membangun WebUI modular HTMX dan REST API di Sprint `NET-03`.
- **Mitigasi:** Upstream smoltcp di-freeze pada versi stabil v0.11.1 dan di-vendor tanpa modifikasi internal untuk menjamin audit integritas.
