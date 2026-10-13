//! `asl-router-webui` — Embedded WebUI Frontend & Asset Dispatcher
//!
//! Exposes embedded unminified web assets and handles static resource dispatch.

#![no_std]

pub mod registry;

use asl_router_webcore::{HttpRequest, serve_static};

pub fn dispatch_static(req: &HttpRequest, out_buf: &mut [u8]) -> Option<Result<usize, &'static str>> {
    let path = if req.path == "/" || req.path == "/status" || req.path == "/interfaces" || req.path == "/firewall" || req.path == "/services" {
        "/"
    } else {
        req.path
    };

    for asset in registry::ALL_ASSETS.iter() {
        if asset.path == path {
            return Some(serve_static(asset, req, out_buf));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_modules_registry() {
        assert_eq!(registry::MODULES.len(), 4);
        assert_eq!(registry::MODULES[0].name, "Dashboard");
        assert!(registry::MODULES[0].enabled);
    }

    #[test]
    fn test_02_all_assets_present() {
        assert_eq!(registry::ALL_ASSETS.len(), 8);
        assert_eq!(registry::SHELL_HTML.content_type, "text/html; charset=utf-8");
        assert!(registry::SHELL_HTML.content.len() > 0);
    }

    #[test]
    fn test_03_dispatch_shell_root() {
        let req = HttpRequest::parse(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 4096];
        let res = dispatch_static(&req, &mut buf).expect("Must find root asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("RINJANI 1.0.0"));
        assert!(resp.contains("Open Core Router Web Console"));
    }

    #[test]
    fn test_04_dispatch_css_asset() {
        let req = HttpRequest::parse(b"GET /css/base.css HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 4096];
        let res = dispatch_static(&req, &mut buf).expect("Must find css asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("Content-Type: text/css"));
        assert!(resp.contains("--bg-main"));
    }

    #[test]
    fn test_05_dispatch_vendor_htmx() {
        let req = HttpRequest::parse(b"GET /vendor/htmx.js HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 4096];
        let res = dispatch_static(&req, &mut buf).expect("Must find htmx asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("Content-Type: application/javascript"));
        assert!(resp.contains("htmx = factory()"));
    }
}
