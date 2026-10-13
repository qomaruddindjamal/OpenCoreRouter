# EVALUASI STRATEGI TCP & KRITERIA EVALUASI TERUKUR (SPRINT EVAL-01)
## EVAL-01 CRITERIA: GOVERN BEFORE RESEARCH

> **Status Dokumen:** RATIFIED BY ARCHITECT — GOVERNANCE PRE-CONDITION  
> **Target Substrat:** Open Core Router (Rinjani 1.0.0 / ASL Core Kernel v1.0.0 `#![no_std]`)  
> **Tujuan:** Menetapkan kriteria objektif, bobot, dan ambang batas lulus (threshold) sebelum menguji atau mengintegrasikan stack TCP (smoltcp vs native stack).  
> **Otoritas:** DECISION_REGISTER.md (ADR-001..010)  

---

## 1. Matriks Kriteria Evaluasi (Terukur & Berbobot)

| ID | Dimensi Evaluasi | Parameter & Ambang Lulus (Threshold) | Bobot | Metode Pengujian |
| :---: | :--- | :--- | :---: | :--- |
| **K1** | **Doktrina Platform & `#![no_std]`** | Wajib `no_std` murni, tanpa libc/glibc, tanpa syscall Linux (`epoll`, `socket`, `netlink`). Ambang: Kompilasi `x86_64-unknown-none` lolos tanpa warning. | **20%** | `cargo check --target x86_64-unknown-none` |
| **K2** | **Kesesuaian Lisensi (GPL-3.0 Interop)** | Lisensi aktual upstream wajib kompatibel GPL-3.0-or-later (0-BSD, MIT, atau Apache-2.0). Ambang: `cargo-deny check license` lulus 100%. | **15%** | Audit repo upstream & `cargo-deny` |
| **K3** | **Zero-Copy Ring Interoperability** | Mampu menerima & mengirim paket via `DescriptorRing` / `netbuf pool` tanpa copy payload paket. Ambang: Buffer slice diteruskan langsung ke ring TX. | **20%** | Code inspection & unit test adapter trait |
| **K4** | **Kelengkapan Fitur TCP Carrier** | Wajib: Handshake 3-way (SYN/SYN-ACK/ACK), Window flow control, Retransmit timeout (RTO), graceful shutdown (FIN/RST), TIME_WAIT. Kejujuran: Fitur tidak didukung wajib didaftar. | **15%** | Verifikasi RFC 793 compliance suite |
| **K5** | **Dampak Kinerja (Degradasi vs Forwarding)** | Mengukur throughput socket TCP lokal vs baseline forwarding L3 (842 Mbps). Ambang: Minimal throughput >= 150 Mbps untuk control-plane/HTTP API tanpa stall. | **10%** | Benchmark iperf3 / burst data transfer |
| **K6** | **Maintainability & Upstream Activity** | Proyek aktif, rilis stabil, rekam jejak bebas CVE kritis, bebas alokasi dinamis wajib (bisa heapless/bounded capacity). | **10%** | Audit commit history, releases, and issue tracking |
| **K7** | **Ukuran Kode & Footprint Memori (RAM)** | Footprint binary tambahan < 150 KB, alokasi buffer statis dapat dikonfigurasi saat compile time. | **10%** | Size analysis biner ELF & BSS/Data section |

---

## 2. Kriteria Eliminasi Mutlak (Disqualifying Red Lines)
Jika salah satu kondisi berikut terlanggar, kandidat opsi dinyatakan **GUGUR OTOMATIS**:
1. Mengharuskan `std` atau dependensi terhadap syscall Linux (`/proc`, `libc`, `nix`).
2. Lisensi incompatibel dengan GPL-3.0-or-later (seperti GPLv2-only tanpa klausa "or later", lisensi proprietary tertutup, atau lisensi copyleft non-GPL).
3. Membutuhkan runtime garbage collector dinamis atau alokator heap tak terbatas di jalur packet handler.
4. Kode sumber kernel/driver disalin secara mentah tanpa atribusi atau melanggar hak cipta.

---

## 3. Ambang Kelulusan Komposit (Passing Threshold)
- Nilai tertimbang kumulatif minimum: **80 / 100**.
- Rekomendasi akhir diserahkan kepada Project Lead/Owner melalui `ADR-011` dengan status `PROPOSED`.
