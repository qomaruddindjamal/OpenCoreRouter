# DECISION REGISTER — OPEN CORE ROUTER & ASL CORE KERNEL

> **Status:** AUTHORITATIVE & RATIFIED  
> **Authority:** Project Lead / Owner (`qomaruddindjamal`)  
> **Last Updated:** 2026-10-14  
> **Standard:** DO-178C DAL-A Configuration Management  

---

## 1. Summary of Architectural & Governance Decisions

| Decision ID | Date | Subject | Decision Summary | Status |
| :--- | :--- | :--- | :--- | :--- |
| **ADR-001** | 2026-10-13 | Naming & Foundation | "ASL Core Kernel" adalah fondasi mutlak (immutable). Dilarang me-rename atau menduplikasi sebagai dual-kernel. | `RATIFIED` |
| **ADR-002** | 2026-10-13 | Kernel Version Lock | Versi fondasi terkunci di `v1.0.0` (git tag `v1.0.0` commit `8ff19c3f`). | `RATIFIED` |
| **ADR-003** | 2026-10-14 | Distribution Version Lock | Versi distro resmi dikunci owner di `Rinjani 1.0.0`. Histori versi 1.0.3 dipensiunkan permanen. | `RATIFIED` |
| **ADR-004** | 2026-10-13 | Platform Doctrine | "ASL Core is NOT Linux". Microkernel `#![no_std]`, no glibc, no Linux syscalls, no netlink, native carrier stack. | `RATIFIED` |
| **ADR-005** | 2026-10-14 | Codebase Licensing | Source code dilisensikan di bawah **GPL-3.0-or-later**. Dokumentasi di bawah **CC-BY-4.0**. | `RATIFIED` |
| **ADR-006** | 2026-10-14 | Hardware Inventory Doctrine | "Real Hardware Only". Enumerasi nyata PCI & CPUID. Bila device nihil, tampilkan "none detected". Dilarang dummy/mock di kernel bin. | `RATIFIED` |
| **ADR-007** | 2026-10-14 | External Audit Boundary | Probe telemetri eksternal (Mitranet 10.10.66.228) mutlak **READ-ONLY**. Dilarang mutasi, restart, atau reboot. | `RATIFIED` |
| **ADR-008** | 2026-10-14 | Interrupt vs Polling Strategy | Polling-first sebagai mode default fail-safe. Local APIC timer framework diaktifkan via flag terverifikasi. | `RATIFIED` |
| **ADR-009** | 2026-10-14 | Language Policy | Kernel/router paket = Rust no_std; Host tools rilis = Rust std; Build script PS diperbolehkan hari ini, migrasi cargo-xtask "sentuh-berarti-ganti"; WebUI htmx+JS, berat WASM; tools/ bebas; bahasa lain = referensi. | `RATIFIED` |
| **ADR-010** | 2026-10-14 | Open Interoperability Doctrine | Standar terbuka IETF dulu; dialek populer dijembatani dari spek publik (EoIP/MNDP); dilarang RE proprietary (Winbox); prioritas: EoIP+MNDP -> RIP -> OSPF -> BGP; SNMP read-only dulu. | `RATIFIED` |
| **ADR-011** | 2026-10-14 | TCP Strategy & Architecture | Hybrid smoltcp engine terisolasi di balik facade `asl-router-netstack`. Fastpath data forwarding tetap zero-copy mandiri. | `RATIFIED` |
| **ADR-012** | 2026-10-14 | Sprint DS-01 Scope & Mitranet M0 | Ratifikasi owner untuk Sprint DS-01 (DHCP Server eth1 + DNS Proxy + WebUI Mitranet M0 + Pelengkap Bukti Runtime NET-03). Opsi 2 di-rename W2; istilah V0-RCI tetap untuk disk image & Real Capacity (OPCR-25..27). | `RATIFIED` |

---

## 2. Detailed Decision Records

### ADR-001: Immutable Foundation Identifier
- **Context:** Terjadi ketidakjelasan penamaan modul pada fase awal riset antara brand distribusi dan fondasi kernel.
- **Decision:** Nama **ASL Core Kernel** adalah nama fondasi abadi. Crate fondasi wajib ber-prefix `asl-` di subfolder `asl-kernel/`. Crate router ber-prefix `asl-router-` di subfolder `router/`.
- **Consequences:** Struktur repositori terpisah secara modular, bersih, dan mematuhi DO-178C DAL-A trace hierarchy.

### ADR-002: Kernel Version v1.0.0
- **Context:** Pencegahan version drift di level kernel bare-metal.
- **Decision:** Versi kernel terkunci di `v1.0.0` sesuai rilis resmi commit `8ff19c3f` di repo induk `asl`.
- **Consequences:** Workspace crate fondasi memakai versi workspace `0.1.0` secara internal, namun identitas kernel runtime adalah `v1.0.0`.

### ADR-003: Distro Release Rinjani 1.0.0
- **Context:** Permintaan owner untuk mengonsolidasikan versi rilis produk menjadi 1.0.0 awal.
- **Decision:** Versi distro Open Core Router dikunci resmi pada **Rinjani 1.0.0**. Boot banner wajib mencetak:
  - `[R1] RINJANI 1.0.0 — Open Core Router`
  - `[R2] Powered by ASL Core Kernel v1.0.0`
- **Consequences:** Seluruh gate verifikasi melacak kesiapan produk menuju GA di atas baseline 1.0.0.

### ADR-004: Pure Bare-Metal (#![no_std]) Non-Linux
- **Context:** Kebutuhan sistem operasi router avionik berstandar DO-178C DAL-A dan ARINC 653 IMA.
- **Decision:** Tidak mengimpor Linux kernel, C runtime (libc/glibc/musl), systemd, busybox, atau netlink. Driver Ethernet (VirtIO, Intel 82599) dan protokol jaringan (ARP, IPv4, ICMP, DHCP, UDP) diimplementasikan secara native di Rust `#![no_std]`.
- **Consequences:** Waktu boot instan (< 1 ms), deterministik temporal penuh, zero surface area kerentanan Linux userland.

### ADR-005: Licensing Framework (GPL-3.0-or-later)
- **Context:** Klarifikasi hak cipta dan lisensi perangkat lunak terbuka dengan perlindungan copyleft kuat.
- **Decision:** Seluruh kode Rust di repositori OpenCR dan ASL Core Kernel berlisensi GPL-3.0-or-later. Dokumentasi berlisensi CC-BY-4.0. Merek dagang tetap dilindungi hak cipta owner `qomaruddindjamal`.
- **Consequences:** Memastikan kebebasan kode tetap terlindungi seraya menjaga integritas identitas proyek.

### ADR-006: Real Hardware Only (Zero Dummy Data)
- **Context:** Integritas telemetri dan audit diagnostik perangkat keras.
- **Decision:** Hardware inventory hanya melaporkan device yang benar-benar terdeteksi melalui pembacaan register PCI Configuration Space (0xCF8/0xCFC) dan instruksi CPUID. Jika tidak ada perangkat ditemukan, sistem melaporkan "none detected" atau count = 0.
- **Consequences:** Menjamin akurasi operasional penerbangan/carrier dan integritas telemetry audit.

### ADR-009: Comprehensive Language Policy
- **Context:** Standarisasi bahasa pemrograman di seluruh lapisan proyek OpenCR untuk menjamin keandalan, portabilitas, dan efisiensi pemeliharaan.
- **Decision:** 
  1. **Kernel & Layanan Paket Router:** Wajib Rust `#![no_std]` bare-metal tanpa dynamic allocator (`alloc`) di fastpath.
  2. **Host Tools Rilis:** Rust `std` (CLI diagnostics, testing tools, daemon host).
  3. **Build Scripts:** Skrip PowerShell diperbolehkan untuk workflow saat ini; migrasi menuju `cargo-xtask` dilakukan dengan prinsip "sentuh-berarti-ganti".
  4. **Web UI:** Menggunakan pendekatan modular `htmx` + Vanilla JavaScript; modul visualisasi/kripto berat diperbolehkan menggunakan Rust terkompilasi ke WASM.
  5. **Skrip Sekali-Pakai & Otomasi:** Skrip evaluasi/audit di `tools/` bebas bahasa (Python, Bash, PowerShell).
  6. **Bahasa Lain di Dunia (156 Bahasa):** Berstatus literasi teknis, analisis komparatif, dan referensi interoperabilitas — bukan untuk kode produksi runtime.
- **Consequences:** Kode produksi terlindungi dari fragmentasi dependensi dan tetap berkinerja carrier-grade.

### ADR-010: Open Interoperability Doctrine
- **Context:** Kebutuhan interoperabilitas jaringan OpenCR dengan ekosistem vendor heterogen (MikroTik, Cisco, Juniper) tanpa kompromi legal atau vendor lock-in.
- **Decision:**
  1. **Standard Protocols First:** Utamakan standar terbuka IETF (IEEE 802.3, ARP RFC 826, IPv4 RFC 791, ICMP RFC 792, UDP RFC 768, TCP RFC 793, DHCP RFC 2131).
  2. **Bridging Dialek Populer:** Protokol populer dijembatani HANYA dari spesifikasi teknis publik (misal EoIP RFC draft / MNDP broadcast).
  3. **Larangan Rekayasa Balik Proprietary:** DILARANG KERAS merekayasa-balik (reverse engineering) antarmuka/protokol proprietary tertutup seperti MikroTik Winbox protocol.
  4. **Bebas Vendor Lock-in Dua Arah:** Sistem tidak boleh bergantung pada satu vendor maupun mengunci pengguna ke satu vendor.
  5. **Prioritas Implementasi Interoperabilitas:** Urutan prioritas rute dan tunneling: `EoIP + MNDP` -> `RIP (RFC 2453)` -> `OSPFv2 (RFC 2328)` -> `BGP-4 (RFC 4271)`. Protokol manajemen `SNMP` diimplementasikan mode READ-ONLY terlebih dahulu.
- **Consequences:** Menjamin kepatuhan hukum penuh, keterbukaan sistem, dan adopsi luas di jaringan ISP/carrier.
