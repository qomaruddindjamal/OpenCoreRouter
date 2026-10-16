# AUD-01 PROJECT DISCOVERY — inventaris nyata repositori & WebUI

> **Document ID:** `docs/RND/AUD01_PROJECT_DISCOVERY.md`
> **Release Target:** Code OS Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
> **Date:** 2026-10-17
> **Metode:** pembacaan sumber di working tree `C:\OpenCR` + probing HTTP ke VM dev + DOM inspeksi di peramban. Tidak ada satu pun entri yang dikarang; setiap baris punya sumber `file:line` atau tangkapan runtime.
> **Redaksi:** alamat IP lab dan MAC ditulis tersamar (`10.10.MM.NNN`, `08:00:27:XX:XX:XX`).

## 1. Modul crate

| Wilayah | Crate | Peran terverifikasi |
|---|---|---|
| Fondasi (`asl-kernel/`) | `asl-kernel-bin` | satu-satunya binari `no_std` yang di-boot Limine; memegang poll loop, CLI serial, `web_runtime!` |
| Fondasi | `asl-kernel-core`, `asl-hal`, `asl-memory`, `asl-bootloader`, `asl-afdx-ipc` | scheduler/APIC/HAL/memmgr/IPC sesuai CANONICAL |
| Router (`router/`) | `asl-router-webcore` | parser & perakit HTTP `no_std`, validator ETag berbasis isi, `response_status()` |
| Router | `asl-router-api` | 12 jalur GET + 2 jalur POST, JSON writer terbatas, `WebRuntime` sebagai satu-satunya sumber fakta |
| Router | `asl-router-webui` | registry 49 aset ter-embed + dispatcher + tes penjaga gerbang ukuran |
| Router | `asl-router-netstack` | ARP/IPv4/ICMP-echo-reply/UDP/DHCP-client + TCP state machine native |
| Router | `asl-router-tcpstub`, `asl-router-fastpath`, `asl-router-tcam`, `asl-router-nat`, `asl-router-lpm` | mesin yang **ada dan diuji unit**, tetapi sebagian belum ditanya pada frame yang lewat || Router | `asl-router-drivers-virtio`, `-ata`, `-virtio-blk`, `-pci`, `-hwinv`, `-undi-npu` | DMA net nyata; ATA PIO; enumerasi PCI |
| Router | `asl-router-persistence`, `-gpt`, `-fat32`, `-installer` | jurnal RJCFG01 + installer di-dalam-kernel |
| Router | `asl-router-gpon-ont`, `-olt-carrier`, `-wireless*`, `-qos`, `-security-vault`, `-switching` | **marker crate**: ada paketnya, tidak ada jalur datanya |

## 2. Kontrak HTTP yang benar-benar ada

GET (12): `/api/v1/system`, `/api/v1/interfaces`, `/api/v1/firewall/rules`, `/api/v1/nat`, `/api/v1/nat/rules`, `/api/v1/nat/conntrack`, `/api/v1/wireless`, `/api/v1/wireless/devices`, `/api/v1/olt`, `/api/v1/olt/ons`, `/api/v1/services`, `/api/v1/writes/last`.

POST (2): `/api/v1/power` (aksi `arm|restart|shutdown`, wajib arm dulu, jendela 20 s) dan `/api/v1/interfaces/<nama>` (mode dhcp/static, validasi kernel + persistensi RJCFG01).

Aset statis: 49 berkas (`shell.html`, 8 CSS, 1 SVG, 14 JS inti, 25 parsial `views/*.html`), masing-masing ber-ETag `"<label>-<panjang byte>"` sehingga validator berubah bila isi berubah. Gerbang ukuran per aset = `WEB_RESP_MAX 8192 − HTTP_HEADER_BUDGET 256` = **7 936 B**.

## 3. Inventaris navigasi (kesamaan dengan mandat §3)

28 item sidebar terverifikasi dari DOM (`document.querySelectorAll('.sidebar [data-view], .sidebar [data-power]')`): **26 `data-view`** + **2 `data-power`** (Restart, Shutdown — bukan halaman). Empat induk grup (`Firewall`, `Services`, `System`, `Tools`) adalah anchor `href="#"` yang juga membawa `data-view` alias ke anak pertamanya.

Klasifikasi modul: **9 modul membaca endpoint kernel secara pasif** (Dashboard, vOlt, Interfaces, Wireless, Firewall, NAT, Services, Service List, DNS Server), **1 modul pemanggil aktif** (Terminal: 12 jalur baca + 2 jalur tulis atas permintaan), **13 modul rupa-saja** (WireGuard, VPN, XRay-Core, QoS Manager, ARP Table, Gateway, DHCP Server, Appearance, Packages, User Manager, Speed Test, Telnet, Trace Route) yang semuanya menampilkan "Modul belum aktif di build ini" dan tidak pernah menampilkan angka karangan.

## 4. Kapabilitas kernel (kebenaran lapangan, bukan aspirasi)

| Kemampuan | Status | Sumber |
|---|---|---|
| Virtio-net DMA RX/TX | ADA, terbukti | `[R9] ETH0 VIRTIO-NET DMA UP` di konsol boot |
| ARP responder + gratuitous ARP | ADA | `asl-router-netstack` `build_arp_reply` / `build_arp_gratuitous` |
| ICMP echo **responder** | ADA | balasan gema untuk permintaan masuk |
| DHCP **client** DORA | ADA, terbukti | `[R28] DHCP BOUND eth0=10.10.MM.NNN/24` |
| WAN statik + DAD + RJCFG01 persist/rollback | ADA, terbukti | sprint WAN-01/IS-02 |
| TCP + HTTP + WebUI + API | ADA, terbukti | sesi ini, alamat WAN bridged |
| TCAM classifier | MESIN ADA, **tidak ditanya pada frame** | `firewall_wired:false` (baru, AUD-01) |
| NAT engine | MESIN ADA, `nat_forwarding_wired:false` | `/api/v1/nat` `forwarding:"engine-only"` |
| DHCP server pool | MESIN ADA, tidak ada jalur RX | `[R20]` saat boot |
| DNS proxy | MESIN ADA, tidak melayani | `[R21]` saat boot |
| LPM FIB | MESIN ADA, `route_packet` tidak dipanggil | `[R15]` saat boot |
| Tabel ARP (cache tetangga) | **TIDAK ADA** | hanya penjawab |
| 802.1Q/VLAN, VXLAN, GRE, EoIP, VRRP, MACsec, MACVLAN, bonding | **TIDAK ADA** | tab Interfaces adalah rupa |
| ICMP echo **keluar**, traceroute | **TIDAK ADA** | tidak ada pemancar/TTL decrement |
| WireGuard / kripto / XRay | **TIDAK ADA** | crate marker |
| QoS shaper, packet capture, wireless, GPON/OLT nyata | **TIDAK ADA** | crate marker |
| Autentikasi/otorisasi WebUI | **TIDAK ADA** | hanya pemeriksaan sama-origin pada POST |
