# CANONICAL MASTER — Open Core Router (OpenCR)

> **Produk Distribusi:** Open Core Router (Codename: Rinjani 1.0.3)  
> **Fondasi Inti:** ASL Core Kernel v1.0.0 (`#![no_std]` Rust)  
> **Standar Keselamatan:** DO-178C DAL-A / ARINC 653 IMA  

---

## 1. Arsitektur Inti
Open Core Router dibangun dengan memisahkan Control Plane dan Data Plane menggunakan prinsip isolasi partisi ARINC 653 di atas fondasi ASL Core Kernel v1.0.0. 

1. **Kernel Base (`asl-kernel/`):**
   - `asl-kernel-core`: Executive pengatur partisi dan state lifecycle.
   - `asl-hal`: Abstraksi instruksi CPU, volatile MMIO, dan memory barriers.
   - `asl-sched`: Preemptive fixed-priority scheduler untuk cyclic execution.
   - `asl-memory`: Alokasi memori fisik statis terikat pada ZoneDMA32.
   - `asl-afdx-ipc`: Komunikasi antar-partisi zero-copy menggunakan lock-free ring buffer.
   - `asl-bootloader`: Validasi parameter boot handoff.
   - `asl-kernel-bin`: Entry point biner bare-metal.

2. **Router Subsystems (`router/`):**
   - `asl-router-fastpath`: Wire-speed packet processing pipeline & descriptor ring.
   - `asl-router-tcam`: Hardware/software ternary content-addressable memory engine.
   - `asl-router-gpon-ont`: Optical network terminal protocol stack.
   - `asl-router-olt-carrier`: Carrier dynamic bandwidth scheduler.
   - `asl-router-qos`: Traffic policing and shaper.
   - `asl-router-switching`: L2 bridging and VLAN tagging.
   - `asl-router-security-vault`: Isolasi kunci kriptografi dan constant-time operations.
   - `asl-router-undi-npu`: Offload akselerator pemrosesan saraf/paket.
