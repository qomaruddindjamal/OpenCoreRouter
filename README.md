# Open Core Router

<div align="center">

```
  ___  _ __   ___ _ __    ____  ___  _ __ ___     ____  ___  _   _ _____ _____ ____
 / _ \| '_ \ / _ \ '_ \  / ___|/ _ \| '__/ _ \   |  _ \/ _ \| | | |_   _| ____|  _ \
| (_) | |_) |  __/ | | || |  | (_) | | |  __/   | |_) | | | | |_| | | | |  _| | |_) |
 \___/| .__/ \___|_| |_| \___|  \___/|_|  \___|  |_|/  \___/ \___/  |_| |_____|_|
      |_|
```

**Open Core Router 1.0.1** — Software Router / Network OS berbasis OpenWrt 25.12.5

[![Version](https://img.shields.io/badge/version-1.0.1-cyan?style=flat-square)](https://github.com/qomaruddindjamal/OpenCoreRouter)
[![OpenWrt](https://img.shields.io/badge/OpenWrt-25.12.5-blue?style=flat-square)](https://openwrt.org)
[![Linux](https://img.shields.io/badge/kernel-6.6.x-orange?style=flat-square)](https://kernel.org)
[![License](https://img.shields.io/badge/license-GPL--2.0-green?style=flat-square)](LICENSE)

</div>

---

## 📋 Deskripsi

**Open Core Router (OCR)** adalah Network OS berbasis OpenWrt 25.12.5 yang dirancang untuk router x86-64.  
Menggabungkan kekuatan OpenWrt (procd, UCI, firewall4) dengan antarmuka Web UI modern dan daemon manajemen Go.

### Stack Teknologi

| Komponen | Implementasi |
|---|---|
| **Base OS** | OpenWrt 25.12.5 (Linux 6.6.x, x86-64) |
| **Init** | procd |
| **Config** | UCI (`/etc/config/*`) |
| **Package Mgr** | apk (Alpine Package Keeper) |
| **Firewall** | firewall4 → nftables |
| **DHCP/DNS** | dnsmasq-full |
| **Routing** | FRRouting (zebra, OSPF, BGP, RIP, Static) |
| **VPN** | WireGuard (native kernel), OpenVPN, IPSec |
| **QoS** | sqm-scripts + CAKE algorithm |
| **Web Server** | Lighttpd + PHP-FPM |
| **Daemon** | Go (ocrd) — UCI/ubus/netifd integration |
| **Web UI** | Vanilla JS ES Modules (SPA) |

---

## 🚀 Quick Start

### Build Image (via Docker — works on Windows/macOS/Linux)

```bash
# 1. Clone repository
git clone https://github.com/qomaruddindjamal/OpenCoreRouter.git
cd OpenCoreRouter

# 2. Build (requires Docker Desktop)
make build

# Output:
#   build/output/Open-Core-Router.1.0.1.img
#   build/output/Open-Core-Router.1.0.1.iso
```

### Flash ke USB Drive
```bash
# Linux/macOS
sudo dd if=build/output/Open-Core-Router.1.0.1.img \
         of=/dev/sdX bs=4M status=progress oflag=sync

# Windows (dengan Rufus atau balenaEtcher)
# Gunakan file Open-Core-Router.1.0.1.iso
```

### Boot di QEMU/KVM (Testing)
```bash
# Boot dari IMG
make qemu-img

# Boot dari ISO
make qemu-iso

# Manual QEMU
qemu-system-x86_64 -m 512M -enable-kvm \
  -drive file=build/output/Open-Core-Router.1.0.1.img,format=raw \
  -net nic,model=virtio -net user,hostfwd=tcp::8080-:80
```

---

## 📁 Struktur Proyek

```
OpenCoreRouter/
├── Makefile                        # Master build orchestrator
├── build/
│   ├── overlay/                    # Files yang di-overlay ke rootfs
│   │   └── etc/
│   │       ├── banner              # Login banner
│   │       ├── ocr-release         # OS version info
│   │       ├── config/
│   │       │   ├── network         # UCI network (netifd)
│   │       │   ├── firewall        # UCI firewall (firewall4)
│   │       │   ├── system          # UCI system (hostname, NTP)
│   │       │   ├── dhcp            # UCI DHCP/DNS (dnsmasq-full)
│   │       │   └── ocr             # OCR daemon config
│   │       ├── init.d/
│   │       │   ├── ocrd            # procd init: Go daemon
│   │       │   └── ocr-setup       # First-boot setup
│   │       ├── frr/
│   │       │   ├── daemons         # FRRouting daemon config
│   │       │   └── frr.conf        # FRR base config
│   │       ├── lighttpd/
│   │       │   └── lighttpd.conf   # Web server config
│   │       └── nftables.d/
│   │           └── ocr.nft         # Custom nftables rules
│   └── grub/
│       └── grub.cfg                # GRUB2 boot menu
├── config/
│   └── openwrt_deps.toml           # Referensi dependensi OpenWrt 25.12.5
├── core/                           # C dataplane
│   ├── dataplane/                  # Packet I/O (AF_PACKET)
│   ├── rib/                        # Routing Information Base
│   └── include/                    # Header files
├── daemon/                         # Go management daemon (ocrd)
│   ├── cmd/ocrd/main.go
│   └── internal/
│       ├── api/                    # REST API handlers
│       ├── uci/                    # UCI/ubus wrapper
│       ├── firewall/               # firewall4 integration
│       ├── netlink/                # Netlink interface manager
│       └── services/               # DHCP, DNS, VPN services
├── scripts/
│   ├── build-image.sh              # Main build script
│   ├── Dockerfile.build            # Docker build environment
│   └── docker-entrypoint.sh       # Container entrypoint
└── webui/frontend/                 # Web UI (ES Modules)
    ├── index.html
    ├── css/style.css
    └── js/
        ├── app.js                  # Entry point
        ├── core/                   # api, auth, router, websocket, modal, toast, utils
        ├── components/             # chart (Canvas 2D)
        └── pages/                  # dashboard, interfaces, routing, firewall,
                                    # vpn, dhcp, dns, qos, system
```

---

## 🌐 Web UI

Akses setelah boot: **http://192.168.1.1**

| Halaman | Fitur |
|---|---|
| **Dashboard** | Stats real-time via WebSocket, traffic chart, CPU/RAM |
| **Interfaces** | Toggle UP/DOWN, tambah IP, set MTU, buat VLAN |
| **Routing** | Routing table, FRR protocols, static routes |
| **Firewall** | Filter rules (firewall4/nftables), NAT, apply/flush |
| **VPN** | WireGuard, OpenVPN, IPSec tunnels + peer management |
| **DHCP** | Pools, active leases, static reservations |
| **DNS** | dnsmasq forwarders, host overrides, rebind protection |
| **QoS** | CAKE/FQ-CoDel policies via sqm-scripts |
| **System** | apk packages, logread, reboot/shutdown |

---

## ⚙️ Default Configuration

| Parameter | Value |
|---|---|
| Hostname | `Open-Core-Router` |
| LAN IP | `192.168.1.1/24` |
| DHCP Pool | `192.168.1.100 – 192.168.1.250` |
| WAN | DHCP client (eth1) |
| SSH | Port 22, LAN only |
| Web UI | Port 80 (HTTP) |
| API | `/api/v1` |
| SSH credentials | `root` / *no password* — **ubah segera!** |

---

## 🔧 Referensi OpenWrt 25.12.5

| Item | Detail |
|---|---|
| Image | `openwrt-25.12.5-x86-64-generic-ext4-combined-efi.img` |
| Partition 1 | EFI (16MB) — GRUB2 EFI bootloader |
| Partition 2 | RootFS EXT4 (104MB) |
| Kernel | Linux 6.6.x (LTS) |
| Package Manager | apk (Alpine Package Keeper) |
| Config System | UCI |
| Firewall Backend | nftables (via firewall4) |

---

## 📄 License

GNU General Public License v2.0 — lihat [LICENSE](LICENSE)

---

<div align="center">
Built with ❤️ on top of <a href="https://openwrt.org">OpenWrt 25.12.5</a>
</div>
