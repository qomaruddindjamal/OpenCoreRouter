//! `asl-router-webui::registry` — WebUI Static Asset Registry Table
//!
//! Exposes embedded static assets compiled into the kernel image via `include_bytes!`.
//!
//! Struktur aset bersifat MODULAR: `SHELL_HTML` hanya menyediakan chrome
//! (header, sidebar beserta submenu drop-right, footer, dan tempat mount),
//! sedangkan tiap halaman adalah parsial HTML deklaratif di `/views/*.html`
//! yang diminta router client-side saat modulnya dibuka. Modal pengeditan
//! antarmuka juga aset terpisah (`/views/iface-modal.html`) yang diambil
//! `ifedit.js` saat baris pertama kali diklik. Pemecahan ini menjaga setiap
//! berkas di bawah batas buffer respons TCP kernel (gerbang konstan
//! `WEB_RESP_MAX - HTTP_HEADER_BUDGET` di `lib.rs`), yang diperiksa saat
//! kompilasi.

use asl_router_webcore::StaticAsset;

pub struct WebUiModule {
    pub name: &'static str,
    pub path: &'static str,
    pub enabled: bool,
}

/// Modul yang dikenal WebUI, mengikuti urutan sidebar. Dua puluh tiga entri:
/// tiga belas berdata kernel nyata (`enabled:true`, view-nya membaca endpoint
/// sungguhan) dan sepuluh rupa saja (`enabled:false`) karena kernel belum
/// punya endpoint-nya: WireGuard, VPN, XRay-Core, QoS Manager,
/// DHCP Server, Appearance, Packages, Speed
/// Test, Telnet, dan Trace Route. Restart/Shutdown tidak menjadi modul
/// berhalaman: keduanya aksi daya yang langsung membuka dialog. `/arp`, `/qos`,
/// dan `/gateway` dipasang karena owner menaruh ketiganya di sidebar; empat
/// modul konsol (`/terminal`, `/speedtest`, `/telnet`, `/traceroute`) menyusul
/// karena owner menambahkan menu Terminal dan grup Tools. Hanya `/terminal`
/// yang sudah naik jadi berdata nyata: ia memakai jalur HTTP yang sama dengan
/// modul lain, sementara tiga sisanya butuh pemancar paket yang belum ada.
/// `/usermanager` menyusul jadi berdata nyata pada AUD-02 karena `AuthStore`
/// kini membaca dan menulis akun lewat `/api/v1/auth/users`, `/arp` karena
/// kernel punya `ArpTable` nyata yang dibaca lewat `/api/v1/arp`, dan
/// `/gateway` karena isi FIB kernel kini dilaporkan lewat `/api/v1/routes`.
pub const MODULES: [WebUiModule; 23] = [
    WebUiModule {
        name: "Dashboard",
        path: "/status",
        enabled: true,
    },
    WebUiModule {
        name: "vOlt",
        path: "/olt",
        enabled: true,
    },
    WebUiModule {
        name: "Interfaces",
        path: "/interfaces",
        enabled: true,
    },
    WebUiModule {
        name: "Wireless",
        path: "/wireless",
        enabled: true,
    },
    WebUiModule {
        name: "Firewall",
        path: "/firewall",
        enabled: true,
    },
    WebUiModule {
        name: "NAT",
        path: "/nat",
        enabled: true,
    },
    // Rupa saja: tidak ada mesin protokol maupun endpoint baca di kernel.
    WebUiModule {
        name: "WireGuard",
        path: "/wireguard",
        enabled: false,
    },
    WebUiModule {
        name: "VPN",
        path: "/vpn",
        enabled: false,
    },
    WebUiModule {
        name: "XRay-Core",
        path: "/xray",
        enabled: false,
    },
    WebUiModule {
        name: "QoS Manager",
        path: "/qos",
        enabled: false,
    },
    WebUiModule {
        name: "Services",
        path: "/services",
        enabled: true,
    },
    // Berdata kernel nyata: kernel punya ArpTable yang diisi dari frame ARP
    // yang masuk, dan `GET /api/v1/arp` + `/api/v1/arp/stats` melaporkan isi
    // tabel itu apa adanya (0 baris bila belum ada tetangga yang bicara).
    WebUiModule {
        name: "ARP Table",
        path: "/arp",
        enabled: true,
    },
    WebUiModule {
        name: "Service List",
        path: "/serviceslist",
        enabled: true,
    },
    WebUiModule {
        name: "DNS Server",
        path: "/dnsserver",
        enabled: true,
    },
    // Berdata kernel nyata: FIB kernel nyata (`asl-router-lpm`) kini dibaca lewat
    // `GET /api/v1/routes` + `/api/v1/routes/stats`; halaman ini menampilkan isi
    // tabel rute apa adanya, termasuk 0 baris saat WAN belum beralamat.
    WebUiModule {
        name: "Gateway",
        path: "/gateway",
        enabled: true,
    },
    WebUiModule {
        name: "DHCP Server",
        path: "/dhcpserver",
        enabled: false,
    },
    // Berdata kernel nyata: terminal ini bukan cangkang di dalam kernel, ia CLI
    // di atas HTTP. Setiap perintahnya dipetakan ke endpoint baca yang benar-benar
    // ada di dispatch_get, atau ke dua permukaan tulis kernel, sisanya ditolak
    // dengan sebab teknis (lihat /js/core/termcmd.js).
    WebUiModule {
        name: "Terminal",
        path: "/terminal",
        enabled: true,
    },
    // Menu System: rupa lebih dulu sesuai arahan owner. Appearance memakai
    // mesin tema lokal browser (localStorage), bukan endpoint kernel, sehingga
    // tetap enabled:false.
    WebUiModule {
        name: "Appearance",
        path: "/appearance",
        enabled: false,
    },
    WebUiModule {
        name: "Packages",
        path: "/packages",
        enabled: false,
    },
    // Naik kelas pada AUD-02 P0.1-D: `AuthStore` kernel sudah punya jalur baca
    // dan tulis nyata (`GET`/`POST /api/v1/auth/users`), jadi halaman ini menarik
    // daftar akun sungguhan. Aksi tulis lewat formulir web belum dipasang dan
    // halamannya mengatakan itu apa adanya.
    WebUiModule {
        name: "User Manager",
        path: "/usermanager",
        enabled: true,
    },
    // Grup Tools milik owner: tiga-tiganya butuh jalur kirim paket yang belum
    // ada di kernel (klien TCP, gema ICMP keluar, penghasil beban), sehingga
    // halamannya hanya menampilkan rupa WebFig plus fakta jalurnya.
    WebUiModule {
        name: "Speed Test",
        path: "/speedtest",
        enabled: false,
    },
    WebUiModule {
        name: "Telnet",
        path: "/telnet",
        enabled: false,
    },
    WebUiModule {
        name: "Trace Route",
        path: "/traceroute",
        enabled: false,
    },
];

/// Satu berkas aset yang ter-embed. Label `$etag` adalah identitas revisi aset
/// internal (`-ui01`), BUKAN versi produk: versi resmi tetap Rinjani 1.0.0 dan
/// ASL Core Kernel v1.0.0. Validator yang dikirim ke browser disusun otomatis
/// oleh `asl_router_webcore::serve_static` sebagai `"<label>-<panjang isi>"`,
/// jadi suntingan sekecil apa pun membatalkan cache klien tanpa menaikkan label.
macro_rules! asset {
    ($name:ident, $path:literal, $ctype:literal, $etag:literal, $file:literal) => {
        pub const $name: StaticAsset = StaticAsset {
            path: $path,
            content_type: $ctype,
            etag: $etag,
            content: include_bytes!(concat!("../assets/", $file)),
        };
    };
}

asset!(
    SHELL_HTML,
    "/",
    "text/html; charset=utf-8",
    "rinjani-shell-ui02",
    "shell.html"
);
asset!(
    CSS_BASE,
    "/css/base.css",
    "text/css; charset=utf-8",
    "rinjani-css-base-ui02",
    "css/base.css"
);
asset!(
    CSS_LAYOUT,
    "/css/layout.css",
    "text/css; charset=utf-8",
    "rinjani-css-layout-ui02",
    "css/layout.css"
);
asset!(
    CSS_COMPONENTS,
    "/css/components.css",
    "text/css; charset=utf-8",
    "rinjani-css-components-ui02",
    "css/components.css"
);
asset!(
    CSS_WEBFIG,
    "/css/webfig.css",
    "text/css; charset=utf-8",
    "rinjani-css-webfig-ui02",
    "css/webfig.css"
);
// Chrome modul gaya WebFig (tab modul, toolbar, kolom status, menu kolom).
// Dipisah dari webfig.css karena gerbang respons mengukur tiap aset: webfig.css
// sudah 7 187 B dari jatah 7 936 B.
asset!(
    CSS_TABLE,
    "/css/table.css",
    "text/css; charset=utf-8",
    "rinjani-css-table-ui02g",
    "css/table.css"
);
// Ikon sidebar berupa data-URI `mask-image` di kelas `.nav-ico`: markup shell
// tetap kecil (sprite inline akan melewati gerbang respons) dan warna ikon
// mengikuti `currentColor` sehingga ikut tema terang/gelap/senja.
asset!(
    CSS_ICONS,
    "/css/icons.css",
    "text/css; charset=utf-8",
    "rinjani-css-icons-ui02f",
    "css/icons.css"
);
// Aturan responsif dipisah agar layout.css tetap muat di buffer respons;
// memuat drawer navigasi, scrim, dan target sentuh >= 40 px.
// Ikon aksi chrome modul (New/Edit/Enable/Disable/Remove/Comment, Find,
// Filter, Refresh, menu kolom) sebagai data-URI `mask-image` yang sama
// polanya dengan icons.css. Dipisah karena sepuluh URI SVG ini ~3,8 kB.
asset!(
    CSS_ICONS_ACT,
    "/css/icons-act.css",
    "text/css; charset=utf-8",
    "rinjani-css-icons-act-ui02g",
    "css/icons-act.css"
);
// Aturan responsif dipisah agar layout.css tetap muat di buffer respons;
// memuat drawer navigasi, scrim, dan target sentuh >= 40 px.
asset!(
    CSS_MOBILE,
    "/css/mobile.css",
    "text/css; charset=utf-8",
    "rinjani-css-mobile-ui02f",
    "css/mobile.css"
);
// Gaya jendela terminal (baris tab, header berisi tombol New/Close, layar
// gulung, baris prompt). Dimuat malas oleh app.js hanya saat modul Terminal
// dibuka, karena shell.html sudah memegang 7 882 B dari jatah 7 936 B dan
// tidak boleh ikut menanggung CSS yang jarang dipakai.
asset!(
    CSS_TERMINAL,
    "/css/terminal.css",
    "text/css; charset=utf-8",
    "rinjani-css-terminal-ui02i",
    "css/terminal.css"
);
asset!(
    JS_API,
    "/js/core/api.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-api-ui02",
    "js/core/api.js"
);
asset!(
    JS_UI,
    "/js/core/ui.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-ui-ui02",
    "js/core/ui.js"
);
asset!(
    JS_WEBFIG,
    "/js/core/webfig.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-webfig-ui02",
    "js/core/webfig.js"
);
asset!(
    JS_PLOT,
    "/js/core/plot.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-plot-ui02",
    "js/core/plot.js"
);
// Mesin chrome modul: table.js menyusun tab modul + toolbar (New/Edit/...
// Find/Filter/Refresh), tblcols.js menambahkan pengurutan header, menu kolom,
// dan glif status/ikon tipe pada baris hasil kernel. Keduanya hanya menata apa
// yang sudah dikirim endpoint; dua berkas karena gerbang respons per aset.
asset!(
    JS_TABLE,
    "/js/core/table.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-table-ui02g",
    "js/core/table.js"
);
asset!(
    JS_TBLCOLS,
    "/js/core/tblcols.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-tblcols-ui02g",
    "js/core/tblcols.js"
);
asset!(
    JS_IFEDIT,
    "/js/core/ifedit.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-ifedit-ui02",
    "js/core/ifedit.js"
);
// Modal akun AUD-02: jalur tulis nyata ke POST /api/v1/auth/users. Diminta
// client-side oleh useredit.js, sama seperti pola modal antarmuka.
asset!(
    JS_USEREDIT,
    "/js/core/useredit.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-useredit-aud02",
    "js/core/useredit.js"
);
asset!(
    JS_NAV,
    "/js/core/nav.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-nav-ui02f",
    "js/core/nav.js"
);
asset!(
    JS_LOGIN,
    "/js/core/login.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-login-aud02",
    "js/core/login.js"
);
asset!(
    JS_APP,
    "/js/core/app.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-app-ui02",
    "js/core/app.js"
);
asset!(
    JS_SYSTEM,
    "/js/core/system.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-system-ui02c",
    "js/core/system.js"
);
asset!(
    JS_VIEWS,
    "/js/core/views.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-views-ui02e",
    "js/core/views.js"
);
asset!(
    JS_THEME,
    "/js/core/theme.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-theme-ui02e",
    "js/core/theme.js"
);
// Mesin terminal dipecah dua supaya masing-masing muat di buffer respons:
// termcmd.js memegang isi (tabel perintah, sebab penolakan, pencetak JSON)
// tanpa menyentuh DOM, term.js memegang tampilan (sesi, tab, layar, input).
// Keduanya diminta client-side oleh app.js hanya saat modul Terminal dibuka.
asset!(
    JS_TERMCMD,
    "/js/core/termcmd.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-termcmd-ui02i",
    "js/core/termcmd.js"
);
asset!(
    JS_TERM,
    "/js/core/term.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-term-ui02i",
    "js/core/term.js"
);
asset!(
    VIEW_SYSTEM,
    "/views/system.html",
    "text/html; charset=utf-8",
    "rinjani-view-system-ui02",
    "views/system.html"
);
asset!(
    VIEW_INTERFACES,
    "/views/interfaces.html",
    "text/html; charset=utf-8",
    "rinjani-view-interfaces-ui02",
    "views/interfaces.html"
);
asset!(
    VIEW_FIREWALL,
    "/views/firewall.html",
    "text/html; charset=utf-8",
    "rinjani-view-firewall-ui02",
    "views/firewall.html"
);
asset!(
    VIEW_NAT,
    "/views/nat.html",
    "text/html; charset=utf-8",
    "rinjani-view-nat-ui02",
    "views/nat.html"
);
asset!(
    VIEW_WIRELESS,
    "/views/wireless.html",
    "text/html; charset=utf-8",
    "rinjani-view-wireless-ui02",
    "views/wireless.html"
);
asset!(
    VIEW_OLT,
    "/views/olt.html",
    "text/html; charset=utf-8",
    "rinjani-view-olt-ui02",
    "views/olt.html"
);
asset!(
    VIEW_SERVICES,
    "/views/services.html",
    "text/html; charset=utf-8",
    "rinjani-view-services-ui02",
    "views/services.html"
);
asset!(
    VIEW_ARP,
    "/views/arp.html",
    "text/html; charset=utf-8",
    "rinjani-view-arp-aud02",
    "views/arp.html"
);
asset!(
    VIEW_APPEARANCE,
    "/views/appearance.html",
    "text/html; charset=utf-8",
    "rinjani-view-appearance-ui02c",
    "views/appearance.html"
);
asset!(
    VIEW_WIREGUARD,
    "/views/wireguard.html",
    "text/html; charset=utf-8",
    "rinjani-view-wireguard-ui02e",
    "views/wireguard.html"
);
asset!(
    VIEW_VPN,
    "/views/vpn.html",
    "text/html; charset=utf-8",
    "rinjani-view-vpn-ui02e",
    "views/vpn.html"
);
asset!(
    VIEW_XRAY,
    "/views/xray.html",
    "text/html; charset=utf-8",
    "rinjani-view-xray-ui02e",
    "views/xray.html"
);
asset!(
    VIEW_SERVICESLIST,
    "/views/serviceslist.html",
    "text/html; charset=utf-8",
    "rinjani-view-serviceslist-ui02e",
    "views/serviceslist.html"
);
asset!(
    VIEW_DNSSERVER,
    "/views/dnsserver.html",
    "text/html; charset=utf-8",
    "rinjani-view-dnsserver-ui02e",
    "views/dnsserver.html"
);
asset!(
    VIEW_GATEWAY,
    "/views/gateway.html",
    "text/html; charset=utf-8",
    "rinjani-view-gateway-aud02",
    "views/gateway.html"
);
asset!(
    VIEW_QOS,
    "/views/qos.html",
    "text/html; charset=utf-8",
    "rinjani-view-qos-ui02f",
    "views/qos.html"
);
asset!(
    VIEW_DHCPSERVER,
    "/views/dhcpserver.html",
    "text/html; charset=utf-8",
    "rinjani-view-dhcpserver-ui02e",
    "views/dhcpserver.html"
);
asset!(
    VIEW_PACKAGES,
    "/views/packages.html",
    "text/html; charset=utf-8",
    "rinjani-view-packages-ui02e",
    "views/packages.html"
);
asset!(
    VIEW_USERMANAGER,
    "/views/usermanager.html",
    "text/html; charset=utf-8",
    "rinjani-view-usermanager-aud02",
    "views/usermanager.html"
);
// Grup Tools + Terminal lama milik owner. Tiga sisanya (Speed Test, Telnet,
// Trace Route) masih parsial desain-saja: setiap kontrol yang belum punya jalur
// kernel dipasang `disabled` sehingga halaman tidak pernah tampak seperti sedang
// mengerjakan sesuatu. terminal.html sudah naik tingkat menjadi jendela CLI
// nyata — markup-nya hanya cangkang, isinya datang dari term.js/termcmd.js.
asset!(
    VIEW_TERMINAL,
    "/views/terminal.html",
    "text/html; charset=utf-8",
    "rinjani-view-terminal-ui02i",
    "views/terminal.html"
);
asset!(
    VIEW_SPEEDTEST,
    "/views/speedtest.html",
    "text/html; charset=utf-8",
    "rinjani-view-speedtest-ui02h",
    "views/speedtest.html"
);
asset!(
    VIEW_TELNET,
    "/views/telnet.html",
    "text/html; charset=utf-8",
    "rinjani-view-telnet-ui02h",
    "views/telnet.html"
);
asset!(
    VIEW_TRACEROUTE,
    "/views/traceroute.html",
    "text/html; charset=utf-8",
    "rinjani-view-traceroute-ui02h",
    "views/traceroute.html"
);
// Markup modal edit antarmuka dipisah dari shell.html dan diminta client-side
// oleh ifedit.js saat baris pertama kali diklik.
asset!(
    VIEW_IFACE_MODAL,
    "/views/iface-modal.html",
    "text/html; charset=utf-8",
    "rinjani-view-iface-modal-ui02e",
    "views/iface-modal.html"
);
asset!(
    VIEW_USER_MODAL,
    "/views/user-modal.html",
    "text/html; charset=utf-8",
    "rinjani-view-user-modal-aud02",
    "views/user-modal.html"
);
asset!(
    LOGO_SVG,
    "/images/logo.svg",
    "image/svg+xml; charset=utf-8",
    "rinjani-logo-svg-ui02",
    "images/logo.svg"
);

pub const ALL_ASSETS: [&StaticAsset; 52] = [
    &SHELL_HTML,
    &CSS_BASE,
    &CSS_LAYOUT,
    &CSS_COMPONENTS,
    &CSS_WEBFIG,
    &CSS_TABLE,
    &CSS_ICONS,
    &CSS_ICONS_ACT,
    &CSS_MOBILE,
    &CSS_TERMINAL,
    &JS_API,
    &JS_UI,
    &JS_WEBFIG,
    &JS_TABLE,
    &JS_TBLCOLS,
    &JS_PLOT,
    &JS_IFEDIT,
    &JS_USEREDIT,
    &JS_NAV,
    &JS_LOGIN,
    &JS_APP,
    &JS_SYSTEM,
    &JS_VIEWS,
    &JS_THEME,
    &JS_TERMCMD,
    &JS_TERM,
    &VIEW_SYSTEM,
    &VIEW_INTERFACES,
    &VIEW_FIREWALL,
    &VIEW_NAT,
    &VIEW_WIRELESS,
    &VIEW_OLT,
    &VIEW_SERVICES,
    &VIEW_ARP,
    &VIEW_SERVICESLIST,
    &VIEW_DNSSERVER,
    &VIEW_GATEWAY,
    &VIEW_DHCPSERVER,
    &VIEW_WIREGUARD,
    &VIEW_VPN,
    &VIEW_XRAY,
    &VIEW_QOS,
    &VIEW_APPEARANCE,
    &VIEW_PACKAGES,
    &VIEW_USERMANAGER,
    &VIEW_TERMINAL,
    &VIEW_SPEEDTEST,
    &VIEW_TELNET,
    &VIEW_TRACEROUTE,
    &VIEW_IFACE_MODAL,
    &VIEW_USER_MODAL,
    &LOGO_SVG,
];
