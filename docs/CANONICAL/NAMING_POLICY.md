# NAMING & FOUNDATION POLICY — ASL Core Kernel & Open Core Router

> **Status:** RATIFIED & IMMUTABLE  
> **Authority:** Anti Gravity (Senior AI Architect & Mentor) & OpenCR Governance  
> **Date:** 2026-10-14  

---

## 1. Immutabilitas Nama "ASL Core Kernel"
1. Nama **"ASL Core Kernel"** adalah fondasi berkeandalan tinggi (Integrated Modular Avionics, DO-178C DAL-A, ARINC 653) yang bersifat **IMMUTABLE**. Nama ini tidak boleh diubah, diganti, di-rebrand, atau diparalelkan dengan kernel lain dalam konteks apapun.
2. Relasi arsitektural yang sah:
   $$\text{Open Core Router (Codename: Rinjani 1.0.0)} \subset \text{Distribusi Aplikasi di Atas ASL Core Kernel v1.0.0}$$
   Open Core Router adalah sistem operasi / distribusi jaringan router yang berjalan **DI ATAS** fondasi ASL Core Kernel.

---

## 2. Kebijakan Versi Resmi (Evidence-First)
1. **Versi Resmi Kernel:** Versi resmi ASL Core Kernel adalah **v1.0.0** (tercatat secara definitif pada git tag `v1.0.0` commit `8ff19c3f` dan `RELEASE_NOTES.md` tanggal 2026-10-13). Versi ini tidak boleh ditulis berbeda di manapun. Dilarang mengarang nomor versi lain.
2. **Versi Resmi Distro:** Versi resmi distribusi router adalah **Rinjani 1.0.0** (dikunci oleh direktif pemilik proyek). Milestone (VM boot, PC native, Carrier GA) dilacak melalui status Gate (OPCR-01..OPCR-18), bukan dengan menaikkan versi sembarangan. Versi 1.0.3 yang digunakan pada laporan historis R&D awal dicatat sebagai riwayat dan telah dipensiunkan.
3. **Versi Crate Internal:** Crate fondasi versi internal `0.1.0` di `Cargo.toml` adalah independen dan tidak menyatakan versi rilis proyek secara keseluruhan.

---

## 3. Kebijakan Penamaan Crate (Prefix Convention)
1. **Crate Fondasi (Prefix `asl-`):**
   Seluruh modul mikrokernel dasar berlokasi di direktori `asl-kernel/` dan menggunakan prefix `asl-`:
   - `asl-kernel-core`
   - `asl-hal`
   - `asl-memory`
   - `asl-sched`
   - `asl-afdx-ipc`
   - `asl-bootloader`
   - `asl-kernel-bin`
2. **Crate Router / Carrier (Prefix `asl-router-`):**
   Seluruh subsistem jaringan router berlokasi di direktori `router/` dan menggunakan prefix `asl-router-`:
   - `asl-router-fastpath`
   - `asl-router-tcam`
   - `asl-router-gpon-ont`
   - `asl-router-olt-carrier`
   - `asl-router-qos`
   - `asl-router-switching`
   - `asl-router-security-vault`
   - `asl-router-undi-npu`
3. **Aturan Branding Produk:**
   Nama "Open Core Router" dan codename "Rinjani 1.0.0" hanya digunakan untuk produk/distribusi (misalnya README, ISO label, banner terminal) — tidak pernah digunakan sebagai nama crate kernel.

---

## 4. Banner Boot Standar
Seluruh banner boot terminal serial dan konsol wajib mencantumkan dua baris resmi:
```text
[R1] RINJANI 1.0.0 — Open Core Router
[R2] Powered by ASL Core Kernel v1.0.0
```

---

## 5. PLATFORM DOCTRINE — ASL CORE IS NOT LINUX (IMMUTABLE)
1. **Bare-Metal Rust `#![no_std]`:** ASL Core Kernel adalah microkernel avionik murni tanpa ketergantungan Linux. TIDAK ADA kernel Linux, glibc, musl, busybox, systemd, syscall Linux (seperti epoll, netlink, ioctl Linux), `/proc`, `/sys`, atau userland Linux.
2. **Driver Native & Netstack Asli:** Seluruh driver jaringan dan perangkat keras ditulis NATIVE di atas HAL/PMD ASL Core. Protokol jaringan (ARP, IPv4, ICMP, UDP, VXLAN, GRE, WireGuard, dll.) diimplementasikan secara natif di atas netstack ASL — mengejar paritas FUNGSI carrier-grade, bukan hasil porting Debian/Ubuntu/Alpine.
3. **Pemisahan Kode vs Referensi:** Referensi arsitektur dan algoritma standar (RFC, whitepaper, arsitektur Linux/DPDK) boleh DIBACA dan dipelajari, namun kode Linux/GPL TIDAK BOLEH disalin ke dalam repositori.

---

## 6. Melekatnya Gate & Risk Register
Seluruh gate `PASS-LOCKED` dan risk register warisan ASL Core Kernel melekat kuat pada identitas ASL Core Kernel dan tidak dapat dipisahkan.
