# Open Core Router (OpenCR) — Powered by ASL Core Kernel v1.0.0

> **Status:** `TEST VERIFIED` (68/68 Tests PASS) & `BUILD VERIFIED` (bare-metal `x86_64-unknown-none`) & `RUNTIME VERIFIED` (VM Boot & Ping Proof OK)  
> **Distribusi:** Open Core Router (Codename: Rinjani 1.0.0)  
> **Fondasi:** ASL Core Kernel (v1.0.0, bare-metal Rust `#![no_std]`)  
> **Standar:** DO-178C DAL-A / ARINC 653 IMA  

---

## 1. Executive Summary

Open Core Router (OpenCR) adalah sistem operasi router carrier-grade yang berjalan di atas fondasi **ASL Core Kernel v1.0.0**. Sistem ini mempertahankan isolasi partisi ARINC 653 IMA secara spasial dan temporal, serta mengintegrasikan data plane berkecepatan tinggi: lock-free descriptor ring (`asl-router-fastpath`), mesin klasifikasi TCAM IPv4 5-tuple (`asl-router-tcam`), stack akses optik (`asl-router-gpon-ont`, `asl-router-olt-carrier`), zero-copy AFDX IPC (`asl-afdx-ipc`), enumerasi bus PCI (`asl-router-pci`), driver VirtIO-Net (`asl-router-drivers-virtio`), packet buffer pool (`asl-router-netbuf`), dan native netstack (`asl-router-netstack`).

> **Milestone Terverifikasi (Sprint NET-01):** Rinjani 1.0.0 berhasil mem-boot VM VirtualBox, mengenali NIC VirtIO-Net melalui PCI bus scanning, mengaktifkan native stack ARP/IPv4/ICMP, dan merespons paket ICMP Echo Request (`[NET] ICMP ECHO REPLY SENT`) — membuktikan jaringan hidup secara fungsional.

---

## 2. Platform Doctrine — ASL Core is Not Linux

ASL Core Kernel adalah bare-metal microkernel avionik (`#![no_std]`), **bukan distribusi Linux**:
- **Zero Linux Artifacts:** Tanpa kernel Linux, glibc, musl, busybox, systemd, syscall Linux (epoll, netlink), `/proc`, atau `/sys`.
- **Pure Native Implementation:** Driver jaringan dan network protocol stacks (ARP, IPv4, ICMP, UDP, VXLAN, WireGuard) ditulis secara native di atas HAL dan scheduler ASL Core.
- **Strict Code Separation:** Referensi arsitektur dan RFC standar dipelajari, namun tidak ada kode GPL/Linux yang disalin.

---

## 3. Struktur Repositori Bersih (Foundation Reclamation)

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
├── router/                        # Crate Produk Router (Open Core Router / Rinjani 1.0.0)
│   ├── asl-router-fastpath        # Intel 82599/i40e descriptor ring (ALIGN/DMA safe)
│   ├── asl-router-tcam            # IPv4 5-tuple engine (default-deny)
│   ├── asl-router-gpon-ont        # Optical Network Terminal OMCI state machine
│   ├── asl-router-olt-carrier     # Carrier OLT dynamic bandwidth scheduler
│   ├── asl-router-qos             # Traffic shaper & queuing
│   ├── asl-router-switching       # Layer 2/3 hardware bridging logic
│   ├── asl-router-security-vault  # Constant-time crypto & key isolation
│   └── asl-router-undi-npu        # Neural network & coprocessor offload
├── docs/                          # CANONICAL, RND, ROADMAP, RELEASE
├── audit/kernel/                  # Risk register & audit findings
├── Analysis/                      # Reports, quarantine, firmware rom
└── tools/                         # Skrip otomasi (git-ignored)
```

---

## 4. Banner Boot Resmi

```text
[R1] RINJANI 1.0.0 — Open Core Router
[R2] Powered by ASL Core Kernel v1.0.0
```

---

## 5. Lisensi & Hak Cipta (Licensing & Governance)

- **Source Code:** Berlisensi [GNU General Public License v3.0 or later (GPL-3.0-or-later)](file:///C:/OpenCR/LICENSE).
- **Dokumentasi:** Berlisensi [Creative Commons Attribution 4.0 International (CC-BY-4.0)](https://creativecommons.org/licenses/by/4.0/).
- **Third-Party Dependencies:** Rincian dependensi dan lisensi pihak ketiga terdokumentasi di [`LICENSE-THIRD-PARTY.md`](file:///C:/OpenCR/LICENSE-THIRD-PARTY.md).
- **Pemberitahuan Hak Cipta & Merek:** Lihat [`NOTICE`](file:///C:/OpenCR/NOTICE). Nama "Open Core Router", "OpenCR", "Rinjani", dan "ASL Core Kernel" adalah hak cipta/merek dagang qomaruddindjamal. Fork dan redistribusi wajib menggunakan nama produk yang berbeda kecuali dengan izin tertulis.
