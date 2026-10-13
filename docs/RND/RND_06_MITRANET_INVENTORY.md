# R&D Report 06: Mitranet Field Telemetry & Service Inventory

> **Document ID:** RND-OPCR-06  
> **Target Host:** Mitranet Gateway (`10.10.66.228`)  
> **Audit Mode:** STRICTLY READ-ONLY (Whitelist Only)  
> **Audit Timestamp:** 2026-10-14 00:43:00 UTC+7  
> **Status:** `VERIFIED`  
> **Auditor:** Anti Gravity (Senior AI Architect, Systems Auditor & Lead Mentor)  

---

## 1. Executive Summary & Audit Baseline
Audit telemetri lapangan ini dilakukan terhadap server Mitranet (`10.10.66.228`) dalam mode **read-only absolut** tanpa menjalankan perintah modifikasi, instalasi, atau restart layanan. Seluruh perintah mematuhi whitelist yang ditetapkan di Task 1.

---

## 2. Telemetri Lapangan (Redacted Field Snapshot)

### A. Identitas Sistem & Kernel
- **Kernel:** `Linux mitranet 6.12.107+deb13-amd64 #1 SMP PREEMPT_DYNAMIC Debian 6.12.107-1 (2026-08-29) x86_64`
- **Distribusi:** Debian GNU/Linux 13 (trixie), Version 13.7
- **CPU Cores:** 4 cores (`nproc: 4`)
- **Uptime:** 4 days, 5 min, load average: 0.03, 0.06, 0.08
- **Memori:** Total 3851 MB (RAM), Digunakan 498 MB, Bebas 2891 MB, Buff/Cache 702 MB
- **Penyimpanan:** Root `/dev/mmcblk1p2` 53GB (Used: 2.8GB, Avail: 48GB, Use: 6%)

### B. Topologi Interface & Tunneling
Mitranet menjalankan router multi-link dan overlay tunnel:
1. **Physical & Wireless:**
   - `enp1s0`: Physical Ethernet Gigabit (`10.10.66.195/24`)
   - `wlan0`: Wireless Access Point / Local subnet (`10.10.1.254/24`)
2. **Virtual & Overlay Links:**
   - `mac0@enp1s0`: MACVLAN Interface (`10.10.66.209/24`)
   - `wg0`: WireGuard VPN Tunnel (`10.10.77.3/24`)
   - `vxlan100`: Virtual Extensible LAN tunnel (`10.251.1.2/30`)
   - `eoip-vps`: Ethernet over IP MikroTik-compatible tunnel (`10.252.1.2/30`)
   - `ipip-vps`: IP-in-IP encapsulation tunnel (`10.253.1.2/30`)
   - `gre-vps`: Generic Routing Encapsulation tunnel (`10.254.1.2/30`)

### C. Listening Services & Network Ports
- **DNS / DHCP:** `dnsmasq` mendengarkan pada port UDP/TCP 53 di antarmuka local (`127.0.0.1`, `10.10.1.254`) dan DHCP server port 67 (`wlan0`).
- **DHCP Client:** `dhcpcd` mengikat IP dinamis pada multiple MACVLAN (`10.10.66.195`, `10.10.66.209`, dll).
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
| **VXLAN / EoIP / GRE Tunnels** | Header decap/encap pada kecepatan tinggi | `opcr-fastpath` | Pipeline descriptor ring zero-copy untuk parsing tunnel multi-layer tanpa memcpy. |
| **ACL & Port Rules (DNS/WireGuard)** | Klasifikasi 5-tuple (src/dst IP, port, proto) | `opcr-tcam` | RuleKey5T matcher dengan ternary bitmasking untuk drop/forward instan. |
| **WireGuard (UDP 51820)** | Enkripsi simetrik ChaCha20-Poly1305 | `opcr-security-vault` | Offload kunci kriptografi dan isolasi session handshake. |
| **VLAN / MACVLAN (`enp1s0.10`)** | Segmentasi L2 bridge & 802.1Q tagging | `opcr-switching` | Hardware tagging bypass dan isolasi port domain. |
| **Prioritas Bandwidth Multi-Subnet** | QoS traffic policing antar wlan0 dan enp1s0 | `opcr-qos` | Token bucket shaper deterministik ARINC 653 schedule. |
