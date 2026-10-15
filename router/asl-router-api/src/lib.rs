//! `asl-router-api` — REST API v1 Dispatcher & Security Rate Limiter
//!
//! Bare-metal `#![no_std]` REST API endpoints returning strict JSON:
//! - GET `/api/v1/system` -> System metadata, kernel, OS, uptime, hostname
//! - GET `/api/v1/interfaces` -> Port states, IPs, MACs, stats
//! - GET `/api/v1/firewall/rules` -> TCAM / NAT filter rules
//! - GET `/api/v1/services` -> Active daemons (dhcp, http, cli, dns/mdns)
//! - Token-Bucket Rate Limiter per client / global (RFC 6585 HTTP 429)
//!
//! SELURUH nilai pada respons bersumber dari `WebRuntime` yang diisi kernel per
//! iterasi poll loop. Endpoint dilarang mengarang alamat IP atau status service.

#![no_std]

use asl_router_webcore::{HttpRequest, HttpResponseBuilder, HttpStatus};

/// Snapshot state runtime router yang dilaporkan ke WebUI. Kernel mengisi
/// struktur ini setiap iterasi; `tick_100ms` adalah satuan waktu bucket limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebRuntime<'a> {
    pub hostname: &'a str,
    pub uptime_ms: u64,
    pub wan_ip: [u8; 4],
    pub wan_netmask: [u8; 4],
    pub wan_gw: [u8; 4],
    pub wan_up: bool,
    pub wan_source: &'static str,
    pub wan_mac: [u8; 6],
    pub lan_present: bool,
    pub lan_ip: [u8; 4],
    pub lan_mac: [u8; 6],
    pub http_listening: bool,
    pub http_port: u16,
    pub dhcp_client_active: bool,
    pub dns_active: bool,
    pub mdns_active: bool,
    pub serial_cli_active: bool,
    pub tcam_rules: u32,
    pub dnat_rules: u32,
    pub default_policy: &'static str,
    pub wan_rx: u32,
    pub wan_tx: u32,
    pub lan_rx: u32,
    pub lan_tx: u32,
    pub sessions_active: u32,
    pub requests_served: u32,
    pub tick_100ms: u64,
}

impl<'a> Default for WebRuntime<'a> {
    fn default() -> Self {
        Self {
            hostname: "",
            uptime_ms: 0,
            wan_ip: [0, 0, 0, 0],
            wan_netmask: [0, 0, 0, 0],
            wan_gw: [0, 0, 0, 0],
            wan_up: false,
            wan_source: "none",
            wan_mac: [0; 6],
            lan_present: false,
            lan_ip: [0; 4],
            lan_mac: [0; 6],
            http_listening: false,
            http_port: 0,
            dhcp_client_active: false,
            dns_active: false,
            mdns_active: false,
            serial_cli_active: false,
            tcam_rules: 0,
            dnat_rules: 0,
            default_policy: "deny",
            wan_rx: 0,
            wan_tx: 0,
            lan_rx: 0,
            lan_tx: 0,
            sessions_active: 0,
            requests_served: 0,
            tick_100ms: 0,
        }
    }
}

// -----------------------------------------------------------------------------
// JSON WRITER BUFFER TETAP (zero heap)
// -----------------------------------------------------------------------------

struct JsonBuf<'a> {
    buf: &'a mut [u8],
    len: usize,
    overflow: bool,
}

impl<'a> JsonBuf<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            len: 0,
            overflow: false,
        }
    }

    fn raw(&mut self, s: &[u8]) -> &mut Self {
        if self.len + s.len() > self.buf.len() {
            self.overflow = true;
            return self;
        }
        self.buf[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
        self
    }

    fn str(&mut self, s: &str) -> &mut Self {
        self.raw(s.as_bytes())
    }

    fn u32(&mut self, mut v: u32) -> &mut Self {
        let mut tmp = [0u8; 10];
        let mut i = tmp.len();
        if v == 0 {
            i -= 1;
            tmp[i] = b'0';
        }
        while v > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        self.raw(&tmp[i..])
    }

    fn u64(&mut self, mut v: u64) -> &mut Self {
        let mut tmp = [0u8; 20];
        let mut i = tmp.len();
        if v == 0 {
            i -= 1;
            tmp[i] = b'0';
        }
        while v > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        self.raw(&tmp[i..])
    }

    fn bool(&mut self, v: bool) -> &mut Self {
        self.raw(if v { b"true" } else { b"false" })
    }

    fn ip(&mut self, ip: [u8; 4]) -> &mut Self {
        self.u32(ip[0] as u32)
            .raw(b".")
            .u32(ip[1] as u32)
            .raw(b".")
            .u32(ip[2] as u32)
            .raw(b".")
            .u32(ip[3] as u32)
    }

    fn mac_hex(&mut self, mac: [u8; 6]) -> &mut Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut b = [0u8; 17];
        let mut i = 0;
        for (n, o) in mac.iter().enumerate() {
            b[i] = HEX[(*o >> 4) as usize];
            i += 1;
            b[i] = HEX[(*o & 0x0F) as usize];
            i += 1;
            if n < 5 {
                b[i] = b':';
                i += 1;
            }
        }
        self.raw(&b[..i])
    }

    fn quoted_mac(&mut self, mac: [u8; 6]) -> &mut Self {
        self.raw(b"\"").mac_hex(mac).raw(b"\"")
    }

    fn quoted_ip(&mut self, ip: [u8; 4]) -> &mut Self {
        self.raw(b"\"").ip(ip).raw(b"\"")
    }

    fn quoted(&mut self, s: &str) -> &mut Self {
        self.raw(b"\"").str(s).raw(b"\"")
    }

    fn finish(&self) -> Option<usize> {
        if self.overflow {
            None
        } else {
            Some(self.len)
        }
    }
}

pub struct TokenBucketRateLimiter {
    capacity: u32,
    tokens: u32,
    refill_rate: u32, // tokens per tick
    last_tick: u64,
}

impl TokenBucketRateLimiter {
    pub const fn new(capacity: u32, refill_rate: u32) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_rate,
            last_tick: 0,
        }
    }

    pub fn check_and_consume(&mut self, current_tick: u64) -> bool {
        if current_tick > self.last_tick {
            let elapsed = current_tick - self.last_tick;
            let refill = (elapsed as u32).saturating_mul(self.refill_rate);
            self.tokens = (self.tokens + refill).min(self.capacity);
            self.last_tick = current_tick;
        }

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

pub struct ApiDispatcher {
    pub rate_limiter: TokenBucketRateLimiter,
}

impl ApiDispatcher {
    pub const fn new() -> Self {
        Self {
            // Bucket: burst 10 permintaan, refill 2 token per tick 100 ms.
            rate_limiter: TokenBucketRateLimiter::new(10, 2),
        }
    }

    pub fn dispatch(
        &mut self,
        req: &HttpRequest,
        current_tick: u64,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        if !self.rate_limiter.check_and_consume(current_tick) {
            let builder = HttpResponseBuilder::new(out_buf, HttpStatus::TooManyRequests)
                .with_content_type("application/json");
            return builder.finish_with_body(b"{\"ok\":false,\"error\":\"rate_limit_exceeded\"}");
        }

        match req.path {
            "/api/v1/system" => self.handle_system(out_buf, rt),
            "/api/v1/interfaces" => self.handle_interfaces(out_buf, rt),
            "/api/v1/firewall/rules" => self.handle_firewall_rules(out_buf, rt),
            "/api/v1/services" => self.handle_services(out_buf, rt),
            _ => {
                let builder = HttpResponseBuilder::new(out_buf, HttpStatus::NotFound)
                    .with_content_type("application/json");
                builder.finish_with_body(b"{\"ok\":false,\"error\":\"not_found\"}")
            }
        }
    }

    fn ok_json(out_buf: &mut [u8], body: &[u8]) -> Result<usize, &'static str> {
        let builder =
            HttpResponseBuilder::new(out_buf, HttpStatus::Ok).with_content_type("application/json");
        builder.finish_with_body(body)
    }

    fn handle_system(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"distro\":\"Rinjani 1.0.0\",")
                .raw(b"\"kernel\":\"ASL Core Kernel v1.0.0\",\"hostname\":")
                .quoted(rt.hostname)
                .raw(b",\"arch\":\"x86_64\",\"uptime_ms\":")
                .u64(rt.uptime_ms)
                .raw(b",\"wan_up\":")
                .bool(rt.wan_up)
                .raw(b",\"wan_source\":")
                .quoted(rt.wan_source)
                .raw(b",\"requests_served\":")
                .u32(rt.requests_served)
                .raw(b",\"status\":");
            j.quoted(if rt.wan_up { "operational" } else { "wan-down" });
            j.raw(b"}}");
            j.finish().ok_or("JSON system overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_interfaces(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 768];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[{\"name\":\"eth0\",\"mode\":\"wan_bridged\",\"mac\":");
            j.quoted_mac(rt.wan_mac)
                .raw(b",\"ip\":")
                .quoted_ip(rt.wan_ip)
                .raw(b",\"netmask\":")
                .quoted_ip(rt.wan_netmask)
                .raw(b",\"gateway\":")
                .quoted_ip(rt.wan_gw)
                .raw(b",\"source\":")
                .quoted(rt.wan_source)
                .raw(b",\"status\":");
            j.quoted(if rt.wan_up { "up" } else { "down" });
            j.raw(b",\"rx_packets\":")
                .u32(rt.wan_rx)
                .raw(b",\"tx_packets\":")
                .u32(rt.wan_tx)
                .raw(b"}");
            if rt.lan_present {
                j.raw(b",{\"name\":\"eth1\",\"mode\":\"lan_hostonly\",\"mac\":")
                    .quoted_mac(rt.lan_mac)
                    .raw(b",\"ip\":")
                    .quoted_ip(rt.lan_ip)
                    .raw(b",\"status\":")
                    .quoted("up")
                    .raw(b",\"rx_packets\":")
                    .u32(rt.lan_rx)
                    .raw(b",\"tx_packets\":")
                    .u32(rt.lan_tx)
                    .raw(b"}");
            } else {
                j.raw(b",{\"name\":\"eth1\",\"mode\":\"lan_hostonly\",\"status\":");
                j.quoted("absent").raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON interfaces overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_firewall_rules(
        &self,
        out: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        let mut body = [0u8; 384];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"default_policy\":")
                .quoted(rt.default_policy)
                .raw(b",\"tcam_rules\":")
                .u32(rt.tcam_rules)
                .raw(b",\"dnat_rules\":")
                .u32(rt.dnat_rules)
                .raw(b",\"wan_protected\":")
                .bool(rt.wan_up)
                .raw(b",\"http_port_open\":")
                .bool(rt.http_listening)
                .raw(b",\"http_port\":")
                .u32(rt.http_port as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON firewall overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_services(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 384];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"dhcp_client\":")
                .bool(rt.dhcp_client_active)
                .raw(b",\"http_server\":")
                .bool(rt.http_listening)
                .raw(b",\"mdns_responder\":")
                .bool(rt.mdns_active)
                .raw(b",\"dns_proxy\":")
                .bool(rt.dns_active)
                .raw(b",\"serial_cli\":")
                .bool(rt.serial_cli_active)
                .raw(b",\"http_port\":")
                .u32(rt.http_port as u32)
                .raw(b",\"tcp_sessions_active\":")
                .u32(rt.sessions_active)
                .raw(b",\"http_bind_ip\":")
                .quoted_ip(rt.wan_ip)
                .raw(b"}}");
            j.finish().ok_or("JSON services overflow")?
        };
        Self::ok_json(out, &body[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_rate_limiter_depletion_and_refill() {
        let mut limiter = TokenBucketRateLimiter::new(3, 1);
        assert!(limiter.check_and_consume(0));
        assert!(limiter.check_and_consume(0));
        assert!(limiter.check_and_consume(0));
        // Exhausted
        assert!(!limiter.check_and_consume(0));

        // Refill after 2 ticks
        assert!(limiter.check_and_consume(2));
    }

    fn rt() -> WebRuntime<'static> {
        WebRuntime {
            hostname: "rinjani-wan01",
            uptime_ms: 123456,
            wan_ip: [198, 51, 100, 77],
            wan_netmask: [255, 255, 255, 0],
            wan_gw: [198, 51, 100, 254],
            wan_up: true,
            wan_source: "dhcp",
            wan_mac: [0x08, 0x00, 0x27, 0x11, 0x22, 0x33],
            lan_present: false,
            lan_ip: [0; 4],
            lan_mac: [0; 6],
            http_listening: true,
            http_port: 80,
            dhcp_client_active: true,
            dns_active: false,
            mdns_active: false,
            serial_cli_active: true,
            tcam_rules: 16,
            dnat_rules: 1,
            default_policy: "deny",
            wan_rx: 41,
            wan_tx: 39,
            lan_rx: 0,
            lan_tx: 0,
            sessions_active: 2,
            requests_served: 7,
            tick_100ms: 0,
        }
    }

    fn path(p: &'static [u8]) -> HttpRequest<'static> {
        HttpRequest::parse(p).unwrap()
    }

    #[test]
    fn test_02_api_system_endpoint() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"distro\":\"Rinjani 1.0.0\""));
        assert!(resp.contains("\"hostname\":\"rinjani-wan01\""));
        assert!(resp.contains("\"uptime_ms\":123456"));
        assert!(resp.contains("\"wan_source\":\"dhcp\""));
    }

    #[test]
    fn test_03_api_interfaces_reports_real_runtime_values() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"name\":\"eth0\""));
        assert!(resp.contains("\"ip\":\"198.51.100.77\""));
        assert!(resp.contains("\"gateway\":\"198.51.100.254\""));
        assert!(resp.contains("\"mac\":\"08:00:27:11:22:33\""));
        assert!(resp.contains("\"rx_packets\":41"));
        // eth1 tidak ada pada runtime ini -> dilaporkan absent, bukan IP karangan.
        assert!(resp.contains("\"status\":\"absent\""));
        assert!(!resp.contains("192.168.56.1"));
        assert!(!resp.contains("192.0.2.199"));
    }

    #[test]
    fn test_03b_api_interfaces_includes_lan_when_present() {
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.lan_present = true;
        r.lan_ip = [198, 51, 100, 1];
        r.lan_mac = [1, 2, 3, 4, 5, 6];
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"name\":\"eth1\""));
        assert!(resp.contains("\"ip\":\"198.51.100.1\""));
    }

    #[test]
    fn test_04_api_firewall_rules_endpoint() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"default_policy\":\"deny\""));
        assert!(resp.contains("\"tcam_rules\":16"));
        assert!(resp.contains("\"http_port\":80"));
    }

    #[test]
    fn test_05_api_services_reflects_actual_daemons() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/services HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"http_server\":true"));
        assert!(resp.contains("\"mdns_responder\":false"));
        assert!(resp.contains("\"dns_proxy\":false"));
        assert!(resp.contains("\"http_bind_ip\":\"198.51.100.77\""));
    }

    #[test]
    fn test_05b_api_services_reports_http_down_when_not_listening() {
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.http_listening = false;
        r.wan_up = false;
        r.wan_ip = [0, 0, 0, 0];
        let req = path(b"GET /api/v1/services HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"http_server\":false"));
        assert!(resp.contains("\"http_bind_ip\":\"0.0.0.0\""));
    }

    #[test]
    fn test_06_api_not_found_endpoint() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/unknown HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 404 Not Found"));
        assert!(resp.contains("\"error\":\"not_found\""));
    }

    #[test]
    fn test_07_api_rate_limit_429() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        // Consume all 10 tokens
        for _ in 0..10 {
            assert!(dispatcher.dispatch(&req, 0, &mut buf, &rt()).is_ok());
        }
        // 11th request must get 429
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 429 Too Many Requests"));
        assert!(resp.contains("\"error\":\"rate_limit_exceeded\""));
    }

    #[test]
    fn test_08_api_envelope_format() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 1, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("{\"ok\":true,\"data\":"));
    }

    #[test]
    fn test_09_json_writer_ip_mac_and_overflow_guard() {
        let mut body = [0u8; 64];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ip\":").quoted_ip([10, 0, 0, 1]).raw(b",\"m\":");
            j.quoted_mac([0xab, 0xcd, 0xef, 0x00, 0x11, 0x22]);
            j.raw(b",\"u\":").u32(0).raw(b",\"b\":").bool(false);
            j.raw(b"}");
            j.finish().unwrap()
        };
        assert_eq!(
            core::str::from_utf8(&body[..n]).unwrap(),
            "{\"ip\":\"10.0.0.1\",\"m\":\"ab:cd:ef:00:11:22\",\"u\":0,\"b\":false}"
        );

        let mut tiny = [0u8; 8];
        let res = {
            let mut j = JsonBuf::new(&mut tiny);
            j.raw(b"0123456789abcdef");
            j.finish()
        };
        assert!(res.is_none(), "overflow wajib terdeteksi");
    }
}
