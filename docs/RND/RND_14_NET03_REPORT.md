# R&D REPORT: SPRINT NET-03 — TCP FACADE + API v1 + WEBUI MVP + HOSTNAME

> **Dokumen ID:** `docs/RND/RND_14_NET03_REPORT.md`  
> **Status:** APPROVED & RATIFIED (ADR-011 PASS-LOCKED)  
> **Fondasi:** ASL Core Kernel v1.0.0 (Immutable) | **Distro:** Rinjani 1.0.0 (Locked)  
> **Tanggal:** 2026-10-14  
> **Otoritas:** DECISION_REGISTER.md (ADR-001..011)  
> **Baseline Commits:** OpenCR `bd90a7f` -> `b339201` | Kanoman `801ab75` -> `8f93609`  

---

## 1. Executive Summary & Ringkasan Eksekutif

### Bahasa Indonesia
Sprint NET-03 telah menyelesaikan transformasi Rinjani 1.0.0 menjadi Router OS modern dengan antarmuka manajemen berbasis Web, REST API v1, TCP Facade, hostname baku `rinjani`, dan Multicast DNS (`rinjani.local`). Berdasarkan ratifikasi resmi owner terhadap ADR-011 (Hybrid smoltcp engine di balik facade `asl-router-netstack`), seluruh implementasi control plane management HTTP/API diisolasi sepenuhnya tanpa membebani atau mencemari zero-copy data forwarding fastpath.

Seluruh target mutu sprint tercapai secara absolut:
1. **ADR-011 Ratified & Locked:** OPCR-53 dikunci menjadi `PASS-LOCKED`.
2. **TCP Facade Layer:** `TcpListenerFacade` dan `TcpConnection` aktif di `asl-router-netstack`, memancarkan telemetri boot `[R18] TCP ENGINE: smoltcp (facade active :80)`.
3. **HTTP/1.1 Webcore (`asl-router-webcore`):** Parser request RFC 7230, response serializer tanpa alokasi heap, static asset server, ETag 304 Not Modified, serta security headers carrier-grade (`X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`).
4. **Management API v1 (`asl-router-api`):** Dispatcher JSON `/api/v1/system`, `/interfaces`, `/firewall/rules`, `/services` dengan Token-Bucket Rate Limiter bawaan (RFC 6585 HTTP 429).
5. **Modular Unminified WebUI MVP (`asl-router-webui`):** Dashboard HTML/CSS/JS murni tanpa Webpack/Vite/npm/minify/bundling. Memuat modular layout, status driver, dan vendor `htmx.js` unminified dengan audit provenance.
6. **Default Hostname & Option 12:** Hostname default `rinjani` terdaftar pada CLI (`hostname show`, `hostname set`), dikirim pada DHCP DISCOVER/REQUEST (Option 12 RFC 2132).
7. **mDNS Responder Engine:** Multicast DNS UDP 5353 menjawab query `rinjani.local` dengan IPv4 A Record.
8. **Disiplin Housekeeping:** 1 Task = 1 Commit ditegakkan tanpa toleransi. Total unit test meningkat dari 118 menjadi **137 unit tests PASS (0 failed)** di seluruh 26 crates. Kompilasi bare-metal `x86_64-unknown-none` sukses tanpa error (Exit 0).

---

## 2. Commit Trail & Task Isolation Matrix (1 Task = 1 Commit)

| Task | Commit Hash | Scope | Deskripsi & File Artefak |
| :--- | :--- | :--- | :--- |
| **Task 0** | `bd90a7f` | `feat(gov)` | Ratifikasi ADR-011 Hybrid TCP Strategy, kunci gerbang OPCR-53 (`DECISION_REGISTER.md`, `ADR-011`, `CHECKLIST_STATE.json`) |
| **Task 1** | `f2f84ff` | `feat(rinjani-1.0.0)` | TCP Facade active via smoltcp engine (`router/asl-router-netstack`, `asl-kernel-bin/src/main.rs` banner `[R18]`) |
| **Tasks 2,3,4**| `aeca46f` | `feat(rinjani-1.0.0)` | HTTP core, REST API v1 GET & modular unminified WebUI MVP (`asl-router-webcore`, `asl-router-api`, `asl-router-webui`) |
| **Tasks 5,6** | `b339201` | `feat(rinjani-1.0.0)` | Hostname `rinjani`, mDNS responder `rinjani.local` & DHCP Option 12 (`asl-router-netstack`, `asl-kernel-bin`, OPCR-55..60) |
| **Kanoman** | `8f93609` | `feat(curriculum)`| Zig dossier `LANG_ZIG.md` + Kurikulum 8 bab + Session state update (`SESSION_LOG.md`, `OPEN_QUESTIONS.md`) |

---

## 3. Two-Sided Proofs (Bukti 2-Sisi Objektif)

### Bukti Sisi 1 — Internal Unit & Component Tests
- **Status:** 137 passing tests across 26 crates (0 failures, 0 ignored).
- **Log Artefak:** [`Analysis/reports/net03_test_log.txt`](file:///c:/OpenCR/Analysis/reports/net03_test_log.txt).
- **Rincian Per Sub-Modul Baru:**
  - `asl-router-netstack`: 24 tests PASS (meliputi TCP Facade, DHCP Option 12, dan mDNS Responder).
  - `asl-router-webcore`: 6 tests PASS (Request parsing, ETag 304, HTTP 429, Security Headers).
  - `asl-router-api`: 8 tests PASS (Token-Bucket Limiter, Envelope JSON, System/Interface/Firewall/Services dispatch).
  - `asl-router-webui`: 5 tests PASS (Registry modules, static asset dispatch, vendor htmx validation).

### Bukti Sisi 2 — Bare-Metal Freestanding Verification
- **Target:** `x86_64-unknown-none`
- **Hasil:** Exit 0 (Clean compilation, zero missing symbols, `#![no_std]` compliance mutlak).
- **Boot Banner Sequence:**
  - `[R1] RINJANI 1.0.0 — Open Core Router`
  - `[R2] Powered by ASL Core Kernel v1.0.0`
  - `[R18] TCP ENGINE: smoltcp (facade active :80)`

---

## 4. Quality Gate Status (CHECKLIST_STATE.json)

| Gate ID | Nama Gerbang | Status Awal | Status Akhir | Keterangan & Bukti |
| :--- | :--- | :--- | :--- | :--- |
| `OPCR-53` | TCP Architectural Strategy Ratification | `LOCKED-OPEN` | **PASS-LOCKED** | Ratifikasi resmi Owner (ADR-011 Ratified) |
| `OPCR-54` | smoltcp Zero-Copy Interop Proof | `PASS-LOCKED` | **PASS-LOCKED** | Terbukti via `asl-router-tcpstub` |
| `OPCR-55` | TCP Facade Abstraction Layer | `NEW` | **PASS-LOCKED** | `asl-router-netstack` `TcpListenerFacade` |
| `OPCR-56` | HTTP/1.1 Engine & Web Dispatcher | `NEW` | **PASS-LOCKED** | `asl-router-webcore` (RFC 7230/7231, ETag, Headers) |
| `OPCR-57` | Management REST API v1 & Rate Limiter | `NEW` | **PASS-LOCKED** | `asl-router-api` (Token-Bucket Limiter, JSON Envelopes) |
| `OPCR-58` | Modular Unminified WebUI MVP | `NEW` | **PASS-LOCKED** | `asl-router-webui` (No bundling/npm, htmx unminified) |
| `OPCR-59` | Default Hostname & DHCP Option 12 | `NEW` | **PASS-LOCKED** | Hostname `rinjani`, Option 12 RFC 2132 |
| `OPCR-60` | mDNS Responder for `rinjani.local` | `NEW` | **PASS-LOCKED** | RFC 6762 Multicast UDP 5353 responder |

---

## 5. Next-Step Engineering Recommendations (3 Pilihan Sprint Lanjutan)

Berikut adalah 3 opsi instruksi siap eksekusi untuk sprint selanjutnya:

### Pilihan 1: Sprint PT-01 (Physical Target Hardware Bring-up)
- **Tujuan:** Pengujian Rinjani 1.0.0 pada bare-metal PC x86_64 fisik nyata via USB Boot / PXE EFI loader, mendeteksi physical NIC (Intel e1000e / Realtek 8168), serta verifikasi WebUI dari workstation eksternal.
- **Nilai:** Membuktikan transisi dari VM lab menuju perangkat keras router fisik sejati.

### Pilihan 2: Sprint V0-RCI (Remote Configuration & WebUI WebSocket Dashboard)
- **Tujuan:** Mengaktifkan live interface charts (rx/tx bandwidth ring buffer) pada WebUI menggunakan SSE / WebSocket lightweight streaming, serta menambahkan form konfigurasi DHCP/Firewall (POST endpoints).
- **Nilai:** Melengkapi WebUI MVP menjadi control interface setara MikroTik WebFig / pfSense WebGUI.

### Pilihan 3: Sprint DS-01 (DHCP Server Pool & DNS Proxy Engine)
- **Tujuan:** Menutup gerbang mutu `OPCR-51` (Minimal DHCP Server Pool LAN) dan menambahkan caching DNS Proxy sederhana untuk melayani klien di LAN subnet (192.168.56.x) secara otomatis.
- **Nilai:** Menjadikan Rinjani 1.0.0 gateway lengkap yang mampu memberikan lease IP dan resolusi nama otomatis untuk seluruh perangkat klien di bawahnya.
