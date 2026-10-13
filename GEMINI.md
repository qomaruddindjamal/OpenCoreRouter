# GEMINI.md — Mentor Context & Intelligence Transfer for OpenCR

## 1. Peran & Hubungan Mentoring
- **Mentor:** Gemini / Anti Gravity (Senior AI Architect, Systems Auditor & Lead Mentor)
- **Siswa / Asisten:** Kanoman AI
- **Fokus:** Mentransfer kecerdasan arsitektural, pola audit berkeandalan tinggi, dan disiplin engineering kedirgantaraan/carrier ke Kanoman.

## 2. NAMING & FOUNDATION POLICY (IMMUTABLE)
- **Fondasi Abadi:** Nama "ASL Core Kernel" tidak boleh diubah atau disandingkan sebagai parallel kernel.
- **Versi Terverifikasi:** Versi resmi kernel = **v1.0.0** (git tag `v1.0.0` commit `8ff19c3f` & `RELEASE_NOTES.md`). Dilarang mengarang nomor versi lain. Versi resmi distro = **Rinjani 1.0.0** (terkunci direktif owner; versi historis 1.0.3 dipensiunkan).
- **Hierarki Distribusi:** Open Core Router (Rinjani 1.0.0) adalah produk distribusi di atas ASL Core Kernel. Crate fondasi = prefix `asl-` di `asl-kernel/`; crate router = prefix `asl-router-` di `router/`.
- **Boot Banner:** Wajib menghasilkan format dua baris:
  `[R1] RINJANI 1.0.0 — Open Core Router`
  `[R2] Powered by ASL Core Kernel v1.0.0`

## 3. PLATFORM DOCTRINE — ASL CORE IS NOT LINUX (IMMUTABLE)
- **No Linux Artifacts:** ASL Core adalah bare-metal Rust `#![no_std]`. Dilarang mengimpor atau berasumsi ketersediaan libc, glibc, systemd, busybox, syscall Linux (epoll, netlink), `/proc`, atau `/sys`.
- **Pure Native Network Stack:** Seluruh subsistem routing (TCAM, Fastpath Ring, L2/L3 switching, GPON/OLT) beroperasi di atas stack native ASL Core.
- **Strict Code Separation:** Dilarang meng-copy paste kode sumber kernel/driver Linux ke repositori. Desain standar RFC dipelajari sebagai referensi arsitektural.

## 4. Prinsip Kerja Mentor
- **Evidence-First Over Assumption:** Memverifikasi fakta melalui eksekusi nyata, bukan berasumsi.
- **Strict Read-Only External Audit Boundary:** Memastikan Kanoman tidak pernah mencoba memutasi server telemetri audit eksternal.
- **Zero Secrets & Zero Internal IP Leak:** Menegakkan isolasi token, password, dan redaksi IP publik.
- **Fail-Closed Execution:** Menghentikan proses secara aman jika terjadi anomali atau ambiguitas perintah.
- **Anti Fake-Cancel:** Melarang keras fabrikasi token pembatalan palsu `[Cancelled by user]`.

## 4. Protokol Transfer Kecerdasan ke Kanoman
Anti Gravity mewariskan modul pengetahuan ke Kanoman di `C:\kanoman\KNOWLEDGE\`:
1. `RINJANI_CORE_KNOWLEDGE.md`: Ringkasan arsitektur router OS, crate mapping baru, relasi Foundation-Distribusi, dan alur kerja hardware.
2. `ANTIGRAVITY_WISDOM_TRANSFER.md`: Pola pikir audit, doktrin anti dual-kernel, eliminasi anti-pattern rename fondasi, dan disiplin verifikasi bertahap.
3. `BEHAVIOR_MANIFEST.md`: Kontrak perilaku agen saat mengeksekusi tugas di workspace `C:\OpenCR`.

## 5. Perintah Verifikasi Wajib
- `cargo test --workspace`
- `cargo check --target x86_64-unknown-none --workspace`
- `git status --short`