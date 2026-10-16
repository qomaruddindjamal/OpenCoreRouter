# LAPORAN SPRINT BOOT-01: SPLASH BOOT KERNEL & CHECKLIST [ OK ] TERIKAT PEMERIKSAAN NYATA

> **Document ID:** `docs/RND/RND_21_BOOT01_REPORT.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-16
> **Status:** `RUNTIME VERIFIED` (blok splash dicetak kernel pada UART nyata, bukan simulasi host)
> **Author:** Kanoman AI di bawah supervisi Anti Gravity
> **Baseline Commit:** `9b614b5` (`feat(rinjani-1.0.0): WebUI served from real WAN address on single bridged NIC [wan-01]`)
> **Doktrina:** ASL CORE BUKAN LINUX — splash ditulis dengan `SerialPort` COM1 native di `#![no_std]`; tidak ada `/dev/ttyS*`, tidak ada kode Linux yang diimpor.
> **Redaksi:** seluruh alamat IP internal dan oktet akhir MAC ditulis tersamar (`10.10.MM.NNN`, `08:00:27:XX:XX:XX`) sesuai kebijakan Zero Secrets in Repository.

---

## 1. Ringkasan Eksekutif

Permintaan owner: **perbaiki kernel sampai bisa boot dan menampilkan** blok splash berupa ASCII-art "ASL", salam `Selamat Datang Open Core Router`, tiga baris `[ OK ]` (Kernel initialized / Memory manager initialized / CPU architecture detected) dan penutup `ASL kernel booted successfully.`

Yang dikerjakan dan dibuktikan:

1. **Blok splash tampil utuh dan kontigu pada UART guest (`VERIFIED`).** ASCII-art, versi kernel, salam, tiga `[ OK ]` dan baris penutup kini dicetak dalam satu blok berdampingan — sebelumnya terpotong 14 baris telemetri `[R3]..[R8]`.
2. **Versi tetap `ASL Core Kernel v1.0.0` (`VERIFIED`).** Mock owner menulis `ASL Kernel v1.0.1`. Karena `AGENTS.md` §2 mengunci versi resmi kernel pada v1.0.0 (tag git `v1.0.0`) dan melarang mengarang versi lain, owner memilih **tetap v1.0.0** melalui pertanyaan eksplisit. Baris wajib `[R1]`/`[R2]` tetap di posisi paling atas.
3. **Tidak ada `[ OK ]` buta (`VERIFIED`).** Setiap baris checklist diikat pada pemeriksaan yang benar-benar sudah selesai dieksekusi sebelumnya di jalur boot; bila pemeriksaan gagal, yang dicetak adalah `[FAIL]` dan baris penutup berubah menjadi `BOOT TIDAK LENGKAP — periksa baris [FAIL] di atas`.

## 2. Pemetaan Klaim ke Pemeriksaan Nyata

| Baris splash | Flag | Sumber pemeriksaan (sudah selesai sebelum dicetak) |
|---|---|---|
| `[ OK ] Kernel initialized` | `core_ready = sched_ok && ring_ok && tcam_ok` | `[R4]` scheduler benar-benar memilih task (`scheduler.pick_next_task().is_some()`); `[R5]` ring fastpath lulus push+pop (`ring.push(desc).is_ok() && ring.pop().is_some()`); `[R6]` TCAM mengklasifikasi paket contoh sebagai `RuleAction::Permit`. `kernel_init()` (`[R3]`) tidak memberi flag: bila gagal, kernel sudah halt di spin loop sehingga baris ini tidak mungkin tercetak. |
| `[ OK ] Memory manager initialized` | `mem_ready = hhdm != 0 && eth0.is_some() && eth1.is_some()` | HHDM offset teresolusi dari bootloader, dan dua ring virtio-net teralokasi dari `DmaArena` (alokasi gagal → `[R9] DMA ARENA ALLOC FAILED`). |
| `[ OK ] CPU architecture detected` | `arch_ready = hwinv.cpu.max_standard_leaf > 0 && hwinv.cpu.has_tsc` | `CpuInventory` hasil `CPUID` nyata pada `[R8]` (leaf standar > 0 dan bit TSC terpasang). |

Kode: `asl-kernel/asl-kernel-bin/src/main.rs` — `fn splash_check(label: &str, ok: bool)` (satu baris, `[ OK ]`/`[FAIL]`), `const BOOT_ART: [&str; 5]`, dan blok checklist setelah guard arena DMA.

Senjata 7-bit ASCII: art dipakai bentuk `#`/spasi, bukan glyph box-drawing UTF-8, karena log UART menampilkan byte non-ASCII sebagai mojibake.

## 3. Bukti Runtime

Boot dari disk hasil build (`target/rinjani-1.0.0-x86_64-disk.raw`, SHA256 `c26367c0c4e8905a…be6fe9`), tangkapan serial `Analysis/logs/webui-boot6-console.log` baris 303–325 (disalin apa adanya, hanya spasi lead yang dirapikan):

```
[R1] RINJANI 1.0.0 — Open Core Router
[R2] Powered by ASL Core Kernel v1.0.0
[R3] CORE INIT
[R4] SCHED TICK
[R5] FASTPATH RING SELFTEST PASS
[R6] TCAM SELFTEST PASS
[R7] IDLE LOOP
[R8] PCI ENUM & HWINV: scanning bus 0..3
  -> Devices detected: 10, NICs: 1
  -> Found VirtIO-Net at BDF 0:3
   #   ####  #
  # #  #     #
 #####   ###  #
  # #      #  #
  # #   ####  #####
 ASL Core Kernel v1.0.0
 Selamat Datang Open Core Router

 [ OK ] Kernel initialized
 [ OK ] Memory manager initialized
 [ OK ] CPU architecture detected

 ASL kernel booted successfully.
[R24] queue size device q0=1024 q1=1024
[R9] ETH0 VIRTIO-NET DMA UP mac=08:00:27:XX:XX:XX
```

Boot lengkap sampai jalur layanan juga terekam pada sesi yang sama (`VERIFIED`, bukan hanya splash):

```
[R13] DHCP DISCOVER TERKIRIM ke wire (xid=0x52494E4A, opt12=rinjani, 295 bytes)
[R28] DHCP BOUND eth0=10.10.MM.NNN/24 gw=10.10.MM.1 lease=1800 s — WebUI siap pada alamat WAN
[R18] TCP/HTTP ENGINE NATIF UP port=80 sesi_maks=4
[R28] TCP handshake selesai (ESTABLISHED)
[R28] HTTP /api/v1/services -> 200 (415 bytes dibangun)
```

## 4. Temuan Sampingan: Jalur Power-Off

- `VBoxManage controlvm "Open Core Router" acpipowerbutton` **dua kali** tidak mengubah `VMState` (tetap `"running"`) setelah 30 s — status: `OBSERVED` dengan akar masalah `UNKNOWN` (kemungkinan event ACPI button/SCI tidak dikirim ke guest; belum diinvestigasi).
- CLI serial guest `shutdown` **berhasil** mematikan VM: `power::try_poweroff_candidates()` menulis PM1_CNT nyata, `VMState` menjadi `"poweroff"`. Status: `VERIFIED`.
- Catatan alur kerja: dua proses `tools/serial-console.js` yang berebut satu slot `--uartmode1 tcpserver` menyebabkan churn `host connected/disconnected` 1 detik dan log tanpa output guest. Satu listener per VM.

## 5. Gerbang Verifikasi (Semua Hijau)

| Gerbang | Perintah | Hasil |
|---|---|---|
| Format | `cargo fmt --all` | exit `0` |
| Unit/integration | `cargo test --workspace` | exit `0` — **255 lulus / 0 gagal** |
| Bare-metal | `cargo check --target x86_64-unknown-none -p asl-kernel-bin` | exit `0` (hanya warning lama: `cap`/`idx` tak terbaca, `read_desc`, field `width`) |
| Image | `scripts/build-disk.ps1` | `DISK IMAGE BUILD SUCCESSFUL`, MBR `0x55AA` + PMBR `0xEE` + `EFI PART` di LBA1 |
| Runtime | boot VM headless + tangkapan UART | blok splash kontigu tercetak, lihat §3 |

## 6. Item Terbuka

1. **Mojibake em dash pada `[R1]` (`DOCUMENTED`).** Banner wajib memakai em dash UTF-8; pada log UART tampil sebagai `â`. Teks banner adalah kebijakan `AGENTS.md` §2, jadi perubahannya menunggu keputusan owner.
2. **Splash tidak memuat status jaringan (`BY DESIGN`).** Semua telemetri DHCP/TCP/HTTP tetap di baris `[R9]..[R28]`; checklist hanya untuk tiga pemeriksaan yang selesai sebelum `[R9]`, agar tidak ada `[ OK ]` yang mengklaim hal yang belum terjadi.
3. Backlog sprint sebelumnya tidak berubah (jalur RX `dhcp_server_pool`/`dns_proxy`, egress ICMP echo, uji runtime `wan static` + DAD) — lihat `RND_20_WAN01_REPORT.md` §8.
