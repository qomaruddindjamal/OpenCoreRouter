# Third-Party Dependencies and Licenses

This document catalogs all external software components and crate dependencies used in Open Core Router / Code OS Rinjani 1.0.0.

## 1. Rust Ecosystem Dependencies

The following third-party Rust crates are utilized in the compilation and verification of the system:

| Dependency | Version / Source | License | Purpose / Role |
| :--- | :--- | :--- | :--- |
| **limine** | `0.4.3` | Dual `BSD-2-Clause` / `MIT` | Limine protocol boot structures and request tags for bare-metal x86_64 handoff |
| **spin** | `0.9.8` | `MIT` | Bare-metal `#![no_std]` spinlock synchronisation primitives for fastpath & rings |
| **bitflags** | `2.4.2` | Dual `MIT` / `Apache-2.0` | Strongly typed bitmask flags for PCI, VirtIO, and packet headers |

All listed dependencies possess permissive licenses that are 100% compatible with inclusion in and linking with GPL-3.0-or-later works.

## 2. Bootloader & Tooling Binaries (External)

| Tool / Binary | Version | License | Note |
| :--- | :--- | :--- | :--- |
| **Limine Bootloader** | `v8.x-binary` | `BSD-2-Clause` | Stage-1/2 bootloader binary (`limine.exe` / `limine-bios-cd.bin`) for El Torito ISO master |
| **xorriso** | `1.5.2` (via Cygwin/MSYS2) | `GPL-3.0-or-later` | ISO-9660 / El Torito mastering utility |

## 3. Documentation

All technical documentation, architecture specifications, and canonical master records in `docs/` are licensed under:
**Creative Commons Attribution 4.0 International (CC-BY-4.0)**.
