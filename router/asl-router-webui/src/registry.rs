//! `asl-router-webui::registry` — WebUI Static Asset Registry Table
//!
//! Exposes embedded static assets compiled into the kernel image via `include_bytes!`.
//!
//! Struktur aset bersifat MODULAR: `SHELL_HTML` hanya menyediakan chrome (header,
//! sidebar, footer, tempat mount), sedangkan tiap halaman adalah parsial HTML
//! deklaratif di `/views/*.html` yang diminta router client-side saat modulnya
//! dibuka. Pemecahan ini menjaga setiap berkas jauh di bawah batas buffer
//! respons TCP kernel (gerbang konstan `WEB_RESP_MAX` di `lib.rs`).

use asl_router_webcore::StaticAsset;

pub struct WebUiModule {
    pub name: &'static str,
    pub path: &'static str,
    pub enabled: bool,
}

pub const MODULES: [WebUiModule; 4] = [
    WebUiModule {
        name: "Dashboard",
        path: "/status",
        enabled: true,
    },
    WebUiModule {
        name: "Interfaces",
        path: "/interfaces",
        enabled: true,
    },
    WebUiModule {
        name: "Firewall",
        path: "/firewall",
        enabled: true,
    },
    WebUiModule {
        name: "Services",
        path: "/services",
        enabled: true,
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
    "rinjani-shell-ui01",
    "shell.html"
);
asset!(
    CSS_BASE,
    "/css/base.css",
    "text/css; charset=utf-8",
    "rinjani-css-base-ui01",
    "css/base.css"
);
asset!(
    CSS_LAYOUT,
    "/css/layout.css",
    "text/css; charset=utf-8",
    "rinjani-css-layout-ui01",
    "css/layout.css"
);
asset!(
    CSS_COMPONENTS,
    "/css/components.css",
    "text/css; charset=utf-8",
    "rinjani-css-components-ui01",
    "css/components.css"
);
asset!(
    JS_API,
    "/js/core/api.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-api-ui01",
    "js/core/api.js"
);
asset!(
    JS_UI,
    "/js/core/ui.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-ui-ui01",
    "js/core/ui.js"
);
asset!(
    JS_APP,
    "/js/core/app.js",
    "application/javascript; charset=utf-8",
    "rinjani-js-app-ui01",
    "js/core/app.js"
);
asset!(
    VIEW_SYSTEM,
    "/views/system.html",
    "text/html; charset=utf-8",
    "rinjani-view-system-ui01",
    "views/system.html"
);
asset!(
    VIEW_INTERFACES,
    "/views/interfaces.html",
    "text/html; charset=utf-8",
    "rinjani-view-interfaces-ui01",
    "views/interfaces.html"
);
asset!(
    VIEW_FIREWALL,
    "/views/firewall.html",
    "text/html; charset=utf-8",
    "rinjani-view-firewall-ui01",
    "views/firewall.html"
);
asset!(
    VIEW_SERVICES,
    "/views/services.html",
    "text/html; charset=utf-8",
    "rinjani-view-services-ui01",
    "views/services.html"
);
asset!(
    LOGO_SVG,
    "/images/logo.svg",
    "image/svg+xml; charset=utf-8",
    "rinjani-logo-svg-ui01",
    "images/logo.svg"
);

pub const ALL_ASSETS: [&StaticAsset; 12] = [
    &SHELL_HTML,
    &CSS_BASE,
    &CSS_LAYOUT,
    &CSS_COMPONENTS,
    &JS_API,
    &JS_UI,
    &JS_APP,
    &VIEW_SYSTEM,
    &VIEW_INTERFACES,
    &VIEW_FIREWALL,
    &VIEW_SERVICES,
    &LOGO_SVG,
];
