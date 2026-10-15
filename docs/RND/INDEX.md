# R&D DOCUMENTS INDEX (OPEN CORE ROUTER / CODE OS RINJANI)

> **Authority:** Anti Gravity & Kanoman Engineering Team  
> **Repository:** `C:\OpenCR\docs\RND\`  
> **Konvensi Penomoran:** Nomor dokumen RND ditetapkan berurutan sesuai kronologi pembuatan (`RND_XX`). Dilarang memesan nomor di muka.  

---

## 1. Daftar Dokumen R&D Terverifikasi

| Dokumen | Judul / Fokus Riset | Status | Milestone / Sprint Terkait |
| :--- | :--- | :---: | :--- |
| [`RND_01_ASL_CORE_KERNEL_RECLAMATION.md`](file:///c:/OpenCR/docs/RND/RND_01_ASL_CORE_KERNEL_RECLAMATION.md) | Reklamasi Fondasi ASL Core Kernel & DO-178C DAL-A Scaffolding | `DOCUMENTED` | Inisiasi Baseline |
| [`RND_02_CARRIER_NETWORK_STACK_ARCHITECTURE.md`](file:///c:/OpenCR/docs/RND/RND_02_CARRIER_NETWORK_STACK_ARCHITECTURE.md) | Arsitektur Carrier Network Stack & PMD Abstraction | `DOCUMENTED` | Phase-02 |
| [`RND_03_ARINC653_TEMPORAL_PARTITIONING.md`](file:///c:/OpenCR/docs/RND/RND_03_ARINC653_TEMPORAL_PARTITIONING.md) | Partisi Temporal & Scheduler ARINC 653 IMA | `DOCUMENTED` | Phase-03 |
| [`RND_04_FASTPATH_RING_PERFORMANCE.md`](file:///c:/OpenCR/docs/RND/RND_04_FASTPATH_RING_PERFORMANCE.md) | Analisis Kinerja Lock-Free Descriptor Ring | `DOCUMENTED` | Phase-04 |
| [`RND_05_ASL_TO_OPCR_IMPLEMENTATION.md`](file:///c:/OpenCR/docs/RND/RND_05_ASL_TO_OPCR_IMPLEMENTATION.md) | Implementasi Integrasi ASL Core ke Open Core Router | `DOCUMENTED` | OPCR Scaffolding |
| [`RND_06_TCAM_CLASSIFICATION_ENGINE.md`](file:///c:/OpenCR/docs/RND/RND_06_TCAM_CLASSIFICATION_ENGINE.md) | Mesin Klasifikasi Paket TCAM IPv4 5-Tuple | `DOCUMENTED` | OPCR-04 |
| [`RND_07_HARDWARE_INTEGRATION_FEASIBILITY.md`](file:///c:/OpenCR/docs/RND/RND_07_HARDWARE_INTEGRATION_FEASIBILITY.md) | Kelayakan Integrasi Perangkat Keras Carrier | `DOCUMENTED` | OPCR-05 |
| [`RND_08_VM_BOOT_AND_BAREMETAL_STRATEGY.md`](file:///c:/OpenCR/docs/RND/RND_08_VM_BOOT_AND_BAREMETAL_STRATEGY.md) | Strategi Booting Hybrid ISO Limine & VirtualBox | `DOCUMENTED` | Sprint Boot Rinjani |
| [`RND_09_PCI_VIRTIO_NETSTACK_NET01.md`](file:///c:/OpenCR/docs/RND/RND_09_PCI_VIRTIO_NETSTACK_NET01.md) | PCI Scanning, VirtIO-Net Driver & ICMP Ping Proof | `DOCUMENTED` | Sprint NET-01 |
| [`RND_10_GOVERNANCE_NET02_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_10_GOVERNANCE_NET02_REPORT.md) | Lisensi GPL-3.0, Decision Register, HWINV, DHCP & CLI | `VERIFIED` | Sprint GOV-NET02 |
| [`RND_11_ROUTE01_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_11_ROUTE01_REPORT.md) | Static Routing, LPM Trie, NAT/Conntrack & Forwarding | `VERIFIED` | Sprint ROUTE-01 |
| [`RND_12_FWD01_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_12_FWD01_REPORT.md) | Real Multi-Interface Forwarding Proof, NAT & Benchmark | `VERIFIED` | Sprint FWD-01 |
| [`RND_13_EVAL01_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_13_EVAL01_REPORT.md) | smoltcp Evaluation, 2-Sided Smoke Proof & ADR-011 Strategy | `VERIFIED` | Sprint EVAL-01 |
| [`RND_14_NET03_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_14_NET03_REPORT.md) | TCP Facade, API v1, WebUI MVP & Hostname | `VERIFIED` | Sprint NET-03 |
| [`RND_15_DS01_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_15_DS01_REPORT.md) | DHCP Server, DNS Proxy, WebUI Mitranet M0 & Gate Reconciliation | `VERIFIED` | Sprint DS-01 |
| [`RND_16_V0RCI_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_16_V0RCI_REPORT.md) | Disk Image, Persistence, Real Capacity & Mitranet M1 Adoption | `VERIFIED` | Sprint V0-RCI |
| [`RND_17_IS01_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_17_IS01_REPORT.md) | Install to Disk: Boot via ISO & Boot from Installed Disk (VirtualBox) | `VERIFIED` | Sprint IS-01 |
| [`RND_18_IS02_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_18_IS02_REPORT.md) | Block Device Nyata (ATA PIO), Persistensi RJCFG01 Lintas Boot & Gate ARP Host | `VERIFIED` | Sprint IS-02 |
| [`RND_19_IS03_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_19_IS03_REPORT.md) | Self-Installer Dalam Kernel (CLI `install`), Konsol Serial Dua-Arah & Boot dari Disk Hasil Instal | `VERIFIED` | Sprint IS-03 |
| [`RND_20_WAN01_REPORT.md`](file:///c:/OpenCR/docs/RND/RND_20_WAN01_REPORT.md) | WebUI Terpasang & Dilayani dari Alamat WAN (satu NIC bridged), DORA Option 54, Gerbang Ukuran Aset | `VERIFIED` | Sprint WAN-01 |

---

## 2. Topik Terencana (Backlog Riset)
Daftar topik riset terencana di masa mendatang (tanpa nomor pesanan):
- **smoltcp Evaluation & Native TCP Strategy:** Analisis mendalam trade-off antara integrasi smoltcp vs implementasi native TCP state machine berkinerja carrier-grade (RND_12).
- **LPM / FIB Penuh & Skalabilitas 100k Prefix:** Ekspansi trie IPv4 dengan pemadatan DIR-24-8 untuk line-rate routing carrier.
- **Dynamic Routing Protocols:** Implementasi engine OSPFv2 (RFC 2328) dan BGP-4 (RFC 4271).
- **Carrier Tunneling:** EoIP (Ethernet over IP) dan MikroTik Neighbor Discovery Protocol (MNDP) berdasar spesifikasi publik.
- **Network Telemetry & Monitoring:** SNMP Read-Only engine dan flow accounting IPFIX/sFlow.
- **Traffic Engineering & Advanced QoS:** Hierarchical Token Bucket (HTB) dan CoDel fair queueing.
- **DNS Filtering & Transparent Proxy:** Domain classification dan zero-copy packet redirection.
- **Hotspot & Captive Portal:** Layer-2 captive intercept dengan lightweight kernel engine.
- **Cryptographic Package Signing:** Pengesahan image firmware via Ed25519 dan Secure Boot handover.
- **Hypervisor Avionik (Type-1 Microvisor):** Isolasi partisi hardware virtualisasi di atas kernel substrate.
