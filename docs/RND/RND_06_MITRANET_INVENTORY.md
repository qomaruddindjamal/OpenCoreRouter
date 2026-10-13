# R&D Report 06: Mitranet Field Telemetry & Service Inventory

> **DATA TER-REDAKSI UNTUK PUBLIKASI — topologi riil hanya di salinan internal**  
> **Document ID:** RND-OPCR-06  
> **Target Host:** Telemetry Gateway (Redacted: `192.0.2.1` / RFC 5737 TEST-NET-1)  
> **Audit Mode:** STRICTLY READ-ONLY (Whitelist Only)  
> **Audit Timestamp:** 2026-10-14 00:43:00 UTC+7  
> **Status:** `VERIFIED`  
> **Auditor:** Anti Gravity (Senior AI Architect, Systems Auditor & Lead Mentor)  

---

## 1. Tabel Korespondensi Redaksi Keamanan (RFC 5737 Compliance)

| Entitas Lapangan | Alamat Asli (Internal) | Alamat Teredaksi Publik (RFC 5737) | Keterangan Standar |
| :--- | :--- | :--- | :--- |
| Gateway Host IP | Internal Subnet 1 | `192.0.2.1` | TEST-NET-1 Documentation Range |
| Management Subnet | Internal Subnet 1 | `192.0.2.0/24` | TEST-NET-1 Documentation Range |
| Local Wireless Subnet | Internal Subnet 2 | `198.51.100.0/24` | TEST-NET-2 Documentation Range |
| WireGuard VPN Subnet | Internal Subnet 3 | `203.0.113.0/24` | TEST-NET-3 Documentation Range |
| Overlay Point-to-Point | Internal Subnet 4 | `192.0.2.240/30` | TEST-NET-1 Documentation Range |

---

## 2. Telemetri Lapangan (Redacted Field Snapshot)

### A. Identitas Sistem & Kernel
- **Kernel:** `Linux router-gw 6.12.107+deb13-amd64 #1 SMP PREEMPT_DYNAMIC Debian 6.12.107-1 (2026-08-29) x86_64`
- **Distribusi:** Debian GNU/Linux 13 (trixie), Version 13.7
- **CPU Cores:** 4 cores (`nproc: 4`)
- **Uptime:** 4 days, 5 min, load average: 0.03, 0.06, 0.08
- **Memori:** Total 3851 MB (RAM), Digunakan 498 MB, Bebas 2891 MB, Buff/Cache 702 MB
- **Penyimpanan:** Root `/dev/mmcblk1p2` 53GB (Used: 2.8GB, Avail: 48GB, Use: 6%)

### B. Topologi Interface & Tunneling (Teredaksi)
Gateway menjalankan router multi-link dan overlay tunnel:
1. **Physical & Wireless:**
   - `enp1s0`: Physical Ethernet Gigabit (`192.0.2.10/24`)
   - `wlan0`: Wireless Access Point / Local subnet (`198.51.100.1/24`)
2. **Virtual & Overlay Links:**
   - `mac0@enp1s0`: MACVLAN Interface (`192.0.2.20/24`)
   - `wg0`: WireGuard VPN Tunnel (`203.0.113.3/24`)
   - `vxlan100`: Virtual Extensible LAN tunnel (`192.0.2.242/30`)
   - `eoip-vps`: Ethernet over IP MikroTik-compatible tunnel (`192.0.2.246/30`)
   - `ipip-vps`: IP-in-IP encapsulation tunnel (`192.0.2.250/30`)
   - `gre-vps`: Generic Routing Encapsulation tunnel (`192.0.2.254/30`)

### C. Listening Services & Network Ports
- **DNS / DHCP:** `dnsmasq` mendengarkan pada port UDP/TCP 53 di antarmuka local (`127.0.0.1`, `198.51.100.1`) dan DHCP server port 67 (`wlan0`).
- **DHCP Client:** `dhcpcd` mengikat IP dinamis pada multiple MACVLAN interface.
- **VPN / Overlay Ports:**
  - UDP `51820`: WireGuard daemon.
  - UDP `4789`: VXLAN standard overlay port.
- **Management & Control:**
  - TCP `22`: OpenSSH Server (`sshd`).
  - TCP `8443`: Python-based Management Web Gateway.
  - TCP `8000`: Local PHP Web Panel Backend (`127.0.0.1`).

---

## 3. Analisis Relevansi Lapangan ke Modul Open Core Router (Rinjani 1.0.3)

| Layanan / Interface Lapangan | Karakteristik Beban | Modul OpenCR Target | Rencana Adaptasi Rinjani 1.0.3 |
| :--- | :--- | :--- | :--- |
| **VXLAN / EoIP / GRE Tunnels** | Header decap/encap pada kecepatan tinggi | `asl-router-fastpath` | Pipeline descriptor ring zero-copy untuk parsing tunnel multi-layer tanpa memcpy. |
| **ACL & Port Rules (DNS/WireGuard)** | Klasifikasi 5-tuple (src/dst IP, port, proto) | `asl-router-tcam` | RuleKey5T matcher dengan ternary bitmasking untuk drop/forward instan. |
| **WireGuard (UDP 51820)** | Enkripsi simetrik ChaCha20-Poly1305 | `asl-router-security-vault` | Offload kunci kriptografi dan isolasi session handshake. |
| **VLAN / MACVLAN (`enp1s0.10`)** | Segmentasi L2 bridge & 802.1Q tagging | `asl-router-switching` | Hardware tagging bypass dan isolasi port domain. |
| **Prioritas Bandwidth Multi-Subnet** | QoS traffic policing antar wlan0 dan enp1s0 | `asl-router-qos` | Token bucket shaper deterministik ARINC 653 schedule. |
