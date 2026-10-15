//! `asl-router-webui::registry` — WebUI Static Asset Registry Table
//!
//! Exposes embedded static assets compiled into the kernel image via `include_str!` / `include_bytes!`.

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

pub const SHELL_HTML: StaticAsset = StaticAsset {
    path: "/",
    content_type: "text/html; charset=utf-8",
    etag: "rinjani-shell-v102",
    content: include_bytes!("../assets/shell.html"),
};

pub const CSS_BASE: StaticAsset = StaticAsset {
    path: "/css/base.css",
    content_type: "text/css; charset=utf-8",
    etag: "rinjani-css-base-v100",
    content: include_bytes!("../assets/css/base.css"),
};

pub const CSS_LAYOUT: StaticAsset = StaticAsset {
    path: "/css/layout.css",
    content_type: "text/css; charset=utf-8",
    etag: "rinjani-css-layout-v100",
    content: include_bytes!("../assets/css/layout.css"),
};

pub const CSS_COMPONENTS: StaticAsset = StaticAsset {
    path: "/css/components.css",
    content_type: "text/css; charset=utf-8",
    etag: "rinjani-css-components-v100",
    content: include_bytes!("../assets/css/components.css"),
};

pub const JS_COMMON_OPCR: StaticAsset = StaticAsset {
    path: "/js/common/opcr.js",
    content_type: "application/javascript; charset=utf-8",
    etag: "rinjani-js-opcr-v100",
    content: include_bytes!("../assets/js/common/opcr.js"),
};

pub const JS_COMMON_HTMX: StaticAsset = StaticAsset {
    path: "/js/common/htmx-helpers.js",
    content_type: "application/javascript; charset=utf-8",
    etag: "rinjani-js-htmx-helpers-v100",
    content: include_bytes!("../assets/js/common/htmx-helpers.js"),
};

pub const JS_STATUS: StaticAsset = StaticAsset {
    path: "/js/status.js",
    content_type: "application/javascript; charset=utf-8",
    etag: "rinjani-js-status-v102",
    content: include_bytes!("../assets/js/status.js"),
};

pub const VENDOR_HTMX: StaticAsset = StaticAsset {
    path: "/vendor/htmx.js",
    content_type: "application/javascript; charset=utf-8",
    etag: "rinjani-htmx-stub-v1",
    content: include_bytes!("../assets/vendor/htmx.js"),
};

pub const LOGO_SVG: StaticAsset = StaticAsset {
    path: "/images/logo.svg",
    content_type: "image/svg+xml; charset=utf-8",
    etag: "mitranet-logo-svg",
    content: include_bytes!("../assets/images/logo.svg"),
};

pub const ALL_ASSETS: [&StaticAsset; 9] = [
    &SHELL_HTML,
    &CSS_BASE,
    &CSS_LAYOUT,
    &CSS_COMPONENTS,
    &JS_COMMON_OPCR,
    &JS_COMMON_HTMX,
    &JS_STATUS,
    &VENDOR_HTMX,
    &LOGO_SVG,
];
