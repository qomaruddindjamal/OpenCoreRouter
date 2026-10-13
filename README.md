# Open Core Router (OpenCR) — Powered by ASL Core Kernel v1.0.0

> **Status:** `TEST VERIFIED` (45/45 Tests PASS) & `BUILD VERIFIED` (bare-metal `x86_64-unknown-none`)  
> **Distribusi:** Open Core Router (Codename: Rinjani 1.0.3)  
> **Fondasi:** ASL Core Kernel (v1.0.0, bare-metal Rust `#![no_std]`)  
> **Standar:** DO-178C DAL-A / ARINC 653 IMA  

---

## 1. Executive Summary

Open Core Router (OpenCR) adalah sistem operasi router carrier-grade yang berjalan di atas fondasi **ASL Core Kernel v1.0.0**. Sistem ini mempertahankan isolasi partisi ARINC 653 IMA secara spasial dan temporal, serta mengintegrasikan data plane berkecepatan tinggi: lock-free descriptor ring (`asl-router-fastpath`), mesin klasifikasi TCAM IPv4 5-tuple (`asl-router-tcam`), stack akses optik (`asl-router-gpon-ont`, `asl-router-olt-carrier`), dan zero-copy AFDX IPC (`asl-afdx-ipc`).

---

## 2. Struktur Repositori Bersih (Foundation Reclamation)

```
C:\OpenCR
├── README.md                      # Entry point: Powered by ASL Core Kernel v1.0.0
├── AGENTS.md                      # Kontrak AI Kanoman & Naming Policy
├── GEMINI.md                      # Kontrak Mentor Anti Gravity
├── Cargo.toml                     # Workspace root 15 crates (asl-* & asl-router-*)
├── .gitignore                     # Sanitasi git (zero clutter, zero secrets)
├── asl-kernel/                    # Crate Fondasi ASL Core Kernel v1.0.0
│   ├── asl-kernel-core            # Kernel executive & partition descriptor
│   ├── asl-hal                    # CPU contract & MMIO volatile register
│   ├── asl-memory                 # Static page allocator & ZoneDMA32
│   ├── asl-sched                  # Preemptive fixed-priority scheduler
│   ├── asl-afdx-ipc               # SPSC lock-free zero-copy ring buffer
│   ├── asl-bootloader             # Boot info validation handshake
│   └── asl-kernel-bin             # Kernel top-level integration binary
├── router/                        # Crate Produk Router (Open Core Router / Rinjani 1.0.3)
│   ├── asl-router-fastpath        # Intel 82599/i40e descriptor ring (ALIGN/DMA safe)
│   ├── asl-router-tcam            # IPv4 5-tuple engine (default-deny)
│   ├── asl-router-gpon-ont        # Optical Network Terminal OMCI state machine
│   ├── asl-router-olt-carrier     # Carrier OLT dynamic bandwidth scheduler
│   ├── asl-router-qos             # Traffic shaper & queuing
│   ├── asl-router-switching       # Layer 2/3 hardware bridging logic
│   ├── asl-router-security-vault  # Constant-time crypto & key isolation
│   └── asl-router-undi-npu        # Neural network & coprocessor offload
├── docs/                          # CANONICAL, RND, ROADMAP, RELEASE
├── audit/opcr-kernel/             # Risk register & audit findings
├── Analysis/                      # Reports, quarantine, firmware rom
└── tools/                         # Skrip otomasi (git-ignored)
```

---

## 3. Banner Boot Resmi

```text
[R1] RINJANI 1.0.3 — Open Core Router
[R2] Powered by ASL Core Kernel v1.0.0
```
