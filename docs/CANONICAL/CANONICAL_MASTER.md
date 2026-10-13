# CANONICAL MASTER — Open Core Router (OpenCR)

> **OS Code:** Rinjani 1.0.3  
> **Kernel:** ASL Core Kernel Derivative (`#![no_std]` Rust)  
> **Safety:** DO-178C DAL-A / ARINC 653 IMA  

---

## 1. Arsitektur Inti
Open Core Router dibangun dengan memisahkan Control Plane dan Data Plane menggunakan prinsip isolasi partisi ARINC 653. 

1. **Kernel Base (`opcr-kernel/`):**
   - `opcr-core`: Executive pengatur partisi dan state lifecycle.
   - `opcr-hal`: Abstraksi instruksi CPU, volatile MMIO, dan memory barriers.
   - `opcr-sched`: Preemptive fixed-priority scheduler untuk cyclic execution.
   - `opcr-memory`: Alokasi memori fisik statis terikat pada ZoneDMA32.
   - `opcr-afdx-ipc`: Komunikasi antar-partisi zero-copy menggunakan lock-free ring buffer.
   - `opcr-bootloader`: Validasi parameter boot handoff.
   - `opcr-kernel-bin`: Entry point biner bare-metal.

2. **Network Modules (`opcr-modules/`):**
   - `fastpath`: Wire-speed packet processing pipeline.
   - `tcam`: Hardware/software ternary content-addressable memory engine.
   - `gpon-ont`: Optical network terminal protocol stack.
   - `olt-carrier`: Carrier dynamic bandwidth scheduler.
   - `qos`: Traffic policing and shaper.
   - `switching`: L2 bridging and VLAN tagging.
   - `security-vault`: Isolasi kunci kriptografi dan constant-time operations.
   - `undi-npu`: Offload akselerator pemrosesan saraf/paket.
