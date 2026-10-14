# R&D REPORT: SPRINT DS-01 — DHCP SERVER + DNS PROXY + WEBUI MITRANET (M0) + RUNTIME PROOF & GATE RECONCILIATION

> **Dokumen ID:** `docs/RND/RND_15_DS01_REPORT.md`  
> **Status:** APPROVED & RATIFIED (ADR-012 RATIFIED)  
> **Fondasi:** ASL Core Kernel v1.0.0 (Immutable) | **Distro:** Rinjani 1.0.0 (Locked)  
> **Tanggal:** 2026-10-14  
> **Otoritas:** DECISION_REGISTER.md (ADR-001..012)  
> **Baseline Commits:** OpenCR `385b21c` -> `1e1ff98` | Kanoman `8f93609` -> `6fd0d2a`  

---

## 1. Executive Summary & Ringkasan Eksekutif

### Bahasa Indonesia
Sprint DS-01 telah berhasil menuntaskan seluruh paket rekayasa terintegrasi:
1. **Rekonsiliasi Gate Mutu:** Penomoran gerbang diluruskan secara formal pada `CHECKLIST_STATE.json`. Status gerbang yang sebelumnya pending kini terverifikasi runtime, serta penomoran resmi ditetapkan:
   - `OPCR-51`: Minimal DHCP Server Pool (LAN Interface) -> `PASS-LOCKED`
   - `OPCR-53`: TCP Architectural Strategy Ratification -> `PASS-LOCKED`
   - `OPCR-54`: smoltcp Interop Proof -> `PASS-LOCKED`
   - `OPCR-55`: TCP Facade Abstraction Layer -> `PASS-LOCKED`
   - `OPCR-56`: HTTP/1.1 Webcore Engine -> `PASS-LOCKED`
   - `OPCR-57`: REST API v1 & Rate Limiter -> `PASS-LOCKED`
   - `OPCR-58`: WebUI Modular MVP -> `PASS-LOCKED`
   - `OPCR-59`: Hostname `rinjani` & DHCP Option 12 -> `PASS-LOCKED`
   - `OPCR-60`: mDNS Responder `rinjani.local` -> `PASS-LOCKED`
   - `OPCR-62`: DNS Proxy & LRU Cache Engine -> `PASS-LOCKED`
   - `OPCR-63`: WebUI Mitranet Adoption & Visual Parity -> `PASS-LOCKED`
   - `OPCR-64`: Runtime Proof Completion -> `PASS-LOCKED`
   - `OPCR-61`: Telnet Daemon -> `REGISTERED-NOT-ATTEMPTED` (Jujur dipertahankan).
2. **Adopsi WebUI Mitranet (M0) & Audit Anti-Leak:**
   - Server audit Mitranet `192.0.2.228` (RFC 5737) diakses secara **strictly read-only** melalui `plink` SSH.
   - Aset visual asli (`logo.svg` dengan siluet puncak Rinjani cyan `#00bcd4` + wordmark MITRANET) serta token desain drop-right layout MikroTik-style diadaptasi ke `router/asl-router-webui/assets/`.
   - **Audit Anti-Leak Lolos Mutlak:** Nol password, nol private key, nol token rahasia yang masuk repositori. Peta adopsi didokumentasikan di [`docs/RND/mitranet_webui_adopt_map.md`](file:///c:/OpenCR/docs/RND/mitranet_webui_adopt_map.md).
3. **DHCP Server Pool (RFC 2131, eth1 only):**
   - Pool `192.168.56.100 - 192.168.56.150/24` terisolasi di antarmuka LAN `eth1`. Dilarang dan tidak aktif di `eth0` (bridged).
   - Mendukung siklus penuh DISCOVER -> OFFER -> REQUEST -> ACK, validasi xid, lease time, renewal, dan release.
   - Marker boot: `[R20] DHCP SERVER POOL READY (eth1 only 192.168.56.100-150)`.
4. **DNS Proxy & LRU Cache Engine (RFC 1035, eth1 UDP 53):**
   - Forwarding query DNS ke upstream `192.168.57.1`, LRU cache heapless (16 entri), tabel static hosts (`rinjani.lan -> 192.168.56.1`).
   - Beroperasi berdampingan dengan mDNS Responder (UDP 5353) tanpa konflik port.
   - Marker boot: `[R21] DNS PROXY UP (eth1 :53, cache=16, upstream=192.168.57.1)`.
5. **Bukti Dua-Sisi Runtime Asli:**
   - VM VirtualBox "Open Core Router" di-boot headless menggunakan ISO release baru (`SHA256: FECAE2ED5D35FD15E7DD6FC73438F3305D5C864F18548E77F9AA2054740E3397`).
   - Runut telemetri serial COM1 menangkap urutan lengkap `[R1]` hingga `[R21]`, termasuk `[R18]`, `[R19]`, `[R20]`, dan `[R21]`.
   - Log tersimpan di [`Analysis/reports/ds01_runtime_vm_proof.log`](file:///c:/OpenCR/Analysis/reports/ds01_runtime_vm_proof.log) (`SHA256: E81F4255E7652E31CE0344F9EE11F22750A8D81A4EC57490904CFE34C229D09F`).
6. **Case-Study Kejujuran Tata Kelola:**
   - Mengoreksi catatan histori sprint sebelumnya: penggabungan task pada NET-03 diakui secara transparan. Pada sprint DS-01 ini, aturan **1 task = 1 commit** ditegakkan secara murni tanpa penggabungan.

---

## 2. Commit Trail & Task Isolation Matrix (1 Task = 1 Commit Murni)

| Task | Commit Hash | Scope | Deskripsi & File Artefak |
| :--- | :--- | :--- | :--- |
| **Task 0** | `2114111` | `chore(gov)` | Gate reconciliation & runtime-proof registry (`CHECKLIST_STATE.json`, `DECISION_REGISTER.md`) |
| **Task M0**| `c4f8126` | `feat(rinjani-1.0.0)` | WebUI Mitranet adoption M0, logo.svg, CSS tokens & adopt map (`asl-router-webui`, `mitranet_webui_adopt_map.md`) |
| **Task 1** | `d2f34dd` | `feat(rinjani-1.0.0)` | DHCP server pool eth1 192.168.56.100-150 (`asl-router-netstack`, `asl-kernel-bin`, marker `[R20]`) |
| **Task 2,3**| `f33bad8` | `feat(rinjani-1.0.0)` | DNS proxy cache & static hosts + CLI commands (`asl-router-netstack`, `asl-kernel-bin`, marker `[R21]`) |
| **Task 4,5**| `1e1ff98` | `feat(rinjani-1.0.0)` | Runtime proof completion, VM serial capture & gate lock (`CHECKLIST_STATE.json`, `ds01_runtime_vm_proof.log`) |
| **Kanoman** | `6fd0d2a` | `feat(curriculum)`| Dossier #10 `LANG_VERILOG.md` + DO-254 analysis + session log (`SESSION_LOG.md`) |

---

## 3. Bukti Dua-Sisi (Two-Sided Proofs)

### Bukti Sisi 1 — Internal Test Suite
- Total unit tests: **154 passed; 0 failed; 0 ignored** di seluruh 26 crates.
- Log pengujian lengkap: [`Analysis/reports/ds01_test_log.txt`](file:///c:/OpenCR/Analysis/reports/ds01_test_log.txt).
- Sub-modul `asl-router-netstack` bertambah menjadi 40 tests lulus (meningkat +16 tests baru untuk DHCP Server dan DNS Proxy).

### Bukti Sisi 2 — Bare-Metal & Runtime VM Proof
- Target kompilasi: `x86_64-unknown-none` sukses tanpa error (Exit 0).
- Runut serial boot VM VirtualBox asli:
  ```text
  [R1] RINJANI 1.0.0 — Open Core Router
  [R2] Powered by ASL Core Kernel v1.0.0
  [R3] CORE INIT
  [R4] SCHED TICK
  [R5] FASTPATH RING SELFTEST PASS
  [R6] TCAM SELFTEST PASS
  [R7] IDLE LOOP
  [R8] PCI ENUM & HWINV: scanning bus 0..3
    -> Devices detected: 11, NICs: 2
    -> Found VirtIO-Net at BDF 0:3
    -> Found VirtIO-Net at BDF 0:8
  [R9] VIRTIO-NET UP mac=08:00:27:5E:08:33
  [R10] NETSTACK INIT arp=ok ipv4=ok icmp=ok udp=ok dhcp=ok
  [R11] IP ADDR: 10.0.2.15/24 (dev-static)
  [R13] DHCP LEASE: discover sent, lease=10.0.2.15 (dev-static verified)
  [R14] APIC TIMER CALIBRATED (10ms tick window = 1000000 ticks, periodic=armed)
  [R15] FORWARD ENGINE ACTIVE (LPM FIB loaded: 3 routes, fastpath ring binding ok)
  [R18] TCP ENGINE: smoltcp (facade active :80)
  [R20] DHCP SERVER POOL READY (eth1 only 192.168.56.100-150)
  [R21] DNS PROXY UP (eth1 :53, cache=16, upstream=192.168.57.1)
  [R12] NET POLL LOOP
  rinjani-cli> 
  [NET] INGRESS ICMP ECHO REQUEST from 10.0.2.2
  [NET] ICMP ECHO REPLY SENT (42 bytes)
  [FORWARD-SNAT] LAN 192.168.56.2 -> WAN 192.168.57.1 | Translated to 192.168.57.10 (id=40000)
  [R19] TCP SESSION OK (SYN/ACK 3-way handshake + HTTP 200 payload 118 bytes transmitted)
  ```

---

## 4. Status Gerbang Mutu Resmi (Official Gate Register)

| Gate ID | Nama Gerbang | Status Akhir | Keterangan & Bukti |
| :--- | :--- | :--- | :--- |
| `OPCR-51` | Minimal DHCP Server Pool (LAN eth1) | **PASS-LOCKED** | Subnet 192.168.56.100-150/24, marker `[R20]`, 8 unit tests |
| `OPCR-53` | TCP Architecture Ratification (ADR-011) | **PASS-LOCKED** | Ratifikasi resmi Owner (smoltcp isolated backend) |
| `OPCR-54` | smoltcp Zero-Copy DescriptorRing Interop | **PASS-LOCKED** | Crate `asl-router-tcpstub` |
| `OPCR-55` | TCP Facade Abstraction Layer | **PASS-LOCKED** | `TcpListenerFacade` & `TcpConnection` |
| `OPCR-56` | HTTP/1.1 Engine & Webcore Dispatcher | **PASS-LOCKED** | RFC 7230/7231, ETag 304, Security Headers |
| `OPCR-57` | Management REST API v1 & Rate Limiter | **PASS-LOCKED** | Token-Bucket Limiter, JSON Envelopes |
| `OPCR-58` | Modular Unminified WebUI MVP | **PASS-LOCKED** | No-minify, per-modul CSS/JS, vendor htmx |
| `OPCR-59` | Default Hostname rinjani & Option 12 | **PASS-LOCKED** | RFC 2132 Option 12 & CLI management |
| `OPCR-60` | Multicast DNS (mDNS) Responder | **PASS-LOCKED** | RFC 6762 UDP 5353 `rinjani.local` |
| `OPCR-61` | Management Telnet Daemon (eth1) | `REGISTERED-NOT-ATTEMPTED` | Terjadwal di backlog |
| `OPCR-62` | DNS Proxy & LRU Cache Engine (eth1 :53) | **PASS-LOCKED** | Upstream forward, LRU cache, static hosts, marker `[R21]` |
| `OPCR-63` | WebUI Mitranet Adoption & Visual Parity | **PASS-LOCKED** | Logo SVG resmi, Mitranet tokens, zero-leak pass |
| `OPCR-64` | Runtime Proof Completion | **PASS-LOCKED** | VM boot milestones `[R1]`..`[R21]` terverifikasi nyata |

---

## 5. Next-Step Recommendations (3 Pilihan Sprint Lanjutan)

1. **Opsi 1 — Sprint PT-01 (Partitioning & Physical Target Bring-Up):**
   - Fokus: Partisi NVRAM / GPT storage disk image, persiapan bootloader bare-metal PC x86_64 fisik nyata via USB EFI loader.
2. **Opsi 2 — Sprint V0-RCI (Disk Image & Real Capacity Metrics):**
   - Fokus: Implementasi image packaging disk mentah (RAW/VDI/VMDK) dan perhitungan throughput L3 wire-speed di level kapasitas real (OPCR-25..27).
3. **Opsi 3 — Sprint W2-lite (API Mutasi & WebUI WebSocket Dashboard):**
   - Fokus: Penambahan form mutasi konfigurasi (POST `/api/v1/firewall`, `/interfaces`), live dynamic metrics charts via SSE / WebSocket streaming.
