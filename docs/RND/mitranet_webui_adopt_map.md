# MITRANET WEBUI ADOPTION MAP — RINJANI 1.0.0

> **Dokumen:** `docs/RND/mitranet_webui_adopt_map.md`  
> **Status:** RATIFIED BY OWNER (M1 AUDIT VERIFIED)  
> **Server Asal:** `192.0.2.228` (Strictly Read-Only Audit [RFC 5737 Redacted])  
> **Target Subsystem:** `router/asl-router-webui`  
> **Audit Status:** Anti-Leak Audit Passed (Zero Credentials, Zero Private Keys)  
> **Quarantine Hash Verification:**  
> - `Analysis/quarantine_incoming/mitranet-webui/raw/logo.svg`: `2AB938DF...` (UTF-16LE from owner Windows export)  
> - `router/asl-router-webui/assets/images/logo.svg`: `9B14C7E4...` (UTF-8 normalized byte-identical SVG markup)  
> - Visual inspection & SVG element tree: 100% identical polygons, gradient, and wordmark.  

---

## 1. Identifikasi Aset & Desain Asal Mitranet

Server `192.0.2.228` (RFC 5737) mengoperasikan WebUI carrier-grade berbasis:
- **Logo Resmi:** Stylized Rinjani peak / shield icon (`url(#rinjaniGrad)` cyan `#00bcd4` to `#00e5ff`) + Wordmark `MITRANET`.
- **Layout Signature:** Drop-right mikroTik-style sidebar navigation (`sidebar-layout.css`) dengan dark header `#1e222b` dan sidebar `#181b20`.
- **Design Tokens:**
  - Header: `#1e222b` / border `#282d37`
  - Sidebar: `#181b20` / border `#252a35`
  - Accent Color: Cyan `#00bcd4` & Blue `#337ab7` / hover `#4da3ff`
  - Text: `#a4adbd` / subtitle `#6e788b`

---

## 2. Pemetaan Modul (Adopt Map Table)

| Komponen Mitranet | File Asal | Status Adopsi ke Rinjani 1.0.0 | Target Rinjani WebUI | Endpoint API v1 Rinjani |
| :--- | :--- | :--- | :--- | :--- |
| **Logo & Branding** | `/mitranet/web/logo.svg` | **DIADOPSI LENGKAP** | `assets/images/logo.svg` | N/A (Static Asset) |
| **Sidebar & Header Token** | `css/sidebar-layout.css` | **DIADOPSI MODULAR** | `assets/css/layout.css` | N/A (CSS) |
| **Color Scheme & Reset** | `css/mitranet.css` | **DIADOPSI MODULAR** | `assets/css/base.css` | N/A (CSS) |
| **Components & Cards** | `css/mitranet.css` | **DIADOPSI MODULAR** | `assets/css/components.css` | N/A (CSS) |
| **Dashboard / Status** | `index.php` / `status.php` | **DIADOPSI MVP** | `assets/shell.html` + `assets/js/status.js` | `GET /api/v1/system`, `/interfaces` |
| **Interfaces Page** | `status_interfaces.php` | **DIADOPSI** (HTMX target `#main-panel`) | `assets/shell.html` | `GET /api/v1/interfaces` |
| **Firewall Rules** | `firewall_rules.php` | **DIADOPSI** (HTMX target `#main-panel`) | `assets/shell.html` | `GET /api/v1/firewall/rules` |
| **Services Daemon** | `status_services.php` | **DIADOPSI** (HTMX target `#main-panel`) | `assets/shell.html` | `GET /api/v1/services` |
| **PHP Runtime Scripts** | `*.php` (70+ file) | **DITOLAK / TIDAK DIADOPSI** | N/A (ASL Core is NOT Linux; PHP dilarang di bare-metal) | Digantikan Rust `#![no_std]` REST API |
| **Linux-specific tools** | `diag_smart.php`, `pkg_mgr.php` | **DITOLAK** | N/A (Linux package manager/glibc specific) | Native CLI Rinjani |

---

## 3. Rencana Konversi Sesuai Konvensi Owner
1. **Unminified & No Bundling:** Seluruh CSS dan JS di-port tanpa minifikasi dan tanpa node_modules/npm/webpack.
2. **Vendor htmx:** Tetap menggunakan unminified `htmx.js` 1.9.10 di `assets/vendor/htmx.js`.
3. **API Decoupling:** WebUI berkomunikasi secara asinkron murni ke backend REST API via `hx-get="/api/v1/*"`.
