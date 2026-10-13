# OPCR Risk Register & Legacy ASL Blockers Migration

> **Document ID:** RISK-OPCR-01  
> **Release Target:** Code OS Rinjani 1.0.0  
> **Date:** 2026-10-14  
> **Status:** `ACTIVE GOVERNANCE`  
> **Author:** Anti Gravity (Senior AI Architect, Systems Auditor & Lead Mentor)  

---

## 1. Migrasi 4 Temuan Warisan ASL Core Kernel

| Legacy ASL ID | Judul Temuan | Status Migrasi OPCR | Strategi Mitigasi / Implementasi |
| :--- | :--- | :--- | :--- |
| **ALIGN-01** | Unaligned memory access triggers CPU bus faults | **CLOSED-BY-DESIGN** | Ditegakkan compile-time assert `align(64)` pada `DescriptorRing`, serta `align(16)` pada `RxDesc16` dan `align(32)` pada `RxDesc32` (`asl-router-fastpath::ring`). |
| **DMA-ADDR-01** | DMA physical address truncation on >4GB RAM | **CLOSED-BY-DESIGN** | API `asl-router-fastpath` menggunakan abstraksi `DmaAddr` dengan metode `is_zone_dma32()`. Diperkenalkan trait `IommuMapper`. |
| **DMA-CACHE-01** | DMA Cache incoherency on non-coherent arches | **MITIGATED** | Fungsi barrier `dma_cache_clean_inval()` diintegrasikan ke API HAL fastpath, memicu memory barrier pada x86 dan instruksi cache clean pada target non-koheren. |
| **ATOMIC-SPARC-01**| Missing native 64-bit atomic ops on SPARC V8 | **OPEN** | Dicatat sebagai batasan arsitektur Tier 2/4. SPARC V8 akan menggunakan spinlock-emulated 64-bit tick pada Phase 05. |

---

## 2. Temuan Risiko Baru OpenCR (Rinjani 1.0.0)

| OPCR Risk ID | Deskripsi Risiko | Severity | Status | Mitigasi |
| :--- | :--- | :--- | :--- | :--- |
| `OPCR-MEM-01` | Static page allocation capacity limit (256 pages) | MEDIUM | MITIGATED | Alokasi per zona terisolasi; alokator multi-zona dijadwalkan pada Phase 05. |
| `OPCR-IPC-01` | SPSC queue concurrency limit | MEDIUM | MITIGATED | Single-Producer Single-Consumer lock-free ring channel; hardware lock untuk multi-core ingress. |
| `OPCR-SEC-01` | Kebocoran kredensial audit probe eksternal | HIGH | CLOSED-BY-DESIGN | Penegakan zero-secrets policy dan isolasi kredensial hanya pada sesi interaktif. |
| `OPCR-VM-01` | Ketiadaan toolchain limine di Windows native | LOW | CLOSED-BY-TOOLING | Teratasi penuh via `tools/provision-limine.ps1` dan `scripts/build-iso.ps1` (xorriso + Limine v8.4.1 bios-install). |
| `OPCR-BIN-01`| Bare-metal ELF output linking missing linker script | HIGH | CLOSED-BY-DESIGN | Teratasi via higher-half `linker.ld`, entrypoint `_start`, 512KB stack, dan verifikasi header ELF `7F 45 4C 46`. |
| `OPCI-NET-01`| Fragment handling minimal (drop fragmented IPv4) | MEDIUM | MITIGATED | Frame terfragmentasi di-drop secara jujur pada parser; reassembly dijadwalkan pada Sprint NET-02. |
| `OPCI-NET-02`| Polling mode CPU-bound execution (100% core load) | LOW | ACCEPTED-FOR-SPRINT | Polling loop dipilih untuk bukti awal fungsional; interrupt APIC / MSI-X dijadwalkan pada NET-02. |
