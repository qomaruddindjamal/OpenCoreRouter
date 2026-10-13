# Open Core Router (OpenCR) — Code OS Rinjani 1.0.3

> **Status:** `TEST VERIFIED` & `BUILD VERIFIED`  
> **Kernel Base:** ASL Core Kernel (`#![no_std]` Bare-Metal Rust)  
> **Safety Standards:** DO-178C DAL-A / ARINC 653 IMA  
> **Target:** Tier 1 Host MMU + Tier 5 Carrier / Network ASICs  

---

## 1. Executive Summary

Open Core Router (OpenCR) adalah sistem operasi router carrier-grade berbasis microkernel modular adaptasi dari ASL Core Kernel. Sistem operasi ini mengusung codename **Rinjani 1.0.3**, mempertahankan prinsip isolasi partisi ruang dan waktu (*spatial & temporal partitioning*) ARINC 653 IMA, dan dioptimalkan untuk switching berkecepatan tinggi, TCAM, GPON/OLT carrier, dan zero-copy AFDX IPC.

---

## 2. Struktur Repositori

```
C:\OpenCR
├── README.md              # Entry point dokumentasi & status evidence-first
├── AGENTS.md              # Kontrak tata kelola AI asisten (Kanoman)
├── GEMINI.md              # Kontrak AI mentor (Gemini / Anti Gravity)
├── Cargo.toml             # Workspace root 15 crates
├── .gitignore             # Standar sanitasi repository
├── opcr-kernel/           # Core kernel executive & hal
│   ├── opcr-core          # Kernel executive & partition descriptor
│   ├── opcr-hal           # CPU contract & MMIO volatile register
│   ├── opcr-sched         # Preemptive fixed-priority scheduler
│   ├── opcr-memory        # Static page allocator (ZoneDMA32)
│   ├── opcr-afdx-ipc      # SPSC lock-free zero-copy ring buffer
│   ├── opcr-bootloader    # Boot info handshake & validation
│   └── opcr-kernel-bin    # Top-level integration & kernel entry
├── opcr-modules/          # Subsistem jaringan carrier-grade
│   ├── fastpath           # Wire-speed packet processing
│   ├── tcam               # Ternary CAM classification
│   ├── gpon-ont           # Optical Network Terminal OMCI state machine
│   ├── olt-carrier        # Optical Line Terminal DBA scheduler
│   ├── qos                # Deterministic traffic shaper & queuing
│   ├── switching          # Layer 2/3 hardware bridging logic
│   ├── security-vault     # Constant-time crypto & key isolation
│   └── undi-npu           # Neural network & co-processor offload
├── docs/                  # Dokumentasi canonical, rnd, roadmap, release
├── audit/opcr-kernel/     # Laporan temuan audit keamanan
├── Analysis/              # Isolasi laporan, karantina, & firmware dump
└── tools/                 # Tool verifikasi & skrip sanitasi
```

---

## 3. Status Verifikasi

| Komponen | Status Epistemic | Bukti Artefak |
| :--- | :--- | :--- |
| **Workspace Unit Tests** | `TEST VERIFIED` | 15/15 tests PASS (`cargo test --workspace`) |
| **Bare-Metal Compilation**| `BUILD VERIFIED`| `cargo check --target x86_64-unknown-none` PASS |
| **Mitranet Audit Boundary**| `VERIFIED` | TCP Port 22 reachable, probe strictly READ-ONLY |
| **Zero Secrets Policy** | `VERIFIED` | Zero credentials committed |
