# VERSION REGISTER — ASL CORE KERNEL & RINJANI OS

> **Status:** IMMUTABLE / CANONICAL  
> **Authority:** Project Owner Binding Directive  
> **Date:** 2026-10-14  

---

## 1. Official Version Register

| Layer | Name | Official Version | Authority Source | Epistemic Status |
| :--- | :--- | :--- | :--- | :--- |
| **Foundation (Kernel)** | **ASL Core Kernel** | **v1.0.0** | Git tag `v1.0.0` (commit `8ff19c3f`), `RELEASE_NOTES.md` | `VERIFIED` |
| **Distribution (OS)** | **Rinjani (Open Core Router)**| **1.0.0** | Project Owner Lock Directive (Sprint Boot Rinjani) | `VERIFIED` |
| **Internal Workspace Crates**| `asl-*` / `asl-router-*` | **0.1.0** | `Cargo.toml` (Independent crate semver) | `DOCUMENTED` |

---

## 2. Version Lineage & Historical Accounting

1. **ASL Core Kernel v1.0.0:**
   - Ditetapkan secara kanonikal sejak rilis git tag `v1.0.0` commit `8ff19c3f` tanggal 2026-10-13.
   - Merupakan fondasi mutlak (*immutable substrate*) bare-metal `#![no_std]`. Dilarang mengarang nomor versi kernel lain.
2. **Rinjani 1.0.0 (Current Locked Distribution):**
   - Dikunci oleh keputusan mutlak Project Owner pada Sprint Boot Rinjani (2026-10-14).
   - Milestone evolusi sistem (VM boot, PC native bare-metal, Carrier GA) dilacak melalui **GATE STATUS** (e.g. OPCR-01 .. OPCR-18), bukan dengan menaikkan versi distro secara sembarangan.
3. **Catatan Historis Rinjani 1.0.3:**
   - Nomor versi `1.0.3` dipakai pada fase R&D awal (`RND_05` sampai `RND_07`).
   - Sesuai prinsip **Evidence-First & Immutability**, riwayat dokumen historis dan commit log lama **TIDAK DIHAPUS / TIDAK DI-REWRITE**.
   - Nomor `1.0.3` resmi dipensiunkan dan digantikan oleh **Rinjani 1.0.0** melalui direktif kanonikal ini.
