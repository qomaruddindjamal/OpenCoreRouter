# AGENTS.md — Open Core Router / Code OS Rinjani 1.0.3

## 1. Identitas & Misi
- **Project:** Open Core Router (OpenCR) — Powered by ASL Core Kernel v1.0.0
- **Code OS:** Rinjani 1.0.3
- **Kernel Base:** ASL Core Kernel (bare-metal Rust `#![no_std]`, v1.0.0)
- **Standar:** DO-178C DAL-A / ARINC 653 IMA
- **Asisten AI:** Kanoman AI

## 2. NAMING & FOUNDATION POLICY (IMMUTABLE)
1. **Fondasi Abadi:** Nama "ASL Core Kernel" adalah fondasi mutlak (immutable). Dilarang mengganti atau me-rename fondasi ini.
2. **Versi Terverifikasi:** Versi resmi kernel adalah **v1.0.0** (git tag `v1.0.0` commit `8ff19c3f` & `RELEASE_NOTES.md`). Dilarang mengarang versi lain. Versi crate internal `0.1.0` adalah independen.
3. **Prefix Crate:** Crate fondasi wajib ber-prefix `asl-` di `asl-kernel/`; crate router wajib ber-prefix `asl-router-` di `router/`. Branding OpenCR/Rinjani hanya untuk produk distribusi.
4. **Boot Banner:** Wajib memuat dua baris: `[R1] RINJANI 1.0.3 — Open Core Router` dan `[R2] Powered by ASL Core Kernel v1.0.0`.

## 3. Aturan Operasional Wajib
1. **Root Project Wajib Bersih:** Root hanya memuat file resmi: `README.md`, `AGENTS.md`, `GEMINI.md`, `Cargo.toml`, `.gitignore`, dan subdirektori resmi. Dilarang meninggalkan file scratch atau file liar.
2. **Semua Dokumen Masuk `docs/`:** Dokumentasi teknis wajib disimpan di `docs/CANONICAL/`, `docs/RND/`, `docs/ROADMAP/`, atau `docs/RELEASE/`.
3. **Semua Skrip Masuk `tools/`:** Skrip verifikasi dan otomatisasi masuk ke `tools/` (ter-ignore di git).
4. **Baca Dokumen CANONICAL Terlebih Dahulu:** Sebelum mengedit atau menulis kode, asisten AI wajib membaca `docs/CANONICAL/CANONICAL_MASTER.md` dan `docs/CANONICAL/NAMING_POLICY.md`.
5. **Zero Secrets in Repository:** Dilarang keras meng-commit API key, token GitHub, private key, IP internal riil, atau password.
6. **Audit Telemetry Gateway Mutlak Read-Only:** Segala probe telemetri ke server audit eksternal hanya bersifat membaca. Dilarang menulis, merestart service, atau mereboot.
7. **Jujur dan Evidence-First:** Dilarang membuat fake-cancel token (`[Cancelled by user]`). Laporkan error apa adanya dengan exit code riil.
8. **Commit Hanya Setelah Verifikasi:** Commit hanya diizinkan bila `cargo test --workspace` dan `cargo check --target x86_64-unknown-none` sukses.

## 4. Taksonomi Bukti (Epistemic Taxonomy)
Setiap klaim wajib diberi label:
- `VERIFIED` — diverifikasi langsung via eksekusi aktif
- `DOCUMENTED` — tercatat di dokumen atau git history
- `INFERRED` — hasil deduksi logis arsitektural
- `UNKNOWN` — belum diperiksa
- `BLOCKED` — terhalang dependensi atau kebijakan
- `NOT RUN` — direncanakan tetapi belum dijalankan