//! `asl-router-api` — REST API v1 Dispatcher & Security Rate Limiter
//!
//! Bare-metal `#![no_std]` REST API endpoints returning strict JSON:
//! - GET `/api/v1/system` -> System metadata, kernel, OS, uptime, hostname
//! - GET `/api/v1/interfaces` -> HANYA antarmuka yang benar-benar terdeteksi
//! - GET `/api/v1/firewall/rules` -> TCAM / NAT filter rules
//! - GET `/api/v1/nat` + `/api/v1/nat/rules` + `/api/v1/nat/conntrack`
//! - GET `/api/v1/wireless` + `/api/v1/wireless/devices` (driver none => 0 baris)
//! - GET `/api/v1/olt` + `/api/v1/olt/ons` (marker crate => 0 baris)
//! - GET `/api/v1/services` -> Active daemons (dhcp, http, cli, dns/mdns)
//! - POST `/api/v1/interfaces/<nama>` -> antrean tulis yang dieksekusi poll loop
//! - Token-Bucket Rate Limiter per client / global (RFC 6585 HTTP 429)
//!
//! SELURUH nilai pada respons bersumber dari `WebRuntime` yang diisi kernel per
//! iterasi poll loop. Endpoint dilarang mengarang alamat IP atau status service.
//! Endpoint tulis hanya memvalidasi + menaruh perintah di antrean; eksekusi
//! (perubahan FIB/NAT/DHCP dan commit RJCFG01) selalu terjadi di sisi kernel.

#![no_std]

use asl_router_webcore::{HttpRequest, HttpResponseBuilder, HttpStatus};

/// Panjang maksimum nama antarmuka pada jalur tulis WebUI. Sumber kebenarannya
/// skema RJCFG01, supaya aturan validasi UI tidak pernah lepas dari penyimpanan.
pub const IFACE_NAME_MAX: usize = asl_router_persistence::RJCFG_IFACE_NAME_MAX;
/// Jumlah perintah tulis yang dapat menganggu sebelum poll loop mengeksekusi.
pub const WEB_WRITE_QUEUE: usize = 4;
/// Batas baris snapshot conntrack yang dibawa ke satu respons JSON.
pub const NAT_CONN_SNAPSHOT_MAX: usize = 12;
/// Kode mode WAN pada perintah tulis (sama dengan konstanta journal).
pub const WAN_MODE_DHCP: u8 = asl_router_persistence::RJCFG_WAN_MODE_DHCP;
pub const WAN_MODE_STATIC: u8 = asl_router_persistence::RJCFG_WAN_MODE_STATIC;
/// Panjang maksimum body form yang diterima satu permintaan tulis.
pub const WEB_WRITE_BODY_MAX: usize = 512;
/// Kode aksi daya hasil POST `/api/v1/power` — sama dengan perintah CLI serial.
pub const POWER_RESTART: u8 = 0;
pub const POWER_SHUTDOWN: u8 = 1;
/// Jendela arming aksi daya: 20 s dihitung pada tick 100 ms kernel.
pub const POWER_ARM_TICKS: u64 = 200;
/// Sela antara ACK dan eksekusi daya, supaya respons HTTP sempat terkirim ke
/// browser sebelum kernel reboot/power-off.
pub const POWER_EXEC_MS: u64 = 1_500;

use asl_router_persistence::iface_name_ok;

/// Snapshot satu aturan DNAT (port forwarding) milik `NatEngine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnatSnapshot {
    pub wan_port: u16,
    pub protocol: &'static str,
    pub target_ip: [u8; 4],
    pub target_port: u16,
    pub active: bool,
}

/// Snapshot satu sesi conntrack (untuk tabel NAT hidup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnSnapshot {
    pub protocol: &'static str,
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub dst_ip: [u8; 4],
    pub dst_port: u16,
    pub trans_port: u16,
    pub fwd_packets: u32,
    pub rev_packets: u32,
}

/// Satu perintah tulis hasil POST `/api/v1/interfaces/<nama>`. Struct ini murni
/// data: kernel yang memutuskan eksekusi, sehingga validasi API tidak pernah
/// bisa mengubah state sendiri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebWriteCmd {
    pub target: [u8; IFACE_NAME_MAX],
    pub target_len: u8,
    pub new_name: [u8; IFACE_NAME_MAX],
    pub new_name_len: u8,
    pub mode: u8,
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub gw: [u8; 4],
    pub persist: bool,
}

impl WebWriteCmd {
    pub fn target_str(&self) -> &str {
        let n = core::cmp::min(self.target_len as usize, IFACE_NAME_MAX);
        core::str::from_utf8(&self.target[..n]).unwrap_or("")
    }

    pub fn new_name_str(&self) -> &str {
        let n = core::cmp::min(self.new_name_len as usize, IFACE_NAME_MAX);
        core::str::from_utf8(&self.new_name[..n]).unwrap_or("")
    }
}

/// Snapshot state runtime router yang dilaporkan ke WebUI. Kernel mengisi
/// struktur ini setiap iterasi; `tick_100ms` adalah satuan waktu bucket limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebRuntime<'a> {
    pub hostname: &'a str,
    pub uptime_ms: u64,
    pub wan_present: bool,
    pub wan_name: &'a str,
    pub lan_name: &'a str,
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
    /// `true` bila alamat WAN sedang dipegang operator (mode statik).
    pub wan_config_static: bool,
    pub dns_active: bool,
    pub mdns_active: bool,
    pub serial_cli_active: bool,
    pub tcam_rules: u32,
    pub dnat_rules: u32,
    pub default_policy: &'static str,
    /// Salinan aturan DNAT nyata dari `NatEngine` (maks `MAX_DNAT_RULES`).
    pub dnat_list: &'a [DnatSnapshot],
    /// Salinan sesi conntrack aktif (terbatas `NAT_CONN_SNAPSHOT_MAX`).
    pub nat_sessions: &'a [ConnSnapshot],
    /// Total sesi conntrack aktif di tabel kernel (bisa > panjang snapshot).
    pub nat_session_count: u32,
    pub nat_capacity: u32,
    pub nat_table_full_drops: u32,
    /// `true` hanya bila datapath kernel benar-benar memanggil mesin NAT.
    pub nat_forwarding_wired: bool,
    /// `true` hanya bila aturan TCAM benar-benar ditanyakan pada frame yang
    /// lewat (klasifikasi in-line). Mesin yang berdiri sendiri tanpa dipanggil
    /// jalur data tidak boleh dilaporkan sebagai perlindungan.
    pub firewall_wired: bool,
    /// Hasil penerapan perintah tulis terakhir oleh kernel (dipakai modal WebUI).
    pub last_write_ok: bool,
    pub last_write_report: &'a str,
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
            wan_present: false,
            wan_name: "",
            lan_name: "",
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
            wan_config_static: false,
            dns_active: false,
            mdns_active: false,
            serial_cli_active: false,
            tcam_rules: 0,
            dnat_rules: 0,
            default_policy: "deny",
            dnat_list: &[],
            nat_sessions: &[],
            nat_session_count: 0,
            nat_capacity: 0,
            nat_table_full_drops: 0,
            nat_forwarding_wired: false,
            firewall_wired: false,
            last_write_ok: false,
            last_write_report: "",
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
    /// Antrean perintah tulis hasil POST. Poll kernel mengambilnya lewat
    /// `take_write()`; dispatcher tidak pernah mengubah state router.
    write_queue: [Option<WebWriteCmd>; WEB_WRITE_QUEUE],
    write_queue_len: usize,
    /// Hitungan percobaan tulis yang ditolak (antrean penuh / validasi gagal).
    pub writes_rejected: u32,
    /// Penjaga aksi daya: `power_armed` dinyalakan oleh `action=arm` dan
    /// kedaluwarsa setelah `POWER_ARM_TICKS`. Satu arm melayani satu aksi.
    power_armed: bool,
    power_arm_tick: u64,
    /// Aksi daya tertunda yang sudah diterima kernel (dieksekusi poll loop).
    power_pending: Option<u8>,
}

impl ApiDispatcher {
    pub const fn new() -> Self {
        Self {
            // Bucket: burst 10 permintaan, refill 2 token per tick 100 ms.
            rate_limiter: TokenBucketRateLimiter::new(10, 2),
            write_queue: [None, None, None, None],
            write_queue_len: 0,
            writes_rejected: 0,
            power_armed: false,
            power_arm_tick: 0,
            power_pending: None,
        }
    }

    /// Sisa jendela arming dalam tick; `None` bila tidak ter-arm atau kedaluwarsa.
    fn arm_left_ticks(&self, tick: u64) -> Option<u64> {
        if !self.power_armed {
            return None;
        }
        let age = tick.wrapping_sub(self.power_arm_tick);
        if age >= POWER_ARM_TICKS {
            None
        } else {
            Some(POWER_ARM_TICKS - age)
        }
    }

    pub fn power_armed(&self, tick: u64) -> bool {
        self.arm_left_ticks(tick).is_some()
    }

    pub fn power_arm_left_s(&self, tick: u64) -> u32 {
        let ticks = self.arm_left_ticks(tick).unwrap_or(0);
        ((ticks + 9) / 10) as u32
    }

    /// Ambil aksi daya yang sudah diterima, untuk dieksekusi kernel.
    pub fn take_power(&mut self) -> Option<u8> {
        self.power_pending.take()
    }

    /// Ambil satu perintah tulis tertua untuk dieksekusi kernel.
    pub fn take_write(&mut self) -> Option<WebWriteCmd> {
        if self.write_queue_len == 0 {
            return None;
        }
        let cmd = self.write_queue[0].take();
        for i in 0..self.write_queue_len.saturating_sub(1) {
            self.write_queue[i] = self.write_queue[i + 1];
        }
        self.write_queue[self.write_queue_len - 1] = None;
        self.write_queue_len -= 1;
        cmd
    }

    /// Masukkan perintah ke antrean. `false` bila antrean penuh (kernel belum
    /// menyempatkan drain) — klien harus mencoba lagi, bukan diterima diam-diam.
    pub fn push_write(&mut self, cmd: WebWriteCmd) -> bool {
        if self.write_queue_len >= WEB_WRITE_QUEUE {
            self.writes_rejected = self.writes_rejected.wrapping_add(1);
            return false;
        }
        self.write_queue[self.write_queue_len] = Some(cmd);
        self.write_queue_len += 1;
        true
    }

    pub fn write_queue_depth(&self) -> usize {
        self.write_queue_len
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
            "/api/v1/nat" => self.handle_nat_summary(out_buf, rt),
            "/api/v1/nat/rules" => self.handle_nat_rules(out_buf, rt),
            "/api/v1/nat/conntrack" => self.handle_nat_conntrack(out_buf, rt),
            "/api/v1/wireless" => self.handle_wireless(out_buf),
            "/api/v1/wireless/devices" => self.handle_wireless_devices(out_buf),
            "/api/v1/olt" => self.handle_olt(out_buf),
            "/api/v1/olt/ons" => self.handle_olt_ons(out_buf),
            "/api/v1/services" => self.handle_services(out_buf, rt),
            "/api/v1/writes/last" => self.handle_write_status(out_buf, rt),
            _ => {
                let builder = HttpResponseBuilder::new(out_buf, HttpStatus::NotFound)
                    .with_content_type("application/json");
                builder.finish_with_body(b"{\"ok\":false,\"error\":\"not_found\"}")
            }
        }
    }

    /// Jalur tulis: `POST /api/v1/interfaces/<nama>` dengan body
    /// `application/x-www-form-urlencoded`. Respons hanya melaporkan perintah
    /// masuk antrean atau alasan penolakan; kernel yang mengeksekusi.
    pub fn dispatch_post(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        if !self.rate_limiter.check_and_consume(rt.tick_100ms) {
            let builder = HttpResponseBuilder::new(out_buf, HttpStatus::TooManyRequests)
                .with_content_type("application/json");
            return builder.finish_with_body(b"{\"ok\":false,\"error\":\"rate_limit_exceeded\"}");
        }
        if !self.same_origin(req, rt) {
            self.writes_rejected = self.writes_rejected.wrapping_add(1);
            return self.err_json(out_buf, HttpStatus::Forbidden, "host_mismatch");
        }
        if req.path == "/api/v1/power" {
            return self.handle_power(req, out_buf, rt);
        }
        let iface = match req.path.strip_prefix("/api/v1/interfaces/") {
            Some(rest) if !rest.is_empty() => rest,
            _ => return self.err_json(out_buf, HttpStatus::NotFound, "not_found"),
        };
        let body = &req.body[..core::cmp::min(req.content_length, req.body.len())];
        match self.build_write_cmd(iface, req.content_type, body) {
            Ok(cmd) => {
                if !self.push_write(cmd) {
                    return self.err_json(
                        out_buf,
                        HttpStatus::InternalServerError,
                        "write_queue_full",
                    );
                }
                let mut ack = [0u8; 128];
                let n = {
                    let mut j = JsonBuf::new(&mut ack);
                    j.raw(b"{\"ok\":true,\"data\":{\"status\":\"queued\",\"target\":")
                        .quoted(iface)
                        .raw(b",\"mode\":")
                        .quoted(if cmd.mode == WAN_MODE_STATIC {
                            "static"
                        } else {
                            "dhcp"
                        })
                        .raw(b",\"persist\":")
                        .bool(cmd.persist)
                        .raw(b",\"queue_depth\":")
                        .u32(self.write_queue_len as u32)
                        .raw(b"}}");
                    j.finish().ok_or("JSON write ack overflow")?
                };
                Self::ok_json(out_buf, &ack[..n])
            }
            Err(code) => {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                self.err_json(out_buf, HttpStatus::BadRequest, code)
            }
        }
    }

    /// Jalur daya dua-langkah: `POST /api/v1/power` dengan body `action=`.
    /// `arm` menyalakan slot selama `POWER_ARM_TICKS`; `restart`/`shutdown`
    /// hanya diterima bila slot masih ter-arm dan langsung dihabiskan, sehingga
    /// satu permintaan liar dari jaringan tidak bisa mematikan router. Kernel
    /// yang mengeksekusi (poll loop), dispatcher hanya menampung.
    fn handle_power(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        let ct = match req.content_type {
            Some(c) => c,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_content_type"),
        };
        if !starts_with_ci(ct, "application/x-www-form-urlencoded") {
            return self.err_json(out_buf, HttpStatus::BadRequest, "unsupported_content_type");
        }
        let body = &req.body[..core::cmp::min(req.content_length, req.body.len())];
        let action = match Form::parse(body).get("action") {
            Some(a) => a,
            None => {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                return self.err_json(out_buf, HttpStatus::BadRequest, "missing_action");
            }
        };
        if action == "arm" {
            self.power_armed = true;
            self.power_arm_tick = rt.tick_100ms;
            let mut ack = [0u8; 96];
            let n = {
                let mut j = JsonBuf::new(&mut ack);
                j.raw(b"{\"ok\":true,\"data\":{\"status\":\"armed\",\"valid_for_s\":")
                    .u32((POWER_ARM_TICKS / 10) as u32)
                    .raw(b"}}");
                j.finish().ok_or("JSON power arm ack overflow")?
            };
            return Self::ok_json(out_buf, &ack[..n]);
        }
        let code = match action {
            "restart" => POWER_RESTART,
            "shutdown" => POWER_SHUTDOWN,
            _ => {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                return self.err_json(out_buf, HttpStatus::BadRequest, "bad_power_action");
            }
        };
        if !self.power_armed(rt.tick_100ms) {
            self.writes_rejected = self.writes_rejected.wrapping_add(1);
            self.power_armed = false;
            return self.err_json(out_buf, HttpStatus::Forbidden, "power_not_armed");
        }
        self.power_armed = false;
        self.power_pending = Some(code);
        let mut ack = [0u8; 128];
        let n = {
            let mut j = JsonBuf::new(&mut ack);
            j.raw(b"{\"ok\":true,\"data\":{\"status\":\"scheduled\",\"action\":")
                .quoted(action)
                .raw(b",\"exec_in_ms\":")
                .u32(POWER_EXEC_MS as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON power ack overflow")?
        };
        Self::ok_json(out_buf, &ack[..n])
    }

    fn err_json(
        &self,
        out_buf: &mut [u8],
        status: HttpStatus,
        code: &'static str,
    ) -> Result<usize, &'static str> {
        let mut body = [0u8; 96];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":false,\"error\":").quoted(code).raw(b"}");
            j.finish().ok_or("JSON error overflow")?
        };
        let builder =
            HttpResponseBuilder::new(out_buf, status).with_content_type("application/json");
        builder.finish_with_body(&body[..n])
    }

    /// Proteksi CSRF/same-origin sederhana: `Host` wajib sama dengan alamat WAN
    /// atau hostname router (dengan atau tanpa port). Tanpa Host -> ditolak.
    fn same_origin(&self, req: &HttpRequest, rt: &WebRuntime) -> bool {
        let host = match req.host {
            Some(h) => h,
            None => return false,
        };
        let (host_name, port) = split_host_port(host);
        if let Some(p) = port {
            if p != rt.http_port {
                return false;
            }
        }
        let mut ipbuf = [0u8; 15];
        let ip_len = {
            let mut j = JsonBuf::new(&mut ipbuf);
            j.ip(rt.wan_ip);
            j.finish().unwrap_or(0)
        };
        let ip_str = core::str::from_utf8(&ipbuf[..ip_len]).unwrap_or("0.0.0.0");
        host_name == ip_str || (!rt.hostname.is_empty() && host_name == rt.hostname)
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

    /// Inventaris antarmuka REALTIME: hanya NIC yang benar-benar terdeteksi
    /// kernel yang muncul sebagai baris. Tidak ada baris `absent` placeholder;
    /// satu NIC berarti satu baris.
    fn handle_interfaces(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 1024];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            if rt.wan_present {
                j.raw(b"{\"name\":").quoted(rt.wan_name);
                j.raw(b",\"role\":").quoted("wan");
                j.raw(b",\"type\":").quoted("ethernet");
                j.raw(b",\"mac\":").quoted_mac(rt.wan_mac);
                j.raw(b",\"ip\":").quoted_ip(rt.wan_ip);
                j.raw(b",\"netmask\":").quoted_ip(rt.wan_netmask);
                j.raw(b",\"gateway\":").quoted_ip(rt.wan_gw);
                j.raw(b",\"source\":").quoted(rt.wan_source);
                j.raw(b",\"config_mode\":");
                j.quoted(if rt.wan_config_static {
                    "static"
                } else {
                    "dhcp"
                });
                j.raw(b",\"status\":")
                    .quoted(if rt.wan_up { "up" } else { "down" });
                j.raw(b",\"running\":").bool(rt.wan_up);
                j.raw(b",\"rx_packets\":")
                    .u32(rt.wan_rx)
                    .raw(b",\"tx_packets\":")
                    .u32(rt.wan_tx)
                    .raw(b",\"editable\":")
                    .bool(true)
                    .raw(b"}");
            }
            if rt.lan_present {
                // Pemisah hanya bila baris WAN gerçekten tercetak.
                if rt.wan_present {
                    j.raw(b",");
                }
                // Kernel belum punya sensor carrier link, jadi status LAN dibaca
                // dari fakta yang memang ada: alamat IPv4 yang terpasang pada
                // interface itu. Literal "up" akan melaporkan interface tanpa
                // alamat sebagai berjalan.
                let lan_up = rt.lan_ip != [0, 0, 0, 0];
                j.raw(b"{\"name\":").quoted(rt.lan_name);
                j.raw(b",\"role\":").quoted("lan");
                j.raw(b",\"type\":").quoted("ethernet");
                j.raw(b",\"mac\":").quoted_mac(rt.lan_mac);
                j.raw(b",\"ip\":").quoted_ip(rt.lan_ip);
                // Netmask/gateway sisi LAN host-only tidak dilaporkan kernel:
                // field sengaja tidak ada, ditampilkan sebagai em-dash.
                j.raw(b",\"source\":").quoted("local");
                j.raw(b",\"config_mode\":").quoted("local");
                j.raw(b",\"status\":")
                    .quoted(if lan_up { "up" } else { "down" });
                j.raw(b",\"running\":").bool(lan_up);
                j.raw(b",\"rx_packets\":")
                    .u32(rt.lan_rx)
                    .raw(b",\"tx_packets\":")
                    .u32(rt.lan_tx)
                    .raw(b",\"editable\":")
                    .bool(false)
                    .raw(b"}");
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
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"default_policy\":")
                .quoted(rt.default_policy)
                .raw(b",\"tcam_rules\":")
                .u32(rt.tcam_rules)
                .raw(b",\"dnat_rules\":")
                .u32(rt.dnat_rules)
                // Perlindungan hanya boleh dilaporkan bila classifier benar-benar
                // ditanyakan pada frame yang lewat; mesin TCAM yang berdiri
                // sendiri bukan tembok.
                .raw(b",\"wan_protected\":")
                .bool(rt.wan_up && rt.firewall_wired)
                .raw(b",\"rules_enforced\":")
                .bool(rt.firewall_wired)
                .raw(b",\"datapath\":");
            j.quoted(if rt.firewall_wired {
                "wired"
            } else {
                "engine-only"
            });
            j.raw(b",\"http_port_open\":")
                .bool(rt.http_listening)
                .raw(b",\"http_port\":")
                .u32(rt.http_port as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON firewall overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Ringkasan NAT: angka dari `NatEngine` kernel, plus pengakuan jujur apakah
    /// mesin itu benar-benar dipanggil jalur forwarding.
    fn handle_nat_summary(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"masquerade_active\":")
                .bool(rt.wan_up && rt.wan_ip != [0, 0, 0, 0])
                .raw(b",\"masquerade_ip\":")
                .quoted_ip(rt.wan_ip)
                .raw(b",\"forwarding_wired\":")
                .bool(rt.nat_forwarding_wired)
                .raw(b",\"forwarding\":");
            j.quoted(if rt.nat_forwarding_wired {
                "wired"
            } else {
                "engine-only"
            });
            j.raw(b",\"active_sessions\":")
                .u32(rt.nat_session_count)
                .raw(b",\"snapshot_rows\":")
                .u32(rt.nat_sessions.len() as u32)
                .raw(b",\"capacity\":")
                .u32(rt.nat_capacity)
                .raw(b",\"table_full_drops\":")
                .u32(rt.nat_table_full_drops)
                .raw(b",\"dnat_rules\":")
                .u32(rt.dnat_rules)
                .raw(b"}}");
            j.finish().ok_or("JSON nat overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_nat_rules(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 1024];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            for (i, r) in rt.dnat_list.iter().enumerate() {
                if i > 0 {
                    j.raw(b",");
                }
                j.raw(b"{\"wan_port\":")
                    .u32(r.wan_port as u32)
                    .raw(b",\"protocol\":")
                    .quoted(r.protocol)
                    .raw(b",\"target_ip\":")
                    .quoted_ip(r.target_ip)
                    .raw(b",\"target_port\":")
                    .u32(r.target_port as u32)
                    .raw(b",\"active\":")
                    .bool(r.active);
                j.raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON nat rules overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_nat_conntrack(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 2048];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            for (i, c) in rt.nat_sessions.iter().enumerate() {
                if i > 0 {
                    j.raw(b",");
                }
                j.raw(b"{\"protocol\":").quoted(c.protocol);
                j.raw(b",\"src\":").quoted_ip(c.src_ip);
                j.raw(b",\"src_port\":").u32(c.src_port as u32);
                j.raw(b",\"dst\":").quoted_ip(c.dst_ip);
                j.raw(b",\"dst_port\":").u32(c.dst_port as u32);
                j.raw(b",\"translated_port\":")
                    .u32(c.trans_port as u32)
                    .raw(b",\"fwd_packets\":")
                    .u32(c.fwd_packets)
                    .raw(b",\"rev_packets\":")
                    .u32(c.rev_packets);
                j.raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON nat conntrack overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_services(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 768];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"dhcp_client\":")
                .bool(rt.dhcp_client_active)
                .raw(b",\"wan_mode\":");
            j.quoted(if rt.wan_config_static {
                "static"
            } else {
                "dhcp"
            });
            j.raw(b",\"http_server\":")
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
                .raw(b",\"write_queue_depth\":")
                .u32(self.write_queue_len as u32)
                .raw(b",\"writes_rejected\":")
                .u32(self.writes_rejected)
                .raw(b",\"power_armed\":")
                .bool(self.power_armed(rt.tick_100ms))
                .raw(b",\"power_arm_left_s\":")
                .u32(self.power_arm_left_s(rt.tick_100ms))
                .raw(b",\"power_pending\":")
                .bool(self.power_pending.is_some())
                .raw(b",\"http_bind_ip\":")
                .quoted_ip(rt.wan_ip)
                .raw(b"}}");
            j.finish().ok_or("JSON services overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Status perintah tulis terakhir. POST hanya menjanjikan "masuk antrean";
    /// hasil sebenarnya (diterapkan/ditolak kernel, plus nasib persistensi
    /// RJCFG01) baru ada setelah poll loop mengambil antrean, jadi modal WebUI
    /// membaca endpoint ini alih-alih menebak dari respons POST.
    fn handle_write_status(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 256];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"applied\":")
                .bool(!rt.last_write_report.is_empty())
                .raw(b",\"queue_depth\":")
                .u32(self.write_queue_len as u32)
                .raw(b",\"ok\":")
                .bool(rt.last_write_ok)
                .raw(b",\"report\":")
                .quoted(rt.last_write_report)
                .raw(b"}}");
            j.finish().ok_or("JSON write status overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Sisi nirkabel build ini: TIDAK ada driver radio. Endpoint mengembalikan
    /// nol perangkat secara eksplisit agar UI tidak pernah menampilkan baris
    /// karangan; status `operational` sengaja `false` (bukan klaim crate marker).
    fn handle_wireless(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        let mut body = [0u8; 256];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"radios\":0,\"stations\":0,\"operational\":")
                .bool(false)
                .raw(b",\"reason\":")
                .quoted("no wireless driver in this build")
                .raw(b"}}");
            j.finish().ok_or("JSON wireless overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_wireless_devices(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        Self::ok_json(out, b"{\"ok\":true,\"data\":[]}")
    }

    /// vOlt: crate `asl-router-olt-carrier` / `asl-router-gpon-ont` hanyalah
    /// marker 40 baris dengan `is_operational()` bernilai tetap `true`. Laporan
    /// ini tidak menelan klaim itu: ONU = 0 dan scheduler belum terhubung.
    fn handle_olt(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        let mut body = [0u8; 384];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"ons\":0,\"ports\":0,\"operational\":")
                .bool(false)
                .raw(b",\"scheduler_connected\":")
                .bool(false)
                .raw(b",\"reason\":")
                .quoted("gpon/olt crates are markers without datapath")
                .raw(b",\"marker_crates\":")
                .quoted("asl-router-gpon-ont, asl-router-olt-carrier")
                .raw(b"}}");
            j.finish().ok_or("JSON olt overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_olt_ons(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        Self::ok_json(out, b"{\"ok\":true,\"data\":[]}")
    }

    fn build_write_cmd(
        &self,
        iface: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<WebWriteCmd, &'static str> {
        let ct = content_type.ok_or("missing_content_type")?;
        if !starts_with_ci(ct, "application/x-www-form-urlencoded") {
            return Err("unsupported_content_type");
        }
        if body.len() > WEB_WRITE_BODY_MAX {
            return Err("body_too_large");
        }
        if !iface_name_ok(iface) {
            return Err("target_invalid");
        }
        let form = Form::parse(body);
        let mode_s = form.get("mode").ok_or("missing_mode")?;
        let mode = match mode_s {
            "dhcp" => WAN_MODE_DHCP,
            "static" => WAN_MODE_STATIC,
            _ => return Err("mode_invalid"),
        };
        let new_name = form.get("name").unwrap_or(iface);
        if !iface_name_ok(new_name) {
            return Err("name_invalid");
        }

        let mut cmd = WebWriteCmd {
            target: [0u8; IFACE_NAME_MAX],
            target_len: 0,
            new_name: [0u8; IFACE_NAME_MAX],
            new_name_len: 0,
            mode,
            ip: [0; 4],
            netmask: [0; 4],
            gw: [0; 4],
            persist: form.get("persist") == Some("1"),
        };
        put_name(iface, &mut cmd.target, &mut cmd.target_len);
        put_name(new_name, &mut cmd.new_name, &mut cmd.new_name_len);

        let ip_s = form.get("ip").unwrap_or("");
        let mask_s = form.get("netmask").unwrap_or("");
        let gw_s = form.get("gateway").unwrap_or("");

        if mode == WAN_MODE_DHCP {
            // Aturan DHCP vs statik: mode DHCP tidak boleh membawa alamat
            // tulisan tangan; kalau operator ingin alamat tetap, mode statik.
            if !ip_s.is_empty() || !mask_s.is_empty() || !gw_s.is_empty() {
                return Err("dhcp_rejects_manual_address");
            }
            return Ok(cmd);
        }

        if ip_s.is_empty() {
            return Err("static_requires_ip");
        }
        if mask_s.is_empty() {
            return Err("static_requires_netmask");
        }
        let ip = parse_dotted(ip_s).ok_or("ip_invalid")?;
        let mask = parse_dotted(mask_s).ok_or("netmask_invalid")?;
        let prefix = mask_prefix(mask).ok_or("netmask_not_contiguous")?;
        if !(8..=30).contains(&prefix) {
            return Err("netmask_prefix_out_of_range");
        }
        if is_reserved(ip) {
            return Err("ip_reserved_or_loopback");
        }
        let net = subnet_of(ip, mask);
        if ip == net {
            return Err("ip_is_network_address");
        }
        if ip == broadcast_of(ip, mask) {
            return Err("ip_is_broadcast_address");
        }
        cmd.ip = ip;
        cmd.netmask = mask;
        if !gw_s.is_empty() {
            let gw = parse_dotted(gw_s).ok_or("gateway_invalid")?;
            if is_reserved(gw) {
                return Err("gateway_reserved_or_loopback");
            }
            if subnet_of(gw, mask) != net {
                return Err("gateway_outside_subnet");
            }
            if gw == ip {
                return Err("gateway_equals_own_ip");
            }
            if gw == broadcast_of(ip, mask) {
                return Err("gateway_is_broadcast_address");
            }
            cmd.gw = gw;
        }
        Ok(cmd)
    }
}

// -----------------------------------------------------------------------------
// VALIDATOR MURN (tanpa state, dipakai dispatcher maupun unit test)
// -----------------------------------------------------------------------------

/// Pisahkan `host:port` versi IPv4/label; `None` bila port tidak ada.
fn split_host_port(host: &str) -> (&str, Option<u16>) {
    match host.rsplit_once(':') {
        Some((name, port)) => {
            let mut v: u32 = 0;
            let mut digits = 0usize;
            for b in port.bytes() {
                if !b.is_ascii_digit() {
                    return (host, None);
                }
                v = v.saturating_mul(10).saturating_add((b - b'0') as u32);
                digits += 1;
            }
            if digits == 0 || v > 65535 {
                return (host, None);
            }
            (name, Some(v as u16))
        }
        None => (host, None),
    }
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    let (a, b) = (s.as_bytes(), prefix.as_bytes());
    if a.len() < b.len() {
        return false;
    }
    for i in 0..b.len() {
        if a[i].to_ascii_lowercase() != b[i].to_ascii_lowercase() {
            return false;
        }
    }
    true
}

/// Urai "a.b.c.d" menjadi oktet; tolak digit kosong, >3 angka, dan nilai >255.
pub fn parse_dotted(s: &str) -> Option<[u8; 4]> {
    let mut oct = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u32 = 0;
    let mut digits = 0usize;
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            if digits == 0 || idx >= 4 {
                return None;
            }
            oct[idx] = cur as u8;
            idx += 1;
            cur = 0;
            digits = 0;
        } else if b.is_ascii_digit() {
            if digits == 3 {
                return None;
            }
            cur = cur * 10 + (b - b'0') as u32;
            digits += 1;
            if cur > 255 {
                return None;
            }
        } else {
            return None;
        }
    }
    if digits == 0 || idx != 3 {
        return None;
    }
    oct[3] = cur as u8;
    Some(oct)
}

/// Panjang prefix bila netmask kontigu (mis. 255.255.254.0 => 23), else None.
pub fn mask_prefix(mask: [u8; 4]) -> Option<u8> {
    let bits = u32::from(mask[0]) << 24
        | u32::from(mask[1]) << 16
        | u32::from(mask[2]) << 8
        | u32::from(mask[3]);
    let ones = bits.count_ones() as u8;
    if ones == 0 {
        return None;
    }
    let expected = if ones >= 32 {
        u32::MAX
    } else {
        !(u32::MAX >> ones)
    };
    if bits == expected {
        Some(ones)
    } else {
        None
    }
}

fn to_u32(ip: [u8; 4]) -> u32 {
    u32::from(ip[0]) << 24 | u32::from(ip[1]) << 16 | u32::from(ip[2]) << 8 | u32::from(ip[3])
}

fn from_u32(v: u32) -> [u8; 4] {
    [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn subnet_of(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    from_u32(to_u32(ip) & to_u32(mask))
}

fn broadcast_of(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    from_u32(to_u32(ip) | !to_u32(mask))
}

/// Alamat yang tidak boleh dipakai host WAN: this-net, loopback, link-local,
/// multicast, dan reserved.
fn is_reserved(ip: [u8; 4]) -> bool {
    ip[0] == 0 || ip[0] == 127 || ip[0] >= 224 || (ip[0] == 169 && ip[1] == 254)
}

fn put_name(src: &str, dst: &mut [u8; IFACE_NAME_MAX], len: &mut u8) {
    for b in dst.iter_mut() {
        *b = 0;
    }
    let n = core::cmp::min(src.len(), IFACE_NAME_MAX);
    dst[..n].copy_from_slice(&src.as_bytes()[..n]);
    *len = n as u8;
}

/// Pengurai body `application/x-www-form-urlencoded` dengan kapasitas tetap.
/// Nilai tidak didekode percent-encoding: karakter `%`/`+` akan ditolak
/// validator field terkait, sehingga kegagalan terdengar, tidak diam-diam.
struct Form<'a> {
    keys: [&'a str; 8],
    vals: [&'a str; 8],
    len: usize,
}

impl<'a> Form<'a> {
    fn parse(body: &'a [u8]) -> Self {
        let mut f = Form {
            keys: [""; 8],
            vals: [""; 8],
            len: 0,
        };
        let text = match core::str::from_utf8(body) {
            Ok(t) => t,
            Err(_) => return f,
        };
        for pair in text.split('&') {
            if pair.is_empty() || f.len >= f.keys.len() {
                continue;
            }
            if let Some((k, v)) = pair.split_once('=') {
                f.keys[f.len] = k;
                f.vals[f.len] = v;
                f.len += 1;
            }
        }
        f
    }

    fn get(&self, key: &str) -> Option<&'a str> {
        for i in 0..self.len {
            if self.keys[i] == key {
                return Some(self.vals[i]);
            }
        }
        None
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

    static DNAT: [DnatSnapshot; 1] = [DnatSnapshot {
        wan_port: 7777,
        protocol: "udp",
        target_ip: [192, 0, 2, 11],
        target_port: 7777,
        active: true,
    }];

    fn rt() -> WebRuntime<'static> {
        WebRuntime {
            hostname: "rinjani-wan01",
            uptime_ms: 123456,
            wan_present: true,
            wan_name: "eth0",
            lan_name: "eth1",
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
            wan_config_static: false,
            dns_active: false,
            mdns_active: false,
            serial_cli_active: true,
            tcam_rules: 16,
            dnat_rules: 1,
            default_policy: "deny",
            dnat_list: &DNAT,
            nat_sessions: &[],
            nat_session_count: 0,
            nat_capacity: 128,
            nat_table_full_drops: 0,
            nat_forwarding_wired: false,
            firewall_wired: false,
            last_write_ok: false,
            last_write_report: "",
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
        assert!(resp.contains("\"role\":\"wan\""));
        assert!(resp.contains("\"ip\":\"198.51.100.77\""));
        assert!(resp.contains("\"gateway\":\"198.51.100.254\""));
        assert!(resp.contains("\"mac\":\"08:00:27:11:22:33\""));
        assert!(resp.contains("\"rx_packets\":41"));
        assert!(resp.contains("\"config_mode\":\"dhcp\""));
    }

    #[test]
    fn test_03a_api_interfaces_never_invents_absent_nic() {
        // Kontrak baru: satu NIC terdeteksi => tepat satu baris. Tidak ada
        // placeholder eth1 berstatus `absent` lagi.
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.lan_present = false;
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            !resp.contains("eth1"),
            "NIC tak terdeteksi tidak boleh muncul"
        );
        assert!(!resp.contains("absent"));
        assert_eq!(resp.matches("\"name\":").count(), 1);
    }

    #[test]
    fn test_03c_api_interfaces_empty_when_no_nic_present() {
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.wan_present = false;
        r.lan_present = false;
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            resp.contains("\"data\":[]"),
            "tidak ada NIC => array kosong"
        );
    }

    #[test]
    fn test_03d_kernel_interface_names_are_used() {
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.wan_name = "wan-uplink";
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("\"name\":\"wan-uplink\""));
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

    // Jejak cacat yang ditemukan di browser nyata: kurung penutup objek baris
    // WAN tidak ditulis, JSON.parse browser gagal, dan tabel Interfaces diam
    // tanpa pesan. Semua body GET dijaga seimbang agar kelas cacat ini tidak
    // bisa kembali lolos.
    fn json_pairs_balanced(s: &str) -> Result<(), &'static str> {
        let mut stack = [0u8; 16];
        let mut depth = 0usize;
        let mut in_str = false;
        let mut esc = false;
        for &b in s.as_bytes() {
            if in_str {
                if esc {
                    esc = false;
                } else if b == b'\\' {
                    esc = true;
                } else if b == b'"' {
                    in_str = false;
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'{' | b'[' => {
                    if depth == stack.len() {
                        return Err("nested terlalu dalam");
                    }
                    stack[depth] = b;
                    depth += 1;
                }
                b'}' | b']' => {
                    if depth == 0 {
                        return Err("penutup tanpa pembuka");
                    }
                    depth -= 1;
                    let want = if b == b'}' { b'{' } else { b'[' };
                    if stack[depth] != want {
                        return Err("pasangan tidak cocok");
                    }
                }
                _ => {}
            }
        }
        if in_str || depth != 0 {
            return Err("terpotong");
        }
        Ok(())
    }

    static CONN1: [ConnSnapshot; 1] = [ConnSnapshot {
        protocol: "udp",
        src_ip: [192, 0, 2, 20],
        src_port: 34567,
        dst_ip: [198, 51, 100, 9],
        dst_port: 53,
        trans_port: 20480,
        fwd_packets: 4,
        rev_packets: 3,
    }];

    #[test]
    fn test_03e_every_get_body_is_parseable_json() {
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.lan_present = true;
        r.lan_ip = [198, 51, 100, 1];
        r.lan_mac = [1, 2, 3, 4, 5, 6];
        r.nat_sessions = &CONN1;
        r.nat_session_count = 1;
        r.last_write_ok = true;
        r.last_write_report = "wan0 applied";
        let reqs: [&'static [u8]; 12] = [
            b"GET /api/v1/system HTTP/1.1\r\n\r\n",
            b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n",
            b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n",
            b"GET /api/v1/nat HTTP/1.1\r\n\r\n",
            b"GET /api/v1/nat/rules HTTP/1.1\r\n\r\n",
            b"GET /api/v1/nat/conntrack HTTP/1.1\r\n\r\n",
            b"GET /api/v1/wireless HTTP/1.1\r\n\r\n",
            b"GET /api/v1/wireless/devices HTTP/1.1\r\n\r\n",
            b"GET /api/v1/olt HTTP/1.1\r\n\r\n",
            b"GET /api/v1/olt/ons HTTP/1.1\r\n\r\n",
            b"GET /api/v1/services HTTP/1.1\r\n\r\n",
            b"GET /api/v1/writes/last HTTP/1.1\r\n\r\n",
        ];
        for (i, raw) in reqs.iter().enumerate() {
            let req = path(raw);
            let mut buf = [0u8; 2400];
            let len = dispatcher
                .dispatch(&req, (i as u64) * 2, &mut buf, &r)
                .unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            let body = resp.split_once("\r\n\r\n").map_or("", |x| x.1);
            assert!(
                body.starts_with("{\"ok\":true"),
                "{} tidak memulai sampul ok:true: {}",
                req.path,
                body
            );
            json_pairs_balanced(body).unwrap_or_else(|e| panic!("{}: {} — {}", req.path, e, body));
        }
    }

    #[test]
    fn test_03f_interfaces_rows_are_closed_objects() {
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.lan_present = true;
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        // Setiap baris harus berakhir `true}` / `false}` sebelum pemisah atau
        // penutup array; ini persis bentuk yang dulu bocor satu kurung.
        assert!(resp.contains("\"editable\":true},"));
        assert!(resp.contains("\"editable\":false}]}"));
        assert_eq!(resp.matches("\"name\":").count(), 2);
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
    fn test_04b_firewall_protected_requires_datapath() {
        // Jumlah aturan bukan bukti penegakan. Selama classifier tidak pernah
        // ditanya pada frame yang lewat, pelaporan "WAN terlindungi: YA" adalah
        // klaim keamanan yang salah.
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];

        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"wan_protected\":false"));
        assert!(resp.contains("\"rules_enforced\":false"));
        assert!(resp.contains("\"datapath\":\"engine-only\""));

        let mut wired = rt();
        wired.firewall_wired = true;
        let len = dispatcher.dispatch(&req, 0, &mut buf, &wired).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"wan_protected\":true"));
        assert!(resp.contains("\"rules_enforced\":true"));
        assert!(resp.contains("\"datapath\":\"wired\""));
    }

    #[test]
    fn test_04c_lan_status_datang_dari_alamat_bukan_literal() {
        // Regresi §5: interface LAN tanpa alamat tidak boleh lagi dilaporkan
        // berjalan hanya karena barisnya tercetak.
        let mut dispatcher = ApiDispatcher::new();
        let mut r = rt();
        r.lan_present = true;
        r.lan_ip = [0, 0, 0, 0];
        let req = path(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 2048];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("\"role\":\"lan\""));
        assert!(resp.contains("\"status\":\"down\""));
        assert!(resp.contains("\"running\":false"));

        r.lan_ip = [192, 168, 56, 100];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"status\":\"up\""));
        assert!(resp.contains("\"running\":true"));
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

    #[test]
    fn test_10_nat_summary_admits_engine_only_forwarding() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/nat HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        // rt() memakai alamat test-net, bukan IP produksi.
        assert!(resp.contains("\"masquerade_ip\":\"198.51.100.77\""));
        assert!(resp.contains("\"forwarding\":\"engine-only\""));
        assert!(resp.contains("\"forwarding_wired\":false"));
        assert!(resp.contains("\"capacity\":128"));
        assert!(resp.contains("\"active_sessions\":0"));
    }

    #[test]
    fn test_11_nat_rules_come_from_snapshot() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/nat/rules HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"wan_port\":7777"));
        assert!(resp.contains("\"target_ip\":\"192.0.2.11\""));
        assert!(resp.contains("\"protocol\":\"udp\""));
    }

    #[test]
    fn test_12_wireless_and_olt_report_zero_rows() {
        let mut dispatcher = ApiDispatcher::new();
        let mut buf = [0u8; 512];
        let req = path(b"GET /api/v1/wireless HTTP/1.1\r\n\r\n");
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"radios\":0"));
        assert!(resp.contains("\"operational\":false"));

        let req = path(b"GET /api/v1/olt/ons HTTP/1.1\r\n\r\n");
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("{\"ok\":true,\"data\":[]}"));
    }

    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::format;

    /// Susun permintaan POST utuh dengan Content-Length yang benar. Bocor
    /// (leak) disengaja: hanya terjadi di test host,umur data dibuat 'static.
    fn post(body: &str) -> HttpRequest<'static> {
        let head = format!(
            "POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let raw: &'static mut str = Box::leak((head + body).into_boxed_str());
        HttpRequest::parse(raw.as_bytes()).unwrap()
    }

    #[test]
    fn test_13_post_static_queues_command() {
        let mut dispatcher = ApiDispatcher::new();
        let req = post("mode=static&ip=198.51.100.30&netmask=255.255.255.0&gateway=198.51.100.1&name=eth0&persist=1");
        let mut buf = [0u8; 512];
        let len = dispatcher
            .dispatch_post(&req, &mut buf, &rt())
            .expect("POST harus terlayani");
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"queued\""));
        assert!(resp.contains("\"persist\":true"));

        let cmd = dispatcher.take_write().expect("perintah masuk antrean");
        assert_eq!(cmd.target_str(), "eth0");
        assert_eq!(cmd.mode, WAN_MODE_STATIC);
        assert_eq!(cmd.ip, [198, 51, 100, 30]);
        assert_eq!(cmd.gw, [198, 51, 100, 1]);
        assert!(cmd.persist);
        assert!(dispatcher.take_write().is_none());
    }

    #[test]
    fn test_14_post_dhcp_mode_is_accepted_and_clears() {
        let mut dispatcher = ApiDispatcher::new();
        let req = post("mode=dhcp");
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("\"mode\":\"dhcp\""));
        let cmd = dispatcher.take_write().unwrap();
        assert_eq!(cmd.mode, WAN_MODE_DHCP);
        assert_eq!(cmd.ip, [0, 0, 0, 0]);
        // Nama tidak dikirim => tetap nama berjalan.
        assert_eq!(cmd.new_name_str(), "eth0");
    }

    #[test]
    fn test_15_post_rejects_dhcp_with_manual_address() {
        let mut dispatcher = ApiDispatcher::new();
        let req = post("mode=dhcp&ip=198.51.100.30");
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 400 Bad Request"));
        assert!(resp.contains("\"error\":\"dhcp_rejects_manual_address\""));
        assert!(dispatcher.take_write().is_none());
        assert_eq!(dispatcher.writes_rejected, 1);
    }

    #[test]
    fn test_16_post_address_validation_matrix() {
        let cases: [(&str, &str); 8] = [
            ("mode=static", "static_requires_ip"),
            ("mode=static&ip=198.51.100.9", "static_requires_netmask"),
            (
                "mode=static&ip=198.51.100.9&netmask=255.0.255.0",
                "netmask_not_contiguous",
            ),
            (
                "mode=static&ip=198.51.100.9&netmask=255.255.255.0&gateway=10.9.8.7",
                "gateway_outside_subnet",
            ),
            (
                "mode=static&ip=198.51.100.0&netmask=255.255.255.0",
                "ip_is_network_address",
            ),
            (
                "mode=static&ip=198.51.100.255&netmask=255.255.255.0",
                "ip_is_broadcast_address",
            ),
            (
                "mode=static&ip=127.0.0.1&netmask=255.0.0.0",
                "ip_reserved_or_loopback",
            ),
            (
                "mode=static&ip=198.51.100.9&netmask=255.255.255.0&name=BAD%20NAME",
                "name_invalid",
            ),
        ];
        for (body, expected) in cases {
            let mut dispatcher = ApiDispatcher::new();
            let req = post(body);
            let mut buf = [0u8; 512];
            let len = dispatcher
                .dispatch_post(&req, &mut buf, &rt())
                .expect("respons harus tersusun");
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(
                resp.contains(expected),
                "body `{}` harus ditolak `{}`, dapat: {}",
                body,
                expected,
                resp
            );
            assert!(dispatcher.take_write().is_none());
        }
    }

    #[test]
    fn test_17_post_requires_matching_host_and_content_type() {
        // Host beda asal => 403 (proteksi CSRF), tanpa menyentuh antrean.
        let mut dispatcher = ApiDispatcher::new();
        let raw = b"POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 203.0.113.9\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 9\r\n\r\nmode=dhcp";
        let req = HttpRequest::parse(raw).unwrap();
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(resp.contains("\"error\":\"host_mismatch\""));

        // Content-type salah => 400.
        let raw = b"POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/json\r\nContent-Length: 9\r\n\r\nmode=dhcp";
        let req = HttpRequest::parse(raw).unwrap();
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"unsupported_content_type\""));
    }

    #[test]
    fn test_18_write_queue_capacity_is_honest() {
        let mut dispatcher = ApiDispatcher::new();
        let mut buf = [0u8; 512];
        for _ in 0..WEB_WRITE_QUEUE {
            let req = post("mode=dhcp");
            let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
            assert!(core::str::from_utf8(&buf[..len])
                .unwrap()
                .contains("HTTP/1.1 200 OK"));
        }
        let req = post("mode=dhcp");
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 500 Internal Server Error"));
        assert!(resp.contains("\"error\":\"write_queue_full\""));
    }

    #[test]
    fn test_19_pure_validators() {
        assert_eq!(parse_dotted("198.51.100.77"), Some([198, 51, 100, 77]));
        assert_eq!(parse_dotted("198.51.100.256"), None);
        assert_eq!(parse_dotted("198.51.100"), None);
        assert_eq!(parse_dotted("198.51.100.77.5"), None);
        assert_eq!(parse_dotted("1.2.3.4 "), None);
        assert_eq!(mask_prefix([255, 255, 254, 0]), Some(23));
        assert_eq!(mask_prefix([255, 0, 255, 0]), None);
        assert_eq!(mask_prefix([0, 0, 0, 0]), None);
        assert_eq!(split_host_port("10.0.0.1:8080"), ("10.0.0.1", Some(8080)));
        assert_eq!(split_host_port("rinjani"), ("rinjani", None));
        assert!(starts_with_ci(
            "Application/X-WWW-Form-Urlencoded; charset=utf-8",
            "application/x-www-form-urlencoded"
        ));
    }

    #[test]
    fn test_20_write_status_reports_kernel_verdict() {
        let mut dispatcher = ApiDispatcher::new();
        let mut buf = [0u8; 512];
        let req = path(b"GET /api/v1/writes/last HTTP/1.1\r\n\r\n");

        // Belum ada perintah yang pernah diterapkan kernel: tidak ada laporan,
        // jadi UI tidak boleh menampilkan pesan keberhasilan.
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"applied\":false"));
        assert!(resp.contains("\"report\":\"\""));

        // Setelah poll loop menerapkan perintah, verdict kernel terbaca apa
        // adanya termasuk alasan penolakannya.
        let mut r = rt();
        r.last_write_ok = true;
        r.last_write_report = "statik diterapkan, probe ARP DAD TX";
        let len = dispatcher.dispatch(&req, 1, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"applied\":true"));
        assert!(resp.contains("probe ARP DAD TX"));

        let mut r = rt();
        r.last_write_ok = false;
        r.last_write_report = "target 'eth9' bukan antarmuka WAN terdeteksi";
        let len = dispatcher.dispatch(&req, 2, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"ok\":false"));
        assert!(resp.contains("bukan antarmuka WAN"));
    }

    /// POST `/api/v1/power` dengan Content-Length yang benar.
    fn post_power(body: &str) -> HttpRequest<'static> {
        let head = format!(
            "POST /api/v1/power HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let raw: &'static mut str = Box::leak((head + body).into_boxed_str());
        HttpRequest::parse(raw.as_bytes()).unwrap()
    }

    fn power_post(dispatcher: &mut ApiDispatcher, body: &str, tick: u64) -> alloc::string::String {
        let req = post_power(body);
        let mut r = rt();
        r.tick_100ms = tick;
        let mut buf = [0u8; 512];
        let len = dispatcher
            .dispatch_post(&req, &mut buf, &r)
            .expect("POST /api/v1/power harus terlayani");
        alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap())
    }

    #[test]
    fn test_21_power_requires_arm_token_before_action() {
        let mut dispatcher = ApiDispatcher::new();

        // Aksi tanpa slot ter-arm => 403 dan tidak ada perintah tertunda.
        let resp = power_post(&mut dispatcher, "action=restart", 0);
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(resp.contains("\"error\":\"power_not_armed"));
        assert!(dispatcher.take_power().is_none());
        assert!(!dispatcher.power_armed(0));

        // Arm menyalakan slot dan ACK menyebut masa berlakunya.
        let resp = power_post(&mut dispatcher, "action=arm", 10);
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"armed\""));
        assert!(resp.contains("\"valid_for_s\":20"));
        assert!(dispatcher.power_armed(10));
        assert!(dispatcher.power_armed(209));
        assert_eq!(dispatcher.power_arm_left_s(200), 1);

        // Satu aksi memakai slot sampai habis; aksi kedua harus ditolak.
        let resp = power_post(&mut dispatcher, "action=shutdown", 20);
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"scheduled\""));
        assert!(resp.contains("\"action\":\"shutdown\""));
        assert!(resp.contains("\"exec_in_ms\":1500"));
        assert_eq!(dispatcher.take_power(), Some(POWER_SHUTDOWN));
        assert!(!dispatcher.power_armed(21));
        assert!(dispatcher.take_power().is_none());

        let resp = power_post(&mut dispatcher, "action=restart", 22);
        assert!(resp.contains("\"error\":\"power_not_armed\""));
        assert!(dispatcher.take_power().is_none());
    }

    #[test]
    fn test_22_power_arm_expires_after_window() {
        let mut dispatcher = ApiDispatcher::new();
        power_post(&mut dispatcher, "action=arm", 100);
        assert!(dispatcher.power_armed(299));
        assert!(!dispatcher.power_armed(100 + POWER_ARM_TICKS));

        // Kedaluwarsa bukan hanya penolakan: slot dibersihkan, jadi permintaan
        // susulan pada tick yang sama tidak bisa 'menumpang' arm lama.
        let resp = power_post(&mut dispatcher, "action=restart", 100 + POWER_ARM_TICKS);
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(dispatcher.take_power().is_none());
        assert!(!dispatcher.power_armed(100 + POWER_ARM_TICKS + 1));
    }

    #[test]
    fn test_23_power_rejects_bad_input_without_touching_slot() {
        let mut dispatcher = ApiDispatcher::new();
        power_post(&mut dispatcher, "action=arm", 0);

        let resp = power_post(&mut dispatcher, "action=hibernate", 1);
        assert!(resp.contains("HTTP/1.1 400"));
        assert!(resp.contains("\"error\":\"bad_power_action\""));

        let resp = power_post(&mut dispatcher, "mode=dhcp", 2);
        assert!(resp.contains("\"error\":\"missing_action\""));

        // Aksi liar tidak boleh menghabiskan hak arm yang sedang berlaku.
        assert!(dispatcher.power_armed(3));
        let resp = power_post(&mut dispatcher, "action=restart", 4);
        assert!(resp.contains("\"status\":\"scheduled\""));
        assert_eq!(dispatcher.take_power(), Some(POWER_RESTART));
    }

    #[test]
    fn test_24_power_post_content_type_and_host_are_enforced() {
        // Content-type salah => ditolak sebelum slot daya disentuh.
        let mut dispatcher = ApiDispatcher::new();
        let raw = b"POST /api/v1/power HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\naction=arm";
        let req = HttpRequest::parse(raw).unwrap();
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"unsupported_content_type\""));
        assert!(!dispatcher.power_armed(0));

        // Host asing => 403 host_mismatch; jalur daya tidak ditembus.
        let raw = b"POST /api/v1/power HTTP/1.1\r\nHost: 203.0.113.9\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 11\r\n\r\naction=arm";
        let req = HttpRequest::parse(raw).unwrap();
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(resp.contains("\"error\":\"host_mismatch\""));
        assert!(!dispatcher.power_armed(0));
    }

    #[test]
    fn test_25_services_advertises_power_slot_state() {
        let mut dispatcher = ApiDispatcher::new();
        let req = path(b"GET /api/v1/services HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"power_armed\":false"));
        assert!(resp.contains("\"power_arm_left_s\":0"));
        assert!(resp.contains("\"power_pending\":false"));

        power_post(&mut dispatcher, "action=arm", 50);
        let mut r = rt();
        r.tick_100ms = 50;
        let len = dispatcher.dispatch(&req, 1, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"power_armed\":true"));
        assert!(resp.contains("\"power_arm_left_s\":20"));
    }
}
