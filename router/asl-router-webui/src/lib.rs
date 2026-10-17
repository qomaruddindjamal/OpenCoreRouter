//! `asl-router-webui` — Embedded WebUI Frontend & Asset Dispatcher
//!
//! Exposes embedded unminified web assets and handles static resource dispatch.
//!
//! Modul ini juga memuat [`TcpWebServer`]: server TCP/HTTP bare-metal native ASL
//! (RFC 9293 + RFC 7230) yang menyajikan aset & REST API di atas jalur DMA
//! virtio-net nyata. Tidak ada dependensi smoltcp maupun kode Linux.

#![no_std]

pub mod registry;

use asl_router_api::{ApiDispatcher, WebRuntime};
use asl_router_netstack::{
    build_tcp_frame, parse_ethernet_header, parse_ipv4_header, parse_tcp_header, tcp_checksum_ok,
    TcpSocketState, ETHERTYPE_IPV4, IP_PROTO_TCP, TCP_FLAG_ACK, TCP_FLAG_FIN, TCP_FLAG_PSH,
    TCP_FLAG_RST, TCP_FLAG_SYN, TCP_MAX_PAYLOAD,
};
use asl_router_webcore::{
    declared_body_len, response_status, serve_static, HttpMethod, HttpRequest, HttpResponseBuilder,
    HttpStatus,
};

pub fn dispatch_static(
    req: &HttpRequest,
    out_buf: &mut [u8],
) -> Option<Result<usize, &'static str>> {
    // Route shell: semua entri sidebar dilayani dari satu dokumen; router
    // client-side (app.js + views.js) yang memilih parsial view-nya.
    let path = if req.path == "/"
        || matches!(
            req.path,
            "/status"
                | "/interfaces"
                | "/firewall"
                | "/nat"
                | "/services"
                | "/arp"
                | "/serviceslist"
                | "/dnsserver"
                | "/gateway"
                | "/dhcpserver"
                | "/wireguard"
                | "/vpn"
                | "/xray"
                | "/qos"
                | "/wireless"
                | "/olt"
                | "/terminal"
                | "/appearance"
                | "/packages"
                | "/usermanager"
                | "/speedtest"
                | "/telnet"
                | "/traceroute"
        ) {
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

// -----------------------------------------------------------------------------
// EMBEDDED TCP + HTTP SERVER (RFC 9293 / RFC 7230) — NATIVE ASL, NO SMOLTCP
// -----------------------------------------------------------------------------

/// Jumlah sesi TCP paralel yang dapat dilayani. WebUI modular membuka banyak
/// aset sekaligus saat muat pertama (chrome + 4 CSS + 5 JS + logo + parsial
/// view); dengan 4 slot browser harus menunggu SYN yang dijatuhkan diulang,
/// sehingga halaman pertama terasa lambat.
pub const WEB_SESSIONS: usize = 8;
/// Batas bytes header permintaan yang dikumpulkan per sesi. Header browser
/// modern (sec-ch-ua, Accept-Encoding, Range) bisa melewati 1 KiB.
pub const WEB_REQ_MAX: usize = 2048;
/// Batas bytes respons yang dibangun per sesi. Harus menampung aset embedded
/// terbesar (parsial view / CSS) + header HTTP; lihat gerbang konstan di bawah.
pub const WEB_RESP_MAX: usize = 8192;
/// Window size yang kita iklankan ke peer.
pub const WEB_ANNOUNCED_WINDOW: u16 = 32768;
/// Panjang frame Ethernet minimum tanpa FCS. Frame lebih pendek dipadding agar
/// NIC/hypervisor tidak membuangnya.
pub const ETH_MIN_FRAME: usize = 60;
/// Ticks (100 ms) sesi boleh tertahan di FIN-WAIT / SYN-RECEIVED sebelum slot
/// didaur ulang. Mencegah kebocoran slot akibat peer yang tidak menutup socket.
pub const WEB_SESSION_EXPIRY_TICKS: u32 = 50;

/// Ukuran aset embedded terbesar, dihitung pada waktu compile.
const fn largest_asset_len() -> usize {
    let mut max = 0;
    let mut i = 0;
    while i < registry::ALL_ASSETS.len() {
        let len = registry::ALL_ASSETS[i].content.len();
        if len > max {
            max = len;
        }
        i += 1;
    }
    max
}

/// Budget header HTTP terburuk (status line + Security/ETag/Length/CType).
const HTTP_HEADER_BUDGET: usize = 256;

// Gerbang build: aset yang terlalu besar untuk buffer respons dulu membuat
// kernel menjawab 500 pada /js/status.js di hardware nyata. Sekarang ukuran
// yang tidak muat gagal kompilasi, bukan gagal di wire.
const _: () = assert!(largest_asset_len() + HTTP_HEADER_BUDGET <= WEB_RESP_MAX);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebSession {
    pub state: TcpSocketState,
    pub remote_mac: [u8; 6],
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    /// Nomor seq berikutnya yang kita harapkan dari peer.
    pub rcv_nxt: u32,
    /// Nomor seq berikutnya yang akan kita kirim.
    pub snd_nxt: u32,
    /// Batas bawah ruang kirim yang belum di-ACK peer.
    pub snd_una: u32,
    /// Window yang diiklankan peer pada segmen terakhir.
    pub peer_window: u16,
    pub req_len: usize,
    pub resp_len: usize,
    pub resp_sent: usize,
    pub fin_sent: bool,
    /// Sesi sudah punya respons (mencegah dispatch dua kali).
    pub responded: bool,
    /// Jumlah tick sesi menunggu penutupan (FIN-WAIT / SYN-RECEIVED).
    pub hold_ticks: u32,
}

impl WebSession {
    pub const fn closed() -> Self {
        Self {
            state: TcpSocketState::Closed,
            remote_mac: [0; 6],
            remote_ip: [0; 4],
            remote_port: 0,
            rcv_nxt: 0,
            snd_nxt: 0,
            snd_una: 0,
            peer_window: 0,
            req_len: 0,
            resp_len: 0,
            resp_sent: 0,
            fin_sent: false,
            responded: false,
            hold_ticks: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebStats {
    pub syn_received: u32,
    pub sessions_established: u32,
    pub requests_served: u32,
    pub bytes_sent: u32,
    pub segments_sent: u32,
    pub responses_404: u32,
    pub responses_405: u32,
    pub responses_400: u32,
    pub api_served: u32,
    pub checksum_errors: u32,
    pub out_of_order: u32,
    pub session_full_drops: u32,
    pub rst_sent: u32,
    pub tx_failures: u32,
}

/// Server WebUI di atas TCP native. State sesi + buffer berada dalam satu
/// struktur agar dapat dialokasikan kernel pada region non-DMA.
pub struct TcpWebServer {
    pub port: u16,
    pub listening: bool,
    pub sessions: [WebSession; WEB_SESSIONS],
    pub req: [[u8; WEB_REQ_MAX]; WEB_SESSIONS],
    pub resp: [[u8; WEB_RESP_MAX]; WEB_SESSIONS],
    /// Status terakhir HTTP yang dikirim (untuk telemetri CLI).
    pub last_status: u16,
    pub last_path: [u8; 64],
    pub last_path_len: u8,
    isn_state: u32,
    pub stats: WebStats,
}

impl TcpWebServer {
    pub const fn empty() -> Self {
        Self {
            port: 0,
            listening: false,
            sessions: [const { WebSession::closed() }; WEB_SESSIONS],
            req: [[0u8; WEB_REQ_MAX]; WEB_SESSIONS],
            resp: [[0u8; WEB_RESP_MAX]; WEB_SESSIONS],
            last_status: 0,
            last_path: [0u8; 64],
            last_path_len: 0,
            isn_state: 0x1985_1001,
            stats: WebStats {
                syn_received: 0,
                sessions_established: 0,
                requests_served: 0,
                bytes_sent: 0,
                segments_sent: 0,
                responses_404: 0,
                responses_405: 0,
                responses_400: 0,
                api_served: 0,
                checksum_errors: 0,
                out_of_order: 0,
                session_full_drops: 0,
                rst_sent: 0,
                tx_failures: 0,
            },
        }
    }

    pub const fn new(port: u16) -> Self {
        let mut s = Self::empty();
        s.port = port;
        s
    }

    pub fn listen(&mut self, port: u16) {
        self.port = port;
        self.listening = true;
    }

    pub fn active_sessions(&self) -> u32 {
        let mut n = 0u32;
        for s in self.sessions.iter() {
            if s.state != TcpSocketState::Closed {
                n += 1;
            }
        }
        n
    }

    /// ISN dari PRNG xorshift-multiply (LCG) — deterministik untuk uji unit dan
    /// cukup untuk urutan TCP, BUKAN entropy kriptografis. Dinyatakan jujur di
    /// laporan RND.
    fn next_isn(&mut self) -> u32 {
        self.isn_state = self
            .isn_state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345)
            .wrapping_add(0x9e37_79b9);
        self.isn_state >> 8
    }

    fn free_slot(&mut self) -> Option<usize> {
        for i in 0..WEB_SESSIONS {
            if self.sessions[i].state == TcpSocketState::Closed {
                return Some(i);
            }
        }
        None
    }

    fn find_slot(&self, ip: [u8; 4], port: u16) -> Option<usize> {
        for i in 0..WEB_SESSIONS {
            let s = &self.sessions[i];
            if s.state != TcpSocketState::Closed && s.remote_ip == ip && s.remote_port == port {
                return Some(i);
            }
        }
        None
    }

    /// Proses satu frame Ethernet masuk. `Ok(true)` = frame TCP milik server ini
    /// (sudah ditangani/dijatuhkan secara eksplisit); `Ok(false)` = bukan TCP,
    /// pemanggil boleh meneruskan ke responder ARP/ICMP.
    ///
    /// `scratch` adalah buffer frame TX (1518 B) milik pemanggil, sengaja di luar
    /// struktur ini agar kernel tidak menaruh 1.5 KiB di atas stack per-poll.
    pub fn on_frame(
        &mut self,
        frame: &[u8],
        rt: &WebRuntime,
        api: &mut ApiDispatcher,
        scratch: &mut [u8; 1518],
        tx: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
        log: &dyn Fn(&str),
    ) -> Result<bool, &'static str> {
        let (eth, eth_payload) = match parse_ethernet_header(frame) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if eth.ethertype != ETHERTYPE_IPV4 {
            return Ok(false);
        }
        let (ip, seg_all) = match parse_ipv4_header(eth_payload) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if ip.protocol != IP_PROTO_TCP {
            return Ok(false);
        }
        let (th, payload) = match parse_tcp_header(seg_all) {
            Ok(v) => v,
            Err(_) => {
                self.stats.checksum_errors += 1;
                return Ok(true);
            }
        };
        // Frame TCP yang ditujukan ke port lain bukan ranah server ini.
        if th.dst_port != self.port {
            return Ok(false);
        }
        if !self.listening || ip.dst_ip != rt.wan_ip {
            // Tidak ada binding (WAN DOWN / alamat lain): RST agar peer tidak
            // menggantung, kecuali WAN belum terbentuk sama sekali.
            if th.flags & TCP_FLAG_RST == 0 && rt.wan_ip != [0, 0, 0, 0] {
                self.stats.rst_sent += 1;
                if let Ok(n) = build_tcp_frame(
                    eth.src_mac,
                    rt.wan_mac,
                    ip.dst_ip,
                    ip.src_ip,
                    self.port,
                    th.src_port,
                    0,
                    th.seq.wrapping_add(payload.len() as u32),
                    TCP_FLAG_RST | TCP_FLAG_ACK,
                    0,
                    &[],
                    scratch,
                ) {
                    if send_frame(scratch, n, tx).is_err() {
                        self.stats.tx_failures += 1;
                    }
                }
            }
            return Ok(true);
        }
        if !tcp_checksum_ok(ip.src_ip, ip.dst_ip, seg_all) {
            self.stats.checksum_errors += 1;
            log("[R28] TCP checksum invalid — segmen dibuang\r\n");
            return Ok(true);
        }

        let slot = match self.find_slot(ip.src_ip, th.src_port) {
            Some(i) => i,
            None => {
                if th.flags & TCP_FLAG_SYN != 0 && th.flags & TCP_FLAG_ACK == 0 {
                    let i = match self.free_slot() {
                        Some(i) => i,
                        None => {
                            self.stats.session_full_drops += 1;
                            log("[R28] TCP SYN ditolak: tabel sesi penuh\r\n");
                            return Ok(true);
                        }
                    };
                    let isn = self.next_isn();
                    let mut s = WebSession::closed();
                    s.state = TcpSocketState::SynReceived;
                    s.remote_mac = eth.src_mac;
                    s.remote_ip = ip.src_ip;
                    s.remote_port = th.src_port;
                    s.rcv_nxt = th.seq.wrapping_add(1);
                    s.snd_nxt = isn.wrapping_add(1);
                    s.snd_una = isn;
                    s.peer_window = th.window;
                    self.stats.syn_received += 1;
                    let r = self.send_ctl(&mut s, rt, TCP_FLAG_SYN | TCP_FLAG_ACK, scratch, tx);
                    self.sessions[i] = s;
                    if r.is_ok() {
                        log("[R28] TCP SYN dari ");
                        log_ip(&ip.src_ip, log);
                        log(":");
                        log_u16(th.src_port, log);
                        log(" -> SYN/ACK (SYN-RECEIVED)\r\n");
                    } else {
                        self.stats.tx_failures += 1;
                    }
                    return Ok(true);
                }
                // Segmen untuk sesi yang tidak dikenal: diam (peer akan timeout).
                return Ok(true);
            }
        };

        let mut s = self.sessions[slot];
        let fin_peer = th.flags & TCP_FLAG_FIN != 0;
        if th.flags & TCP_FLAG_RST != 0 {
            self.sessions[slot] = WebSession::closed();
            log("[R28] TCP RST dari peer — sesi dibebaskan\r\n");
            return Ok(true);
        }

        // Konfirmasi data masuk (hanya yang in-order).
        if !payload.is_empty() {
            if th.seq == s.rcv_nxt {
                let take = core::cmp::min(payload.len(), WEB_REQ_MAX - s.req_len);
                if take > 0 {
                    self.req[slot][s.req_len..s.req_len + take].copy_from_slice(&payload[..take]);
                    s.req_len += take;
                }
                if take < payload.len() {
                    self.stats.out_of_order += 1;
                }
                s.rcv_nxt = s.rcv_nxt.wrapping_add(payload.len() as u32);
            } else {
                self.stats.out_of_order += 1;
            }
        }
        if fin_peer {
            s.rcv_nxt = s.rcv_nxt.wrapping_add(1);
        }

        // Kemajuan ACK peer (window update), dengan uji rentang modular.
        if th.flags & TCP_FLAG_ACK != 0 {
            let diff = th.ack.wrapping_sub(s.snd_una);
            let span = s.snd_nxt.wrapping_sub(s.snd_una);
            if diff <= span {
                s.snd_una = th.ack;
            }
            if s.state == TcpSocketState::SynReceived && th.ack == s.snd_nxt {
                s.state = TcpSocketState::Established;
                self.stats.sessions_established += 1;
                log("[R28] TCP handshake selesai (ESTABLISHED)\r\n");
            }
        }
        s.peer_window = th.window;
        s.hold_ticks = 0;

        // Dispatch HTTP hanya ketika permintaan benar-benar lengkap: header
        // tertutup dan body sepanjang Content-Length sudah mendarat penuh.
        if s.state == TcpSocketState::Established
            && !s.responded
            && s.req_len > 0
            && http_request_is_complete(&self.req[slot][..s.req_len], s.req_len == WEB_REQ_MAX)
        {
            self.build_response(slot, &mut s, rt, api, log);
        }

        // Kirim respons sepanjang window peer.
        if s.resp_len > s.resp_sent {
            self.flush_data(slot, &mut s, rt, scratch, tx);
        }

        // Respons ter-ACK penuh -> tutup sisi kita (Connection: close).
        let tx_done = s.resp_sent == s.resp_len && s.snd_una == s.snd_nxt;
        if !s.fin_sent && !fin_peer && s.responded && (tx_done || s.resp_len == 0) {
            s.fin_sent = true;
            s.state = TcpSocketState::FinWait;
            if self
                .send_ctl(&mut s, rt, TCP_FLAG_FIN | TCP_FLAG_ACK, scratch, tx)
                .is_err()
            {
                s.fin_sent = false;
            }
        }

        if fin_peer {
            // ACK atas FIN peer, lalu bebaskan slot.
            let _ = self.send_ctl(&mut s, rt, TCP_FLAG_ACK, scratch, tx);
            self.sessions[slot] = WebSession::closed();
            log("[R28] TCP sesi ditutup peer — slot dibebaskan\r\n");
        } else {
            self.sessions[slot] = s;
        }
        Ok(true)
    }

    /// Pompa per-tick: kirim sisa respons yang tertahan window, tutup sesi yang
    /// sudah lengkap, dan daur ulang slot yang ditinggal peer.
    pub fn on_tick(
        &mut self,
        rt: &WebRuntime,
        scratch: &mut [u8; 1518],
        tx: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
        log: &dyn Fn(&str),
    ) {
        if !self.listening {
            return;
        }
        for i in 0..WEB_SESSIONS {
            if self.sessions[i].state == TcpSocketState::Closed {
                continue;
            }
            let mut s = self.sessions[i];
            if s.resp_len > s.resp_sent {
                self.flush_data(i, &mut s, rt, scratch, tx);
            }
            if s.state == TcpSocketState::Established
                && s.responded
                && s.resp_len > 0
                && s.resp_sent == s.resp_len
                && s.snd_una == s.snd_nxt
                && !s.fin_sent
            {
                s.fin_sent = true;
                s.state = TcpSocketState::FinWait;
                if self
                    .send_ctl(&mut s, rt, TCP_FLAG_FIN | TCP_FLAG_ACK, scratch, tx)
                    .is_err()
                {
                    s.fin_sent = false;
                }
            }
            let stalled_body = s.state == TcpSocketState::Established
                && !s.responded
                && s.req_len > 0
                && s.resp_len == 0;
            if s.state == TcpSocketState::FinWait
                || s.state == TcpSocketState::SynReceived
                || stalled_body
            {
                s.hold_ticks += 1;
                if s.hold_ticks > WEB_SESSION_EXPIRY_TICKS {
                    self.sessions[i] = WebSession::closed();
                    log("[R28] TCP sesi kedaluwarsa — slot didaur ulang\r\n");
                    continue;
                }
            }
            self.sessions[i] = s;
        }
    }

    /// Bangun segmen kontrol murni (SYN/ACK, ACK, FIN/ACK).
    fn send_ctl(
        &mut self,
        s: &mut WebSession,
        rt: &WebRuntime,
        flags: u8,
        scratch: &mut [u8; 1518],
        tx: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        let seq = if flags & TCP_FLAG_SYN != 0 {
            s.snd_nxt.wrapping_sub(1)
        } else {
            s.snd_nxt
        };
        let n = build_tcp_frame(
            s.remote_mac,
            rt.wan_mac,
            rt.wan_ip,
            s.remote_ip,
            self.port,
            s.remote_port,
            seq,
            s.rcv_nxt,
            flags,
            WEB_ANNOUNCED_WINDOW,
            &[],
            scratch,
        )?;
        send_frame(scratch, n, tx)?;
        self.stats.segments_sent += 1;
        self.stats.bytes_sent += n as u32;
        if flags & TCP_FLAG_FIN != 0 {
            s.snd_nxt = s.snd_nxt.wrapping_add(1);
        }
        Ok(())
    }

    /// Kirim respons HTTP terfragmentasi per MSS, tunduk pada window peer.
    fn flush_data(
        &mut self,
        slot: usize,
        s: &mut WebSession,
        rt: &WebRuntime,
        scratch: &mut [u8; 1518],
        tx: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
    ) {
        let mut guard = 0u32;
        while s.resp_sent < s.resp_len && guard < 8 {
            guard += 1;
            let outstanding = s.snd_nxt.wrapping_sub(s.snd_una) as usize;
            let allowed = (s.peer_window as usize).saturating_sub(outstanding);
            let remaining = s.resp_len - s.resp_sent;
            let chunk = remaining.min(TCP_MAX_PAYLOAD).min(allowed);
            if chunk == 0 {
                break; // window tertutup: tunggu ACK berikutnya
            }
            let start = s.resp_sent;
            let last = start + chunk == s.resp_len;
            let flags = TCP_FLAG_ACK | if last { TCP_FLAG_PSH } else { 0 };
            let seq = s.snd_nxt;
            let built = build_tcp_frame(
                s.remote_mac,
                rt.wan_mac,
                rt.wan_ip,
                s.remote_ip,
                self.port,
                s.remote_port,
                seq,
                s.rcv_nxt,
                flags,
                WEB_ANNOUNCED_WINDOW,
                &self.resp[slot][start..start + chunk],
                scratch,
            );
            match built {
                Ok(n) => {
                    if send_frame(scratch, n, tx).is_err() {
                        self.stats.tx_failures += 1;
                        break;
                    }
                    self.stats.segments_sent += 1;
                    self.stats.bytes_sent += n as u32;
                    s.snd_nxt = s.snd_nxt.wrapping_add(chunk as u32);
                    s.resp_sent += chunk;
                }
                Err(_) => {
                    self.stats.tx_failures += 1;
                    break;
                }
            }
        }
    }

    /// Serialize respons HTTP/JSON untuk permintaan yang sudah lengkap.
    fn build_response(
        &mut self,
        slot: usize,
        s: &mut WebSession,
        rt: &WebRuntime,
        api: &mut ApiDispatcher,
        log: &dyn Fn(&str),
    ) {
        let raw_len = s.req_len;
        let parsed = HttpRequest::parse(&self.req[slot][..raw_len]);
        let out = &mut self.resp[slot];
        match parsed {
            Err(err) => {
                // Kode kesalahan ditentukan parser framing (400/413/501), bukan
                // diasumsikan 400. Respons hanya memuat kode protokol umum;
                // `err.reason` hanya tercatat di konsol, tidak dikirim ke jaringan.
                let code: &'static str = match err.status {
                    HttpStatus::PayloadTooLarge => "body_too_large",
                    HttpStatus::NotImplemented => "transfer_encoding_unsupported",
                    _ => "bad_request",
                };
                let body = match code {
                    "body_too_large" => b"{\"ok\":false,\"error\":\"body_too_large\"}".as_slice(),
                    "transfer_encoding_unsupported" => {
                        b"{\"ok\":false,\"error\":\"transfer_encoding_unsupported\"}".as_slice()
                    }
                    _ => b"{\"ok\":false,\"error\":\"bad_request\"}".as_slice(),
                };
                let b =
                    HttpResponseBuilder::new(out, err.status).with_content_type("application/json");
                if let Ok(n) = b.finish_with_body(body) {
                    let st = response_status(&out[..n]);
                    s.resp_len = n;
                    self.last_status = st;
                    if st == 400 {
                        self.stats.responses_400 += 1;
                    }
                }
                s.responded = true;
                log("[R28] HTTP permintaan ditolak sebelum dispatch: ");
                log(err.reason);
                log("\r\n");
            }
            Ok(req) => {
                let is_head = req.method == HttpMethod::Head;
                let is_post = req.method == HttpMethod::Post;
                let pl = req.path.len().min(64);
                self.last_path[..pl].copy_from_slice(&req.path.as_bytes()[..pl]);
                self.last_path_len = pl as u8;
                // POST hanya diterima untuk sumber daya antarmuka, jalur daya,
                // sesi autentikasi, dan manajemen akun; metode lain yang bukan
                // GET/HEAD tetap 405. Gerbang ini hanya membatasi metode —
                // otorisasinya tetap di lapisan API, sehingga menambah jalur di
                // sini tidak pernah membuka aksi tanpa sesi sah (dan tanpa
                // role admin untuk manajemen akun).
                let post_target_ok = is_post
                    && (req.path.starts_with("/api/v1/interfaces/")
                        || req.path == "/api/v1/power"
                        || req.path == "/api/v1/auth/login"
                        || req.path == "/api/v1/auth/logout"
                        || req.path == "/api/v1/auth/users");
                if (req.method != HttpMethod::Get && !is_head && !post_target_ok)
                    || (is_post && !post_target_ok)
                {
                    let b = HttpResponseBuilder::new(out, HttpStatus::MethodNotAllowed)
                        .with_content_type("application/json");
                    if let Ok(n) =
                        b.finish_with_body(b"{\"ok\":false,\"error\":\"method_not_allowed\"}")
                    {
                        s.resp_len = n;
                        self.stats.responses_405 += 1;
                        self.last_status = 405;
                    }
                    s.responded = true;
                    log("[R28] HTTP 405 method ditolak\r\n");
                    return;
                }
                if req.path.starts_with("/api/") {
                    // Identitas peer diambil dari sesi TCP, bukan dari header.
                    let mut rt_peer = *rt;
                    rt_peer.peer_ip = s.remote_ip;
                    let res = if is_post {
                        api.dispatch_post(&req, out, &rt_peer)
                    } else {
                        api.dispatch(&req, rt_peer.tick_100ms, out, &rt_peer)
                    };
                    match res {
                        Ok(n) => {
                            // API bisa mengembalikan 200, 403, atau 429 di jalur
                            // `Ok`; angka yang dicatat dibaca dari baris status.
                            let st = response_status(&out[..n]);
                            s.resp_len = n;
                            self.stats.api_served += 1;
                            self.last_status = st;
                            if is_post {
                                log("[R28] POST jalur tulis -> ");
                                log_u16(st, log);
                                log("\r\n");
                            }
                        }
                        Err(_) => {
                            let b = HttpResponseBuilder::new(out, HttpStatus::InternalServerError)
                                .with_content_type("application/json");
                            if let Ok(n) =
                                b.finish_with_body(b"{\"ok\":false,\"error\":\"json_overflow\"}")
                            {
                                s.resp_len = n;
                                self.last_status = 500;
                            }
                        }
                    }
                } else {
                    match dispatch_static(&req, out) {
                        Some(Ok(n)) => {
                            // Aset yang validatornya cocok dijawab 304 lewat jalur
                            // `Ok` yang sama; status dibaca dari baris status respons.
                            let st = response_status(&out[..n]);
                            s.resp_len = n;
                            self.last_status = st;
                        }
                        Some(Err(_)) => {
                            let b = HttpResponseBuilder::new(out, HttpStatus::InternalServerError)
                                .with_content_type("text/plain");
                            if let Ok(n) = b.finish_with_body(b"asset too large for buffer\n") {
                                s.resp_len = n;
                                self.last_status = 500;
                            }
                        }
                        None => {
                            let b = HttpResponseBuilder::new(out, HttpStatus::NotFound)
                                .with_content_type("application/json");
                            if let Ok(n) =
                                b.finish_with_body(b"{\"ok\":false,\"error\":\"not_found\"}")
                            {
                                s.resp_len = n;
                                self.stats.responses_404 += 1;
                                self.last_status = 404;
                            }
                        }
                    }
                }
                s.responded = true;
                // RFC 7230 4.3.2: respons HEAD wajib tanpa body (Content-Length tetap utuh).
                if is_head && s.resp_len > 0 {
                    if let Some(end) = find_double_crlf(&self.resp[slot][..s.resp_len]) {
                        s.resp_len = end + 4;
                    }
                }
                if s.resp_len > 0 {
                    self.stats.requests_served += 1;
                    log("[R28] HTTP ");
                    log(req.path);
                    log(" -> ");
                    log_u16(self.last_status, log);
                    log(" (");
                    log_u32(s.resp_len as u32, log);
                    log(" bytes dibangun)\r\n");
                } else {
                    log("[R28] HTTP respons gagal dibangun (buffer sesi terlalu kecil)\r\n");
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// HELPER TX & FORMAT TELEMETRI (tanpa alloc)
// -----------------------------------------------------------------------------

/// Kirim frame dari scratch, padding ke 60 B bila lebih pendek (Ethernet
/// minimum). Payload TCP tidak berubah karena panjang dinyatakan di header IPv4.
fn send_frame(
    scratch: &mut [u8; 1518],
    n: usize,
    tx: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let len = if n < ETH_MIN_FRAME {
        for b in scratch[n..ETH_MIN_FRAME].iter_mut() {
            *b = 0;
        }
        ETH_MIN_FRAME
    } else {
        n
    };
    tx(&scratch[..len])
}

/// Cari terminator header HTTP `\r\n\r\n`; mengembalikan indeks `\r` pertama.
fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    let mut i = 0;
    while i + 3 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Permintaan dianggap lengkap bila header sudah tertutup DAN seluruh body
/// sepanjang `Content-Length` sudah mendarat. Tanpa cek kedua, POST terfragmentasi
/// akan di-dispatch dengan body kosong — perintah tulis hilang diam-diam.
/// Buffer permintaan yang penuh diperlakukan lengkap agar pengurai membalas 400
/// alih-alih menggantung sampai slot kedaluwarsa.
///
/// Penafsiran `Content-Length` tidak lagi dimiliki fungsi ini: ia memanggil
/// `declared_body_len` milik parser yang sama, sehingga gerbang tunggu byte dan
/// pengurai tidak bisa berbeda baca terhadap framing yang identik.
fn http_request_is_complete(buf: &[u8], full: bool) -> bool {
    let header_end = match find_double_crlf(buf) {
        Some(i) => i + 4,
        None => return false,
    };
    if full {
        return true;
    }
    match declared_body_len(&buf[..header_end]) {
        Ok(Some(cl)) => buf.len() >= header_end + cl,
        // Tidak ada Content-Length: permintaan selesai pada header.
        Ok(None) => true,
        // Framing sudah pasti salah (chunked,CL ganda, nilai tak valid, body
        // melebihi batas): jangan tunggu byte tambahan, serahkan ke pengurai
        // yang mengirim kode 400/413/501 yang benar.
        Err(_) => true,
    }
}

fn log_ip(ip: &[u8; 4], log: &dyn Fn(&str)) {
    let mut b = [0u8; 15];
    let mut i = 0usize;
    for (n, o) in ip.iter().enumerate() {
        let mut v = *o;
        if v == 0 {
            b[i] = b'0';
            i += 1;
        } else {
            let mut d = [0u8; 3];
            let mut k = 0;
            while v > 0 {
                d[k] = b'0' + (v % 10) as u8;
                v /= 10;
                k += 1;
            }
            while k > 0 {
                k -= 1;
                b[i] = d[k];
                i += 1;
            }
        }
        if n < 3 {
            b[i] = b'.';
            i += 1;
        }
    }
    log(core::str::from_utf8(&b[..i]).unwrap_or("?"));
}

fn log_u16(v: u16, log: &dyn Fn(&str)) {
    log_u32(v as u32, log);
}

fn log_u32(mut v: u32, log: &dyn Fn(&str)) {
    let mut b = [0u8; 10];
    let mut i = b.len();
    if v == 0 {
        i -= 1;
        b[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        b[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    log(core::str::from_utf8(&b[i..]).unwrap_or("?"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_modules_registry() {
        assert_eq!(registry::MODULES.len(), 23);
        // Tiga belas modul berdata kernel nyata + sepuluh rupa saja yang sengaja
        // enabled:false karena kernel tidak punya endpoint-nya (WireGuard, VPN,
        // XRay, QoS Manager,
        // DHCP Server, Appearance, Packages, Speed Test, Telnet,
        // Trace Route).
        // Restart dan Shutdown tidak lagi menjadi modul berhalaman: keduanya
        // aksi daya yang langsung membuka dialog. Registri tidak boleh melaporkan
        // yang tidak ada sebagai aktif.
        assert_eq!(registry::MODULES.iter().filter(|m| m.enabled).count(), 13);
        assert_eq!(registry::MODULES.iter().filter(|m| !m.enabled).count(), 10);
        assert!(registry::MODULES.iter().all(|m| m.path != "/restart"));
        assert!(registry::MODULES.iter().all(|m| m.path != "/shutdown"));
        // Terminal naik menjadi berdata nyata pada build ini: setiap perintahnya
        // dipetakan ke endpoint yang diuji di test_05g, bukan ke keluaran karangan.
        assert!(registry::MODULES
            .iter()
            .any(|m| m.path == "/terminal" && m.enabled));
        // User Manager naik pada AUD-02 P0.1-D: barisnya dibaca dari AuthStore
        // lewat /api/v1/auth/users, bukan dari teks karangan.
        assert!(registry::MODULES
            .iter()
            .any(|m| m.path == "/usermanager" && m.enabled));
        // ARP Table naik pada AUD-02: kernel kini punya ArpTable nyata yang
        // diisi dari frame ARP yang masuk, dibaca lewat /api/v1/arp.
        assert!(registry::MODULES
            .iter()
            .any(|m| m.path == "/arp" && m.enabled));
        // Gateway naik pada AUD-02: FIB kernel (`asl-router-lpm`) dilaporkan apa
        // adanya lewat /api/v1/routes, termasuk saat rutenya belum ada.
        assert!(registry::MODULES
            .iter()
            .any(|m| m.path == "/gateway" && m.enabled));
        // Urutan mengikuti sidebar: NAT anak Firewall, grup Services (ARP Table,
        // Service List, DNS Server, Gateway, DHCP Server), Terminal, grup System,
        // lalu grup Tools.
        let names: [&str; 23] = [
            "Dashboard",
            "vOlt",
            "Interfaces",
            "Wireless",
            "Firewall",
            "NAT",
            "WireGuard",
            "VPN",
            "XRay-Core",
            "QoS Manager",
            "Services",
            "ARP Table",
            "Service List",
            "DNS Server",
            "Gateway",
            "DHCP Server",
            "Terminal",
            "Appearance",
            "Packages",
            "User Manager",
            "Speed Test",
            "Telnet",
            "Trace Route",
        ];
        for (i, want) in names.iter().enumerate() {
            assert_eq!(
                registry::MODULES[i].name,
                *want,
                "modul ke-{i} urutannya berubah"
            );
        }
    }

    #[test]
    fn test_02_all_assets_present() {
        assert_eq!(registry::ALL_ASSETS.len(), 52);
        assert_eq!(
            registry::SHELL_HTML.content_type,
            "text/html; charset=utf-8"
        );
        assert!(registry::SHELL_HTML.content.len() > 0);
    }

    #[test]
    fn test_02c_chrome_sesi_terpasang_dan_hanya_menampilkan_kode_kernel() {
        // P0.1 di lapisan klien: kartu masuk harus dimuat chrome (bukan
        // tersisa-malas) supaya penolakan 401 pertama dari kernel selalu punya
        // jalan masuk, dan tidak boleh ada akun/kata sandi bawaan yang
        // ditanamkan di aset yang terlayani.
        let shell = core::str::from_utf8(registry::SHELL_HTML.content).expect("shell.html utf-8");
        let pos_api = shell
            .find("/js/core/api.js")
            .expect("shell.html memuat api.js");
        let pos_login = shell
            .find("/js/core/login.js")
            .expect("shell.html memuat login.js");
        let pos_app = shell
            .find("/js/core/app.js")
            .expect("shell.html memuat app.js");
        assert!(
            pos_api < pos_login && pos_login < pos_app,
            "login.js harus termuat setelah klien API dan sebelum router app.js"
        );

        let js = core::str::from_utf8(registry::JS_LOGIN.content).expect("login.js utf-8");
        for need in [
            "OPCR.api.onGate = open",
            "OPCR.api.auth.status",
            "OPCR.api.auth.login",
            "OPCR.api.auth.logout",
            "no_credentials",
            "insufficient_role",
            "session_expired",
        ] {
            assert!(js.contains(need), "login.js kehilangan {need}");
        }
        // Nilai masuk hanya lewat body form klien API; tidak ada markup dari
        // data, tidak ada jejak konsol, dan tidak ada kredensial bawaan.
        for banned in ["innerHTML", "console.log", "password=\"", "Bearer "] {
            assert!(!js.contains(banned), "login.js memuat {banned}");
        }

        let api = core::str::from_utf8(registry::JS_API.content).expect("api.js utf-8");
        assert!(
            api.contains("setRequestHeader(\"Authorization\", \"Bearer \" + token)"),
            "api.js tidak menempelkan token sesi"
        );

        let css = core::str::from_utf8(registry::CSS_WEBFIG.content).expect("webfig.css utf-8");
        assert!(
            css.contains("input[type=\"password\"]"),
            "kolom kata sandi tidak distil"
        );
    }

    #[test]
    fn test_02b_every_asset_fits_response_buffer() {
        // Gerbang runtime: setiap aset harus terlayani penuh pada buffer
        // sebesar WEB_RESP_MAX. Gerbang konstan di modul sudah menahan
        // kompilasi; uji ini memastikan path dispatch benar-benar memuatnya.
        for asset in registry::ALL_ASSETS.iter() {
            // Katalog crate ini no_std: permintaan disusun di buffer stack,
            // bukan lewat `format!`/alokasi.
            let mut raw = [0u8; 96];
            let mut n = 0usize;
            for b in b"GET " {
                raw[n] = *b;
                n += 1;
            }
            for b in asset.path.as_bytes() {
                raw[n] = *b;
                n += 1;
            }
            for b in b" HTTP/1.1\r\n\r\n" {
                raw[n] = *b;
                n += 1;
            }
            let req = HttpRequest::parse(&raw[..n]).unwrap();
            let mut buf = [0u8; WEB_RESP_MAX];
            let len = dispatch_static(&req, &mut buf)
                .unwrap_or_else(|| panic!("{} tidak terdaftar", asset.path))
                .unwrap_or_else(|e| panic!("{} gagal: {e}", asset.path));
            assert!(len >= asset.content.len(), "{} terpotong", asset.path);
        }
    }

    #[test]
    fn test_03_dispatch_shell_root() {
        let req = HttpRequest::parse(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; WEB_RESP_MAX];
        let res = dispatch_static(&req, &mut buf).expect("Must find root asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("RINJANI 1.0.0"));
        assert!(resp.contains("Open Core Router Web Console"));
        // Shell modular hanya berisi chrome; halaman dimuat sebagai parsial.
        assert!(resp.contains("view-mount"));
        assert!(resp.contains("/js/core/app.js"));
        assert!(resp.contains("/js/core/nav.js"));
        assert!(resp.contains("/js/core/views.js"));
        assert!(resp.contains("/js/core/theme.js"));
        // Modal edit antarmuka bukan bagian shell lagi: dipisah ke aset
        // /views/iface-modal.html agar chrome tetap di bawah gerbang respons.
        assert!(!resp.contains("id=\"iface-overlay\""));
        assert!(resp.contains("/js/core/ifedit.js"));
        // Submenu drop-right Firewall -> NAT dan grup Services/System.
        assert!(resp.contains("nav-sub"));
        assert!(resp.contains("href=\"#nat\""));
        assert!(resp.contains("href=\"#dhcpserver\""));
        // Restart/Shutdown adalah aksi daya pada sidebar, bukan view: tidak ada
        // href/`data-view` untuk keduanya, hanya pegangan dialog `data-power`.
        assert!(resp.contains("data-power=\"restart\""));
        assert!(resp.contains("data-power=\"shutdown\""));
        assert!(!resp.contains("href=\"#restart\""));
        assert!(!resp.contains("href=\"#shutdown\""));
        // Ikon sidebar (data-ico + stylesheet mask-image) dan laci mobile.
        assert!(resp.contains("/css/icons.css"));
        assert!(resp.contains("/css/mobile.css"));
        // Chrome modul gaya WebFig: rupa di /css/table.css, ikon aksinya di
        // /css/icons-act.css (dipisah karena gerbang respons mengukur aset
        // satu per satu).
        assert!(resp.contains("/css/table.css"));
        assert!(resp.contains("/css/icons-act.css"));
        assert!(resp.contains("/js/core/table.js"));
        assert!(resp.contains("/js/core/tblcols.js"));
        assert!(resp.contains("class=\"nav-ico\" data-ico=\"iface\""));
        assert!(resp.contains("id=\"nav-scrim\""));
        // Anak grup Services termasuk ARP Table (tambahan owner di sidebar);
        // Registri, views.js, dan alias harus ikut menyimpannya.
        assert!(resp.contains("href=\"#arp\""));
        // Suntingan manual sidebar pernah meninggalkan item Interfaces kedua;
        // satu item per modul adalah kontrak navigasi.
        assert_eq!(
            resp.matches("data-view=\"interfaces\"").count(),
            1,
            "item Interfaces harus tepat satu"
        );
    }

    #[test]
    fn test_04_dispatch_css_asset() {
        let req = HttpRequest::parse(b"GET /css/base.css HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; WEB_RESP_MAX];
        let res = dispatch_static(&req, &mut buf).expect("Must find css asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("Content-Type: text/css"));
        assert!(resp.contains("--pf-bg-main"));
    }

    #[test]
    fn test_05_dispatch_core_js_module() {
        let req = HttpRequest::parse(b"GET /js/core/ui.js HTTP/1.1\r\n\r\n").unwrap();
        // Buffer seukuran buffer respons server nyata: aset terbesar harus muat.
        let mut buf = [0u8; WEB_RESP_MAX];
        let res = dispatch_static(&req, &mut buf).expect("Must find core js asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("Content-Type: application/javascript"));
        assert!(resp.contains("OPCR"));
    }

    /// Pengubah angka di ui.js dan pengikat pada kolom Tujuan rute harus bergerak
    /// bersama. Cacat nyata yang ditutup: kolom Tujuan memakai pengubah `ip`,
    /// sehingga rute default `0.0.0.0/0` tampil sebagai "belum terikat" dan
    /// tabel seolah tidak punya gateway, padahal rute itulah yang membuat
    /// lalu lintas keluar bekerja.
    #[test]
    fn test_05bb_fmt_addr_membedakan_rute_default_dari_belum_terikat() {
        let mut buf = [0u8; WEB_RESP_MAX];
        let req = HttpRequest::parse(b"GET /js/core/ui.js HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("ui.js harus terlayani")
            .unwrap();
        let js = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            js.contains("addr: function"),
            "ui.js kehilangan pengubah addr"
        );
        // Untuk alamat antarmuka, nol tetap berarti belum terikat — pengubah ip
        // tidak boleh ikut dilunakkan.
        assert!(
            js.contains("belum terikat"),
            "ui.js kehilangan label ip nol"
        );

        let req = HttpRequest::parse(b"GET /views/gateway.html HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("gateway.html harus terlayani")
            .unwrap();
        let html = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            html.contains("data-bind=\"dst\" data-fmt=\"addr\""),
            "kolom Tujuan rute belum memakai pengubah addr"
        );
        assert!(
            !html.contains("data-bind=\"dst\" data-fmt=\"ip\""),
            "kolom Tujuan rute masih memakai pengubah ip"
        );
    }

    /// CRUD User Manager: modal akun harus benar-benar terlayani sebagai aset
    /// dan tersambung ke kontrak tulis kernel yang sama dengan konsol serial.
    /// Yang dijaga di sini adalah sambungannya (aset terdaftar, view memanggil,
    /// ketiga action ada, kata sandi tidak pernah menjadi teks polos), bukan
    /// klaim bahwa aksi berhasil — keberhasilan hanya terbukti di runtime.
    #[test]
    fn test_05k_usermanager_crud_terpasang() {
        let mut buf = [0u8; WEB_RESP_MAX];

        let req = HttpRequest::parse(b"GET /js/core/useredit.js HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("/js/core/useredit.js tidak terdaftar")
            .expect("useredit.js terlalu besar untuk buffer respons");
        let js = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(js.contains("\"/api/v1/auth/users\""), "target POST hilang");
        assert!(
            js.contains("\"/views/user-modal.html\""),
            "sumber markup hilang"
        );
        assert!(
            js.contains("/api/v1/writes/last"),
            "verdict jurnal tidak dibaca"
        );
        assert!(js.contains("kredensial:"), "laporan kernel tidak disaring");
        for action in ["action=set", "action=del", "action=revoke"] {
            assert!(js.contains(action), "useredit.js kehilangan {action}");
        }
        // Data kernel tidak pernah menjadi markup (needle ber-dot agar komentar
        // yang menyebut innerHTML tidak ikut menyala).
        assert!(!js.contains(".innerHTML"), "useredit.js menulis innerHTML");

        let req = HttpRequest::parse(b"GET /views/user-modal.html HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("/views/user-modal.html tidak terdaftar")
            .expect("user-modal.html terlalu besar untuk buffer respons");
        let html = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            html.contains("id=\"user-overlay\""),
            "overlay tidak dikenali"
        );
        assert!(
            html.contains("id=\"user-lockrole\""),
            "penanda peran sesi hilang"
        );
        assert!(
            html.contains("insufficient_role"),
            "penolakan viewer tidak dilaporkan di modal"
        );
        assert_eq!(
            html.matches("type=\"password\"").count(),
            2,
            "kata sandi harus dua field bertipe password, bukan teks"
        );
        assert_eq!(
            html.matches("autocomplete=\"new-password\"").count(),
            2,
            "browser tidak boleh menawarkan sandi lama"
        );
        assert!(!html.contains("192.168."), "parsial memuat alamat karangan");

        let req = HttpRequest::parse(b"GET /views/usermanager.html HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("/views/usermanager.html tidak terdaftar")
            .expect("usermanager.html terlalu besar untuk buffer respons");
        let view = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            view.contains("data-list=\"/api/v1/auth/users\" data-modal=\"user\""),
            "tabel akun belum mendeklarasikan modal user"
        );
        assert!(
            view.contains("data-actions=\"new,edit,remove\""),
            "toolbar User Manager belum punya aksi tulis"
        );
        assert!(
            !view.contains("belum dipasang"),
            "view masih mengklaim formulir tulis belum ada"
        );

        let req = HttpRequest::parse(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("shell tidak terlayani")
            .expect("shell terlalu besar untuk buffer respons");
        let shell = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            shell.contains("/js/core/useredit.js"),
            "shell tidak memuat useredit.js"
        );

        // Dispatch chrome: modal yang benar harus terbuka untuk tabel yang benar.
        let req = HttpRequest::parse(b"GET /js/core/webfig.js HTTP/1.1\r\n\r\n").unwrap();
        let len = dispatch_static(&req, &mut buf)
            .expect("/js/core/webfig.js tidak terdaftar")
            .expect("webfig.js terlalu besar untuk buffer respons");
        let wf = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            wf.contains("kind === \"user\""),
            "dblclick belum ke modal user"
        );
        assert!(
            wf.contains("OPCR.useredit.open"),
            "aksi toolbar tidak memanggil modal"
        );
        assert!(
            !wf.contains("OPCR.useredit.remove()"),
            "Remove menghapus tanpa peneguhan operator"
        );
    }

    #[test]
    fn test_05b_dispatch_view_partial_is_declarative() {
        for (raw, path, needle) in [
            (
                b"GET /views/system.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/system.html",
                "data-source=\"/api/v1/system\"",
            ),
            (
                b"GET /views/interfaces.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/interfaces.html",
                "data-list=\"/api/v1/interfaces\"",
            ),
            (
                b"GET /views/firewall.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/firewall.html",
                "data-source=\"/api/v1/firewall/rules\"",
            ),
            (
                b"GET /views/nat.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/nat.html",
                "data-source=\"/api/v1/nat\"",
            ),
            (
                b"GET /views/wireless.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/wireless.html",
                "data-source=\"/api/v1/wireless\"",
            ),
            (
                b"GET /views/olt.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/olt.html",
                "data-source=\"/api/v1/olt\"",
            ),
            (
                b"GET /views/services.html HTTP/1.1\r\n\r\n" as &[u8],
                "/views/services.html",
                "data-source=\"/api/v1/services\"",
            ),
        ] {
            let req = HttpRequest::parse(raw).unwrap();
            let mut buf = [0u8; WEB_RESP_MAX];
            let len = dispatch_static(&req, &mut buf)
                .unwrap_or_else(|| panic!("{path} tidak terdaftar"))
                .expect("parsial terlalu besar");
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(resp.contains(needle), "{path} kehilangan pengikat {needle}");
            // Parsial hanya berisi bentuk + atribut pengikat, tidak pernah data.
            assert!(!resp.contains("192.168."), "{path} memuat alamat karangan");
            // Pola UI-02: setiap modul non-dashboard memakai tab strip + toolbar.
            assert!(
                resp.contains("wf-tabs") && resp.contains("wf-toolbar"),
                "{path} belum bergaya WebFig (tab strip + toolbar)"
            );
        }
    }

    #[test]
    fn test_05c_dispatch_new_sidebar_views() {
        // Sembilan entri baru sidebar owner. Yang punya data kernel nyata harus
        // membawa pengikat endpoint; yang rupa saja harus menandai dirinya
        // "desain saja" supaya tidak menyamar sebagai modul hidup.
        for (path, needle) in [
            (
                "/views/serviceslist.html",
                "data-source=\"/api/v1/services\"",
            ),
            ("/views/dnsserver.html", "data-source=\"/api/v1/dns\""),
            ("/views/wireguard.html", "desain saja"),
            ("/views/vpn.html", "desain saja"),
            ("/views/xray.html", "desain saja"),
            ("/views/qos.html", "desain saja"),
            ("/views/gateway.html", "data-list=\"/api/v1/routes\""),
            ("/views/arp.html", "data-list=\"/api/v1/arp\""),
            ("/views/dhcpserver.html", "desain saja"),
            ("/views/packages.html", "desain saja"),
            (
                "/views/usermanager.html",
                "data-list=\"/api/v1/auth/users\"",
            ),
            ("/views/speedtest.html", "desain saja"),
            ("/views/telnet.html", "desain saja"),
            ("/views/traceroute.html", "desain saja"),
            ("/views/iface-modal.html", "id=\"iface-overlay\""),
            ("/css/icons.css", "mask-image"),
            ("/css/mobile.css", "@media (max-width: 900px)"),
            ("/css/table.css", ".wf-tool"),
            ("/css/icons-act.css", "data-ico=\"cols\""),
            ("/css/terminal.css", ".term-screen"),
            ("/js/core/table.js", "tbl.setup"),
            ("/js/core/tblcols.js", "tbl.tableOps"),
            ("/js/core/termcmd.js", "Open Core Router"),
            ("/js/core/term.js", "term.setup"),
        ] {
            let mut raw = [0u8; 96];
            let mut n = 0usize;
            for b in b"GET " {
                raw[n] = *b;
                n += 1;
            }
            for b in path.as_bytes() {
                raw[n] = *b;
                n += 1;
            }
            for b in b" HTTP/1.1\r\n\r\n" {
                raw[n] = *b;
                n += 1;
            }
            let req = HttpRequest::parse(&raw[..n]).unwrap();
            let mut buf = [0u8; WEB_RESP_MAX];
            let len = dispatch_static(&req, &mut buf)
                .unwrap_or_else(|| panic!("{path} tidak terdaftar"))
                .expect("parsial terlalu besar");
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(resp.contains(needle), "{path} kehilangan {needle}");
            assert!(!resp.contains("192.168."), "{path} memuat alamat karangan");
        }
    }

    /// Ambil setiap token berawalan "/api/v1/" dari sebuah skrip, tanpa alokasi
    /// (katalog ini no_std bahkan saat uji). Karakter yang dianggap bagian path
    /// sama dengan yang diterima router: huruf, angka, '-', '_', '/'.
    fn api_tokens<'a>(src: &'a [u8], out: &mut [Option<&'a str>; 32]) -> usize {
        const PRE: &[u8] = b"/api/v1/";
        let mut n = 0usize;
        let mut i = 0usize;
        while i + PRE.len() <= src.len() {
            if &src[i..i + PRE.len()] == PRE {
                let mut j = i + PRE.len();
                while j < src.len() {
                    let c = src[j];
                    if c.is_ascii_alphanumeric() || c == b'/' || c == b'-' || c == b'_' {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if n < out.len() {
                    out[n] =
                        Some(core::str::from_utf8(&src[i..j]).expect("token path bukan utf-8"));
                }
                n += 1;
                i = j;
            } else {
                i += 1;
            }
        }
        n
    }

    #[test]
    fn test_05g_terminal_adalah_cli_di_atas_endpoint_nyata() {
        // Terminal naik dari rupa-saja menjadi modul berdata. Penjaganya harus
        // mengikat klaim itu pada isi dispatch_get kernel yang sebenarnya: lima
        // belas jalur baca dan dua permukaan tulis. Perintah baru apa pun wajib
        // lewat kernel lebih dulu.
        const READ: [&str; 15] = [
            "/api/v1/system",
            "/api/v1/interfaces",
            "/api/v1/firewall/rules",
            "/api/v1/nat",
            "/api/v1/nat/rules",
            "/api/v1/nat/conntrack",
            "/api/v1/wireless",
            "/api/v1/wireless/devices",
            "/api/v1/olt",
            "/api/v1/olt/ons",
            "/api/v1/services",
            "/api/v1/writes/last",
            "/api/v1/arp",
            "/api/v1/routes",
            "/api/v1/dns",
        ];
        // term.js hanya boleh menyebut satu jalur baca (salam) plus dua jalur
        // tulis kernel: aksi daya dan antarmuka WAN per nama.
        const WRITTEN: [&str; 3] = ["/api/v1/system", "/api/v1/power", "/api/v1/interfaces/"];

        let mut toks = [None; 32];
        let n = api_tokens(registry::JS_TERMCMD.content, &mut toks);
        assert_eq!(n, READ.len(), "termcmd.js merujuk {n} rujukan endpoint");
        for t in toks.iter().flatten() {
            assert!(
                READ.contains(t),
                "termcmd.js merujuk endpoint yang tidak ada: {t}"
            );
        }
        for p in READ.iter() {
            assert!(
                toks.iter().flatten().any(|t| t == p),
                "jalur baca {p} hilang dari termcmd.js"
            );
        }

        let mut toks2 = [None; 32];
        // 2x system (salam + pesan galat), 3x power (arm/restart/shutdown),
        // 1x interfaces/<nama>.
        assert_eq!(
            api_tokens(registry::JS_TERM.content, &mut toks2),
            6,
            "jumlah panggilan term.js berubah dari yang diizinkan kernel"
        );
        for t in toks2.iter().flatten() {
            assert!(
                WRITTEN.contains(t),
                "term.js menulis ke jalur di luar dua permukaan kernel: {t}"
            );
        }
        let eng = core::str::from_utf8(registry::JS_TERM.content).expect("term.js bukan utf-8");
        for guard in ["action=arm", "action=restart", "action=shutdown"] {
            assert!(eng.contains(guard), "term.js kehilangan kontrak {guard}");
        }

        // Tuntutan owner: New dan Close duduk di kartu header bernama, tanpa
        // chrome jendela (minimize/maximize/close), dan capnya OPEN CORE ROUTER.
        let view =
            core::str::from_utf8(registry::VIEW_TERMINAL.content).expect("terminal.html utf-8");
        for id in [
            "id=\"term-tabs\"",
            "id=\"term-host\"",
            "id=\"term-new\"",
            "id=\"term-close\"",
            "id=\"term-screen\"",
            "id=\"term-input\"",
            "@OpenCoreRouter",
        ] {
            assert!(view.contains(id), "terminal.html kehilangan {id}");
        }
        for banned in ["desain saja", "maximize", "minimise", "minimize"] {
            assert!(
                !view.contains(banned),
                "terminal.html masih memuat \"{banned}\""
            );
        }
        // Cap MikroTik dilarang di mana pun, dalam bentuk huruf apa pun.
        for (nama, bytes) in [
            ("views/terminal.html", registry::VIEW_TERMINAL.content),
            ("js/core/termcmd.js", registry::JS_TERMCMD.content),
            ("js/core/term.js", registry::JS_TERM.content),
            ("css/terminal.css", registry::CSS_TERMINAL.content),
        ] {
            assert!(
                !contains_ci(bytes, b"mikrotik"),
                "{nama} memuat cap MikroTik"
            );
        }
        let cmd = core::str::from_utf8(registry::JS_TERMCMD.content).expect("termcmd.js utf-8");
        assert!(
            cmd.contains("Open Core Router") && cmd.contains("ASL Core Kernel v1.0.0"),
            "banner terminal kehilangan cap resmi produk"
        );

        // Regresi UI-02I: `--pf-panel-head` itu strip TERANG (aqua pucat), jadi
        // judul/chip/tombol header tidak boleh memakai token putih murni atau
        // rgba putih — saat verifikasi live tombol New dan Close terrender
        // putih-di-atas-putih: ada di DOM, tidak terbaca di layar.
        let css = core::str::from_utf8(registry::CSS_TERMINAL.content).expect("terminal.css utf-8");
        assert!(
            css.contains("var(--pf-panel-head-text)"),
            "header terminal kehilangan token teks yang ikut tema"
        );
        assert!(
            !css.contains("var(--pf-on-accent)") && !css.contains("rgba(255, 255, 255"),
            "terminal.css memakai lapisan putih mentah di strip terang"
        );
        // Dua token ini terukur terlalu terang untuk teks di palet aku: cyan
        // aksen 2.4:1 (glif banner) dan text-title 3.1:1 (baris perintah).
        assert!(
            !css.contains("var(--pf-accent-cyan)") && !css.contains("var(--pf-text-title)"),
            "terminal.css memakai token teks yang kontrasnya di bawah 4.5:1"
        );
    }

    /// Pencocokan kasus-abai untuk larangan cap merek; tanpa alokasi.
    fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() || hay.len() < needle.len() {
            return false;
        }
        let mut i = 0usize;
        while i + needle.len() <= hay.len() {
            if hay[i..i + needle.len()]
                .iter()
                .zip(needle.iter())
                .all(|(h, n)| h.to_ascii_lowercase() == n.to_ascii_lowercase())
            {
                return true;
            }
            i += 1;
        }
        false
    }

    #[test]
    fn test_05e_appjs_menghidupkan_mesin_chrome() {
        // app.js harus memanggil mesin chrome dua kali: sekali saat parsial
        // dipasang (setup) dan sekali setiap kali baris kernel dilukis ulang
        // (onPaint). Kalau salah satu hilang, tabel tampil tanpa toolbar,
        // glif status, atau ikon tipe tanpa ada kesalahan yang terdengar.
        let req = HttpRequest::parse(b"GET /js/core/app.js HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; WEB_RESP_MAX];
        let len = dispatch_static(&req, &mut buf)
            .expect("app.js tidak terdaftar")
            .expect("app.js terlalu besar");
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("OPCR.tbl.setup(node)"));
        assert!(resp.contains("OPCR.tbl.onPaint(root)"));
        assert!(resp.contains("OPCR.wf.setup(node)"));

        // Terminal dimuat MALAS: shell.html memegang 7 882 B dari jatah 7 936 B,
        // jadi CSS+JS terminal tidak boleh ikut tertanam di chrome. app.js yang
        // memintanya saat halaman terminal dibuka, lalu memanggil setup().
        for src in [
            "\"/css/terminal.css\"",
            "\"/js/core/termcmd.js\"",
            "\"/js/core/term.js\"",
        ] {
            assert!(resp.contains(src), "app.js tidak memuat malas {src}");
        }
        assert!(
            resp.contains("OPCR.term.setup"),
            "app.js memanggil terminal tanpa penjagaan"
        );
        let shell = core::str::from_utf8(registry::SHELL_HTML.content).expect("shell.html utf-8");
        assert!(
            !shell.contains("term.js") && !shell.contains("terminal.css"),
            "shell.html memuat aset terminal secara statis"
        );
    }

    #[test]
    fn test_05f_tblcols_tidak_menimpa_namespace_tbl() {
        // table.js dan tblcols.js berbagi satu namespace OPCR.tbl. Pola IIFE
        // `.tbl = {}` pada berkas kedua akan membuang tbl.setup milik berkas
        // pertama; app.js lalu melempar TypeError dan setiap parsial tampil
        // tanpa chrome maupun data (cacat UI-02G saat verifikasi live).
        let owner = {
            let req = HttpRequest::parse(b"GET /js/core/table.js HTTP/1.1\r\n\r\n").unwrap();
            let mut buf = [0u8; WEB_RESP_MAX];
            let len = dispatch_static(&req, &mut buf)
                .expect("table.js tidak terdaftar")
                .expect("table.js terlalu besar");
            core::str::from_utf8(&buf[..len])
                .unwrap()
                .contains("tbl.setup = function")
        };
        assert!(owner, "table.js harus memegang tbl.setup");

        let req = HttpRequest::parse(b"GET /js/core/tblcols.js HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; WEB_RESP_MAX];
        let len = dispatch_static(&req, &mut buf)
            .expect("tblcols.js tidak terdaftar")
            .expect("tblcols.js terlalu besar");
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("tbl.tableOps = function"));
        assert!(
            !resp.contains(".tbl = {}"),
            "tblcols.js membuat ulang OPCR.tbl dan menghapus tbl.setup"
        );
    }

    fn served(a: &asl_router_webcore::StaticAsset) -> &'static str {
        core::str::from_utf8(a.content).expect("aset bukan utf-8")
    }

    #[test]
    fn test_05h_kejujuran_status_dan_submenu_layar_sentuh() {
        // Guard AUD-01, empat cacat yang muncul dari penelusuran rantai
        // UI->handler->endpoint->kernel:
        // (1) halaman tanpa sumber data dulu menulis "sinkron" padahal nol
        //     permintaan dikirim; (2) readout toolbar adalah simpul terikat
        //     sehingga Find/Filter menghapusnya permanen; (3) tap induk pada
        //     layar sentuh membuka submenu lalu app.js menutupnya lagi;
        // (4) Firewall melaporkan WAN terlindungi meski TCAM tidak ditanya.
        assert!(
            served(&registry::JS_UI).contains(r#"done(true, null, "kosong")"#),
            "jalur nol-sumber tidak lagi menandai halaman sebagai kosong"
        );
        let app = served(&registry::JS_APP);
        assert!(
            app.contains("halaman rupa: 0 endpoint kernel") && app.contains("(ok, why, kosong)"),
            "app.js tidak menangani penanda kosong"
        );

        let sys = served(&registry::VIEW_SYSTEM);
        assert!(
            sys.contains(r#"class="wf-info" data-source="/api/v1/system""#),
            "uptime Dashboard harus terikat di luar sasaran pesan toolbar"
        );
        assert!(
            !sys.contains(r#"wf-readout" data-source"#),
            "readout terikat akan dimusnahkan Find/Filter (table.js menulis textContent-nya)"
        );
        assert!(
            served(&registry::CSS_WEBFIG).contains(".wf-info"),
            ".wf-info belum bergaya: angka kernel dan pesan toolbar tak terbeda"
        );

        assert!(
            served(&registry::JS_NAV).contains("ev.stopImmediatePropagation()"),
            "tap induk layar sentuh diteruskan ke app.js yang menutup submenu"
        );

        let fw = served(&registry::VIEW_FIREWALL);
        assert!(
            fw.contains(r#"data-bind="rules_enforced""#) && fw.contains(r#"data-bind="datapath""#),
            "Firewall kembali melaporkan jumlah aturan sebagai penegakan"
        );
    }

    #[test]
    fn test_05i_status_log_dibaca_dari_baris_status_respons() {
        // Guard §4.27: `dispatch`/`dispatch_static` mengembalikan `Ok` untuk
        // 200, 304, 403 dan 429 sekaligus. Angka status yang ditulis sebagai
        // literal di build_response berarti konsol sedang mengarang.
        let src = include_str!("lib.rs");
        // Kedua penanda ditulis dengan escape: bila keduanya tertulis sebagai
        // literal utuh, badan tes ini menemukan dirinya sendiri dan guard
        // gagal (atau lolos) karena alasan yang salah.
        const HARD: &str = "last_status = 2\x30\x30";
        const CALL: &str = "response_status(&out[..n\x5d)";
        assert!(
            !src.contains(HARD),
            "200 dihardcode lagi: respons 304/403/429 akan tercatat sebagai 200"
        );
        // Tiga jalur, bukan dua: API, aset, dan penolakan framing (400/413/501)
        // semuanya wajib membaca baris status yang benar-benar dibangun.
        assert_eq!(
            src.matches(CALL).count(),
            3,
            "jalur API, jalur aset, dan jalur penolakan wajib membaca baris status yang dikirim"
        );
    }

    #[test]
    fn test_05j_penolakan_bertingkat_dan_footer_halaman_kerja() {
        // Guard AUD-01 lanjutan, dari penelusuran langsung di VM:
        // (1) "/tool ping 198.51.100.9" dulu jatuh ke "perintah tidak dikenal"
        //     padahal tabel NO punya alasan teknisnya (tanpa argumen);
        // (2) kepala sesi terminal membeku pada "menghubungkan…" setelah
        //     view dibangun ulang karena alamat host tidak dibaca lagi;
        // (3) footer melabeli terminal sebagai "halaman rupa" padahal tiap
        //     perintahnya adalah panggilan HTTP nyata ke kernel.
        let term = served(&registry::JS_TERM);
        assert!(
            term.contains("nw[k] === cw[k]"),
            "tabel NO hanya dicocokkan persis: perintah berargumen kehilangan sebab penolakan"
        );
        assert!(
            !term.contains(r#"D.NO[i][0] === c || D.NO[i][0] === head"#),
            "cocok-persis lama masih menjadi satu-satunya gerbang penolakan"
        );
        assert_eq!(
            term.matches("greet(cur)").count(),
            2,
            "sesi yang dibangun ulang wajib membaca ulang alamat host"
        );
        assert!(
            served(&registry::JS_APP).contains("halaman interaktif"),
            "footer tidak lagi membedakan halaman rupa dari halaman kerja"
        );
        assert!(
            served(&registry::VIEW_TERMINAL).contains(r#"data-cli="1""#),
            "terminal.html kehilangan penanda halaman kerja"
        );
    }

    #[test]
    fn test_05d_shell_routes_and_retired_power_paths() {
        // Route shell termasuk grup Services/System/Tools. /reboot pensiun karena
        // sidebar owner memakai label Restart; /restart dan /shutdown pensiun
        // karena Restart/Shutdown bukan halaman lagi, melainkan dialog aksi daya.
        for path in [
            "/wireguard",
            "/vpn",
            "/xray",
            "/serviceslist",
            "/dnsserver",
            "/dhcpserver",
            "/packages",
            "/usermanager",
            "/qos",
            "/gateway",
            "/arp",
            "/terminal",
            "/speedtest",
            "/telnet",
            "/traceroute",
        ] {
            let mut raw = [0u8; 64];
            let mut n = 0usize;
            for b in b"GET " {
                raw[n] = *b;
                n += 1;
            }
            for b in path.as_bytes() {
                raw[n] = *b;
                n += 1;
            }
            for b in b" HTTP/1.1\r\n\r\n" {
                raw[n] = *b;
                n += 1;
            }
            let req = HttpRequest::parse(&raw[..n]).unwrap();
            let mut buf = [0u8; WEB_RESP_MAX];
            let len = dispatch_static(&req, &mut buf)
                .unwrap_or_else(|| panic!("{path} tidak dipetakan ke shell"))
                .expect("shell terlalu besar");
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(resp.contains("id=\"view-mount\""), "{path} bukan shell");
        }

        for retired in [
            b"GET /reboot HTTP/1.1\r\n\r\n".as_slice(),
            b"GET /restart HTTP/1.1\r\n\r\n".as_slice(),
            b"GET /shutdown HTTP/1.1\r\n\r\n".as_slice(),
        ] {
            let req = HttpRequest::parse(retired).unwrap();
            let mut buf = [0u8; WEB_RESP_MAX];
            assert!(
                dispatch_static(&req, &mut buf).is_none(),
                "{} seharusnya sudah pensiun",
                req.path
            );
        }
    }

    #[test]
    fn test_06_dispatch_logo_svg() {
        let req = HttpRequest::parse(b"GET /images/logo.svg HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; WEB_RESP_MAX];
        let res = dispatch_static(&req, &mut buf).expect("Must find logo asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("Content-Type: image/svg+xml"));
        assert!(resp.contains("Open Core Router Rinjani Logo"));
    }

    // -------------------------------------------------------------------------
    // UJI END-TO-END TCP + HTTP (skenario sintetis, alamat TEST-NET RFC 5737)
    // -------------------------------------------------------------------------

    use asl_router_netstack::{
        build_tcp_frame as cli_build, parse_ethernet_header, parse_ipv4_header, parse_tcp_header,
        tcp_checksum_ok, TcpHeader, TCP_FLAG_ACK, TCP_FLAG_FIN, TCP_FLAG_PSH, TCP_FLAG_RST,
        TCP_FLAG_SYN,
    };

    const SRV_MAC: [u8; 6] = [0x08, 0x00, 0x27, 0x0A, 0x00, 0x01];
    const CLI_MAC: [u8; 6] = [0x08, 0x00, 0x27, 0x0A, 0x00, 0x02];
    const SRV_IP: [u8; 4] = [192, 0, 2, 10];
    const CLI_IP: [u8; 4] = [192, 0, 2, 20];
    const CLI_PORT: u16 = 51000;
    const CLI_WIN: u16 = 64240;
    // Kredensial tes hanya hidup di modul ini. Build produksi tidak membawa akun
    // bawaan apa pun (lihat `asl_router_api::auth::test_auth_no_default_credentials`).
    const T_USER: &str = "admin";
    const T_PW: &[u8] = b"rinjani-webui-sesi-uji";

    /// Isi `ArpTable` contoh untuk tes jalur baca: satu tetangga hasil reply di
    /// WAN, satu hasil request di LAN. Nilainya berasal dari fixture runtime,
    /// bukan dikarang endpoint.
    static T_ARP: [asl_router_api::ArpSnapshot; 2] = [
        asl_router_api::ArpSnapshot {
            ip: [192, 0, 2, 20],
            mac: [0x08, 0x00, 0x27, 0x0A, 0x00, 0x02],
            ifindex: 0,
            reply: true,
            seen: 4,
            last_tick: 90,
        },
        asl_router_api::ArpSnapshot {
            ip: [192, 0, 2, 31],
            mac: [0x08, 0x00, 0x27, 0x0A, 0x00, 0x1F],
            ifindex: 1,
            reply: false,
            seen: 1,
            last_tick: 95,
        },
    ];

    /// Isi FIB fixture: default via gateway WAN, connected subnet WAN, dan satu
    /// rute tanpa antarmuka keluar (kalimat "bila tidak ada, jangan menamai").
    static T_ROUTES: [asl_router_api::RouteSnapshot; 3] = [
        asl_router_api::RouteSnapshot {
            network: [0, 0, 0, 0],
            prefix_len: 0,
            ifindex: 1,
            gateway: [192, 0, 2, 1],
            metric: 100,
            kind: asl_router_api::ROUTE_KIND_STATIC,
        },
        asl_router_api::RouteSnapshot {
            network: [192, 0, 2, 0],
            prefix_len: 24,
            ifindex: 1,
            gateway: [0, 0, 0, 0],
            metric: 0,
            kind: asl_router_api::ROUTE_KIND_CONNECTED,
        },
        asl_router_api::RouteSnapshot {
            network: [10, 99, 0, 0],
            prefix_len: 16,
            ifindex: 0,
            gateway: [0, 0, 0, 0],
            metric: 0,
            kind: asl_router_api::ROUTE_KIND_DROP,
        },
    ];

    /// Penangkap frame TX server (tanpa heap).
    struct Cap {
        frames: [[u8; 1518]; 8],
        lens: [usize; 8],
        n: usize,
        dropped: usize,
    }

    impl Cap {
        const fn new() -> Self {
            Self {
                frames: [[0u8; 1518]; 8],
                lens: [0; 8],
                n: 0,
                dropped: 0,
            }
        }
        fn push(&mut self, f: &[u8]) -> Result<(), &'static str> {
            if self.n >= 8 {
                self.dropped += 1;
                return Ok(());
            }
            self.frames[self.n][..f.len()].copy_from_slice(f);
            self.lens[self.n] = f.len();
            self.n += 1;
            Ok(())
        }
        fn reset(&mut self) {
            self.n = 0;
        }
    }

    /// Urai frame hasil tangkapan menjadi (ip_src, ip_dst, tcp_hdr, payload).
    fn seg_of(f: &[u8]) -> ([u8; 4], [u8; 4], TcpHeader, &[u8]) {
        let (_eth, ep) = parse_ethernet_header(f).expect("eth parse");
        let (ip, seg) = parse_ipv4_header(ep).expect("ip parse");
        let (th, payload) = parse_tcp_header(seg).expect("tcp parse");
        assert!(
            tcp_checksum_ok(ip.src_ip, ip.dst_ip, seg),
            "checksum frame keluaran server tidak valid"
        );
        (ip.src_ip, ip.dst_ip, th, payload)
    }

    fn runtime() -> WebRuntime<'static> {
        WebRuntime {
            hostname: "rinjani-wan",
            wan_present: true,
            wan_name: "eth0",
            lan_name: "eth1",
            wan_ip: SRV_IP,
            wan_netmask: [255, 255, 255, 0],
            wan_netmask_src: "option-1-ack",
            wan_gw: [192, 0, 2, 1],
            wan_gw_src: "option-3-ack",
            wan_dns1: [192, 0, 2, 53],
            wan_dns2: [0, 0, 0, 0],
            wan_dns_src: "option-6-offer",
            wan_up: true,
            wan_source: "static",
            wan_mac: SRV_MAC,
            http_listening: true,
            http_port: 80,
            serial_cli_active: true,
            arp_list: &T_ARP,
            arp_count: 2,
            arp_capacity: 16,
            arp_evictions: 0,
            arp_rejected: 0,
            arp_timeout_s: 20,
            route_list: &T_ROUTES,
            route_count: 3,
            route_capacity: 64,
            route_lookups: 5,
            route_forwards: 0,
            route_drops: 5,
            dns_active: false,
            dns_upstream: [192, 0, 2, 53],
            dns_cache_hits: 11,
            dns_cache_misses: 5,
            dns_cache_entries: 3,
            dns_cache_capacity: 16,
            dns_host_count: 1,
            dns_host_capacity: 8,
            // Tick fixture dibuat bukan-nol supaya umur tetangga ARP terhitung
            // nyata (bukan selisih nol yang selalu lulus).
            tick_100ms: 100,
            ..Default::default()
        }
    }

    /// Rakit permintaan HTTP dari potongan-potongan byte ke `dst`. Crate ini
    /// `no_std`, jadi tidak ada `format!` di jalur tes.
    fn req_parts<'a>(dst: &'a mut [u8], parts: &[&[u8]]) -> &'a [u8] {
        let mut n = 0usize;
        for p in parts {
            dst[n..n + p.len()].copy_from_slice(p);
            n += p.len();
        }
        &dst[..n]
    }

    /// Rakit POST `application/x-www-form-urlencoded` lengkap. `Content-Length`
    /// dihitung dari panjang body (crate ini `no_std`, jadi digit disusun manual)
    /// supaya tes tidak lulus karena angka yang ditulis tangan kebetulan cocok.
    fn post_form<'a>(dst: &'a mut [u8], path: &[u8], auth: &[u8], body: &[u8]) -> &'a [u8] {
        let mut n = 0usize;
        let mut put = |src: &[u8]| -> Result<(), ()> {
            if n + src.len() > dst.len() {
                return Err(());
            }
            dst[n..n + src.len()].copy_from_slice(src);
            n += src.len();
            Ok(())
        };
        assert!(put(b"POST ").is_ok());
        assert!(put(path).is_ok());
        assert!(put(b" HTTP/1.1\r\nHost: 192.0.2.10\r\n").is_ok());
        if !auth.is_empty() {
            assert!(put(auth).is_ok());
            assert!(put(b"\r\n").is_ok());
        }
        assert!(
            put(b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: ").is_ok()
        );
        let mut digits = [0u8; 20];
        let mut d = digits.len();
        let mut v = body.len();
        if v == 0 {
            d -= 1;
            digits[d] = b'0';
        }
        while v > 0 {
            d -= 1;
            digits[d] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        assert!(put(&digits[d..]).is_ok());
        assert!(put(b"\r\n\r\n").is_ok());
        assert!(put(body).is_ok());
        &dst[..n]
    }

    /// Meniru bootstrap konsol serial: pasang satu akun admin lalu buka sesi
    /// lewat login nyata (bukan token karangan). `hdr` diisi baris
    /// `Authorization: Bearer <hex64>` dan panjangnya dikembalikan.
    fn boot_admin(api: &mut ApiDispatcher, tick: u64, hdr: &mut [u8; 88]) -> usize {
        use asl_router_api::auth::{AuthStore, Role, AUTH_DEFAULT_ROUNDS};
        let salt = api.auth.new_salt();
        let hash = AuthStore::derive(T_PW, &salt, AUTH_DEFAULT_ROUNDS);
        api.auth
            .set_user(T_USER, Role::Admin, &salt, AUTH_DEFAULT_ROUNDS, &hash)
            .expect("bootstrap admin");
        let (_, tok, _) = api.auth.login(T_USER, T_PW, tick).expect("sesi login");
        let mut hexbuf = [0u8; 64];
        let hx = asl_router_crypto::hex::encode(&tok, &mut hexbuf).expect("token hex");
        let line = b"Authorization: Bearer ";
        hdr[..line.len()].copy_from_slice(line);
        let n = line.len() + hx.len();
        hdr[line.len()..n].copy_from_slice(hx.as_bytes());
        n
    }

    /// Bangun segmen klien lalu suapkan ke server; hasil TX tertangkap di `cap`.
    #[allow(clippy::too_many_arguments)]
    fn client_frame(
        server: &mut TcpWebServer,
        rt: &WebRuntime,
        api: &mut ApiDispatcher,
        seq: u32,
        ack: u32,
        flags: u8,
        payload: &[u8],
        cap: &mut Cap,
    ) {
        let mut scratch = [0u8; 1518];
        let mut inbound = [0u8; 1518];
        let port = server.port;
        let n = cli_build(
            CLI_MAC,
            SRV_MAC,
            CLI_IP,
            SRV_IP,
            CLI_PORT,
            port,
            seq,
            ack,
            flags,
            CLI_WIN,
            payload,
            &mut inbound,
        )
        .expect("frame klien");
        cap.reset();
        let mut tx = |b: &[u8]| cap.push(b);
        let nolog = |_: &str| {};
        let handled = server
            .on_frame(&inbound[..n], rt, api, &mut scratch, &mut tx, &nolog)
            .expect("on_frame");
        assert!(handled, "frame TCP port server harus diakui server");
    }

    /// Rakit seluruh payload TCP dari frame yang tertangkap, urut sesuai seq.
    fn reassemble(cap: &Cap) -> ([u8; 8192], usize) {
        let mut body = [0u8; 8192];
        let mut off = 0usize;
        for i in 0..cap.n {
            let f = &cap.frames[i][..cap.lens[i]];
            let (_s, _d, th, payload) = seg_of(f);
            if payload.is_empty() {
                continue;
            }
            body[off..off + payload.len()].copy_from_slice(payload);
            off += payload.len();
            assert!(th.data_offset >= 5);
        }
        (body, off)
    }

    #[test]
    fn test_10_tcp_handshake_then_get_root_serves_html() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();

        // 1) SYN -> SYN/ACK
        client_frame(
            &mut server,
            &rt,
            &mut api,
            1000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap,
        );
        assert_eq!(cap.n, 1);
        let (src, dst, sa, _) = seg_of(&cap.frames[0][..cap.lens[0]]);
        assert_eq!(src, SRV_IP);
        assert_eq!(dst, CLI_IP);
        assert_eq!(
            sa.flags & (TCP_FLAG_SYN | TCP_FLAG_ACK),
            TCP_FLAG_SYN | TCP_FLAG_ACK
        );
        assert_eq!(sa.ack, 1001, "SYN/ACK harus meng-ACK SYN klien");
        assert_eq!(sa.dst_port, CLI_PORT);
        let our_next = sa.seq.wrapping_add(1); // ISN + 1 (SYN/ACK menempati 1 seq)
        assert_eq!(server.sessions[0].state, TcpSocketState::SynReceived);
        assert_eq!(server.stats.syn_received, 1);

        // 2) ACK -> ESTABLISHED, server belum mengirim apa pun
        client_frame(
            &mut server,
            &rt,
            &mut api,
            1001,
            our_next,
            TCP_FLAG_ACK,
            &[],
            &mut cap,
        );
        assert_eq!(cap.n, 0);
        assert_eq!(server.sessions[0].state, TcpSocketState::Established);
        assert_eq!(server.stats.sessions_established, 1);

        // 3) GET / -> 200 + body shell.html (tersegmentasi)
        let req = b"GET / HTTP/1.1\r\nHost: 192.0.2.10\r\nUser-Agent: curl/8.7\r\nAccept: */*\r\nConnection: close\r\n\r\n";
        let cseq_after = 1001 + req.len() as u32;
        client_frame(
            &mut server,
            &rt,
            &mut api,
            1001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        assert!(cap.n >= 1 && cap.dropped == 0);
        let (body, len) = reassemble(&cap);
        let text = core::str::from_utf8(&body[..len]).expect("respons UTF-8");
        assert!(
            text.starts_with("HTTP/1.1 200 OK"),
            "status line: {}",
            &text[..40.min(len)]
        );
        assert!(text.contains("Content-Length:"));
        assert!(text.contains("Connection: close"));
        assert!(text.contains("RINJANI 1.0.0"));
        assert_eq!(
            len, server.sessions[0].resp_len,
            "seluruh respons harus terkirim"
        );
        assert_eq!(server.stats.requests_served, 1);

        // 4) ACK penuh -> FIN/ACK
        let srv_next = server.sessions[0].snd_nxt;
        client_frame(
            &mut server,
            &rt,
            &mut api,
            cseq_after,
            srv_next,
            TCP_FLAG_ACK,
            &[],
            &mut cap,
        );
        assert_eq!(cap.n, 1);
        let (_, _, fin, _) = seg_of(&cap.frames[0][..cap.lens[0]]);
        assert_eq!(
            fin.flags & (TCP_FLAG_FIN | TCP_FLAG_ACK),
            TCP_FLAG_FIN | TCP_FLAG_ACK
        );
        assert_eq!(server.sessions[0].state, TcpSocketState::FinWait);

        // 5) FIN klien -> slot dibebaskan
        client_frame(
            &mut server,
            &rt,
            &mut api,
            cseq_after,
            fin.seq + 1,
            TCP_FLAG_FIN | TCP_FLAG_ACK,
            &[],
            &mut cap,
        );
        assert_eq!(server.active_sessions(), 0);
        assert_eq!(server.stats.tx_failures, 0);
    }

    #[test]
    fn test_11_api_over_tcp_reports_real_wan_ip_only() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();

        client_frame(
            &mut server,
            &rt,
            &mut api,
            4000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap,
        );
        let our_next = seg_of(&cap.frames[0][..cap.lens[0]]).2.seq.wrapping_add(1);
        client_frame(
            &mut server,
            &rt,
            &mut api,
            4001,
            our_next,
            TCP_FLAG_ACK,
            &[],
            &mut cap,
        );
        let mut hdr = [0u8; 88];
        let mut raw = [0u8; 256];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let req = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/interfaces HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &hdr[..hn],
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut server,
            &rt,
            &mut api,
            4001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        let (body, len) = reassemble(&cap);
        let text = core::str::from_utf8(&body[..len]).expect("json utf-8");
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains("\"ip\":\"192.0.2.10\""));
        assert!(text.contains("\"source\":\"static\""));
        assert!(text.contains("\"name\":\"eth0\""));
        // Topologi satu-NIC: hanya eth0 yang muncul. Tidak ada baris anomali
        // `eth1 absent` lagi — itu dahulu membuat UI tampak punya dua port.
        assert!(!text.contains("eth1"));
        assert!(!text.contains("absent"));
        assert_eq!(text.matches("\"name\":").count(), 1);
        // Tidak boleh ada alamat karangan.
        assert!(!text.contains("192.168.56."));
        assert!(!text.contains("192.0.2.199"));
        assert_eq!(server.stats.api_served, 1);
    }

    #[test]
    fn test_12_unknown_path_returns_404_and_405_for_post() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();

        client_frame(
            &mut server,
            &rt,
            &mut api,
            7000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap,
        );
        let our_next = seg_of(&cap.frames[0][..cap.lens[0]]).2.seq.wrapping_add(1);
        client_frame(
            &mut server,
            &rt,
            &mut api,
            7001,
            our_next,
            TCP_FLAG_ACK,
            &[],
            &mut cap,
        );

        let req = b"GET /tidak-ada HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n";
        client_frame(
            &mut server,
            &rt,
            &mut api,
            7001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        let (body, len) = reassemble(&cap);
        let text = core::str::from_utf8(&body[..len]).unwrap();
        assert!(text.starts_with("HTTP/1.1 404 Not Found"));
        assert!(text.contains("not_found"));
        assert_eq!(server.stats.responses_404, 1);
        assert_eq!(server.last_status, 404);

        // Sesi pertama sudah responded, jadi POST diuji pada server baru.
        let mut server2 = TcpWebServer::new(80);
        server2.listen(80);
        let mut cap2 = Cap::new();
        client_frame(
            &mut server2,
            &rt,
            &mut api,
            8000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap2,
        );
        let our_next2 = seg_of(&cap2.frames[0][..cap2.lens[0]])
            .2
            .seq
            .wrapping_add(1);
        client_frame(
            &mut server2,
            &rt,
            &mut api,
            8001,
            our_next2,
            TCP_FLAG_ACK,
            &[],
            &mut cap2,
        );
        let post = b"POST /api/v1/system HTTP/1.1\r\nHost: 192.0.2.10\r\nContent-Length: 0\r\n\r\n";
        client_frame(
            &mut server2,
            &rt,
            &mut api,
            8001,
            our_next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            post,
            &mut cap2,
        );
        let (body2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&body2[..len2]).unwrap();
        assert!(text2.starts_with("HTTP/1.1 405"));
        assert_eq!(server2.stats.responses_405, 1);
    }

    #[test]
    fn test_13_rst_when_wan_not_bound_and_checksum_rejection() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        // WAN belum terbentuk: alamat nol -> tidak boleh RST dari 0.0.0.0.
        let rt_down = WebRuntime {
            wan_mac: SRV_MAC,
            ..Default::default()
        };
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        client_frame(
            &mut server,
            &rt_down,
            &mut api,
            1000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap,
        );
        assert_eq!(cap.n, 0, "tanpa binding WAN tidak boleh ada TX");
        assert_eq!(server.stats.rst_sent, 0);
        assert_eq!(server.active_sessions(), 0);

        // WAN up tapi frame ditujukan ke IP lain -> RST.
        let rt = runtime();
        let mut inbound = [0u8; 1518];
        let n = cli_build(
            CLI_MAC,
            SRV_MAC,
            CLI_IP,
            [192, 0, 2, 99],
            CLI_PORT,
            80,
            1000,
            0,
            TCP_FLAG_SYN,
            CLI_WIN,
            &[],
            &mut inbound,
        )
        .unwrap();
        let mut scratch = [0u8; 1518];
        {
            cap.reset();
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            assert!(server
                .on_frame(&inbound[..n], &rt, &mut api, &mut scratch, &mut tx, &nolog)
                .unwrap());
        }
        assert_eq!(cap.n, 1);
        let (_, _, rst, _) = seg_of(&cap.frames[0][..cap.lens[0]]);
        assert_eq!(rst.flags & TCP_FLAG_RST, TCP_FLAG_RST);
        assert_eq!(server.stats.rst_sent, 1);

        // Checksum rusak -> dibuang tanpa menyentuh statistik lain.
        let mut bad = [0u8; 1518];
        let bn = cli_build(
            CLI_MAC,
            SRV_MAC,
            CLI_IP,
            SRV_IP,
            CLI_PORT,
            80,
            2000,
            0,
            TCP_FLAG_SYN,
            CLI_WIN,
            &[],
            &mut bad,
        )
        .unwrap();
        bad[bn - 1] ^= 0xFF;
        {
            cap.reset();
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            assert!(server
                .on_frame(&bad[..bn], &rt, &mut api, &mut scratch, &mut tx, &nolog)
                .unwrap());
        }
        assert_eq!(cap.n, 0);
        assert_eq!(server.stats.checksum_errors, 1);
        assert_eq!(server.active_sessions(), 0);
    }

    #[test]
    fn test_14_head_response_has_no_body() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        client_frame(
            &mut server,
            &rt,
            &mut api,
            3000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap,
        );
        let our_next = seg_of(&cap.frames[0][..cap.lens[0]]).2.seq.wrapping_add(1);
        client_frame(
            &mut server,
            &rt,
            &mut api,
            3001,
            our_next,
            TCP_FLAG_ACK,
            &[],
            &mut cap,
        );
        let req = b"HEAD / HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n";
        client_frame(
            &mut server,
            &rt,
            &mut api,
            3001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        let (body, len) = reassemble(&cap);
        let text = core::str::from_utf8(&body[..len]).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.ends_with("\r\n\r\n"), "HEAD tidak boleh membawa body");
        assert!(text.contains("Content-Length:"));
    }

    #[test]
    fn test_15_on_tick_retransmits_window_limited_data() {
        // Peer mengiklankan window kecil: respons tertahan lalu jalan lagi per tick.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        client_frame(
            &mut server,
            &rt,
            &mut api,
            5000,
            0,
            TCP_FLAG_SYN,
            &[],
            &mut cap,
        );
        let our_next = seg_of(&cap.frames[0][..cap.lens[0]]).2.seq.wrapping_add(1);

        // ACK handshake dengan window 1000 -> hanya 1 segmen 1000 B yang boleh keluar.
        let mut inbound = [0u8; 1518];
        let mut scratch = [0u8; 1518];
        let n = cli_build(
            CLI_MAC,
            SRV_MAC,
            CLI_IP,
            SRV_IP,
            CLI_PORT,
            80,
            5001,
            our_next,
            TCP_FLAG_ACK,
            1000,
            &[],
            &mut inbound,
        )
        .unwrap();
        {
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            server
                .on_frame(&inbound[..n], &rt, &mut api, &mut scratch, &mut tx, &nolog)
                .unwrap();
        }
        let req = b"GET / HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n";
        let rn = cli_build(
            CLI_MAC,
            SRV_MAC,
            CLI_IP,
            SRV_IP,
            CLI_PORT,
            80,
            5001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            1000,
            req,
            &mut inbound,
        )
        .unwrap();
        {
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            server
                .on_frame(&inbound[..rn], &rt, &mut api, &mut scratch, &mut tx, &nolog)
                .unwrap();
        }
        let sent = server.sessions[0].resp_sent;
        assert!(sent <= 1000, "window 1000 B, terkirim = {}", sent);
        assert!(sent > 0);
        assert!(server.sessions[0].resp_len > 2000);

        // Buka window -> on_tick harus mengirim sisanya tanpa frame masuk baru.
        server.sessions[0].peer_window = 32768;
        server.sessions[0].snd_una = server.sessions[0].snd_nxt;
        cap.reset();
        {
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            server.on_tick(&rt, &mut scratch, &mut tx, &nolog);
        }
        assert_eq!(server.sessions[0].resp_sent, server.sessions[0].resp_len);
    }

    #[test]
    fn test_16_session_expires_and_slot_is_reclaimed() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        for p in 0..WEB_SESSIONS {
            let mut scratch = [0u8; 1518];
            let mut inbound = [0u8; 1518];
            let n = cli_build(
                CLI_MAC,
                SRV_MAC,
                CLI_IP,
                SRV_IP,
                (CLI_PORT as usize + p) as u16,
                80,
                100,
                0,
                TCP_FLAG_SYN,
                CLI_WIN,
                &[],
                &mut inbound,
            )
            .unwrap();
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            server
                .on_frame(&inbound[..n], &rt, &mut api, &mut scratch, &mut tx, &nolog)
                .unwrap();
        }
        assert_eq!(server.active_sessions(), WEB_SESSIONS as u32);

        // Tabel penuh: SYN berikutnya dijatuhkan, bukan merusak sesi aktif.
        let mut scratch = [0u8; 1518];
        let mut inbound = [0u8; 1518];
        let n = cli_build(
            CLI_MAC,
            SRV_MAC,
            CLI_IP,
            SRV_IP,
            60000,
            80,
            1,
            0,
            TCP_FLAG_SYN,
            CLI_WIN,
            &[],
            &mut inbound,
        )
        .unwrap();
        {
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            server
                .on_frame(&inbound[..n], &rt, &mut api, &mut scratch, &mut tx, &nolog)
                .unwrap();
        }
        assert_eq!(server.stats.session_full_drops, 1);

        // Peer pergi tanpa FIN: tick kedaluwarsa harus mendaur ulang slot.
        for _ in 0..=WEB_SESSION_EXPIRY_TICKS {
            let mut tx = |b: &[u8]| cap.push(b);
            let nolog = |_: &str| {};
            server.on_tick(&rt, &mut scratch, &mut tx, &nolog);
        }
        assert_eq!(server.active_sessions(), 0);
    }

    /// helper: handshake sampai ESTABLISHED, kembalikan seq lanjutannya.
    fn handshake(
        server: &mut TcpWebServer,
        api: &mut ApiDispatcher,
        cap: &mut Cap,
        base: u32,
    ) -> u32 {
        let rt = runtime();
        client_frame(server, &rt, api, base, 0, TCP_FLAG_SYN, &[], cap);
        let our_next = seg_of(&cap.frames[0][..cap.lens[0]]).2.seq.wrapping_add(1);
        client_frame(server, &rt, api, base + 1, our_next, TCP_FLAG_ACK, &[], cap);
        our_next
    }

    #[test]
    fn test_17_post_body_tersplit_belum_memicu_dispatch() {
        // Inti perbaikan jalur tulis: permintaan POST hanya boleh di-dispatch
        // setelah body sepanjang Content-Length mendarat penuh.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let our_next = handshake(&mut server, &mut api, &mut cap, 9000);
        let frames_after_handshake = cap.n;

        let mut hdr = [0u8; 88];
        let mut raw = [0u8; 256];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let head = req_parts(
            &mut raw,
            &[
                b"POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &hdr[..hn],
                b"\r\n",
                b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 67\r\n\r\n",
            ],
        );
        let head_len = head.len();
        client_frame(
            &mut server,
            &rt,
            &mut api,
            9001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            head,
            &mut cap,
        );
        // Baru header: belum boleh ada respons sama sekali.
        assert_eq!(server.stats.api_served, 0);
        assert_eq!(
            cap.n, frames_after_handshake,
            "header saja tidak boleh dijawab"
        );

        let body = b"mode=static&ip=192.0.2.20&netmask=255.255.255.0&name=wan0&persist=1";
        assert_eq!(
            body.len(),
            67,
            "panjang body harus sama dengan Content-Length"
        );
        client_frame(
            &mut server,
            &rt,
            &mut api,
            9001 + head_len as u32,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            body,
            &mut cap,
        );
        assert_eq!(server.stats.api_served, 1);
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains("\"status\":\"queued\""));

        let cmd = api
            .take_write()
            .expect("perintah harus tertampung di antrean");
        assert_eq!(cmd.target_str(), "eth0");
        assert_eq!(cmd.mode, asl_router_api::WAN_MODE_STATIC);
        assert_eq!(cmd.ip, [192, 0, 2, 20]);
        assert_eq!(cmd.netmask, [255, 255, 255, 0]);
        // Nama baru dan flag persistensi ikut terbawa ke kernel, bukan hilang
        // di lapisan transport.
        assert_eq!(cmd.new_name_str(), "wan0");
        assert!(cmd.persist);
    }

    #[test]
    fn test_17b_endpoint_verdict_tulis_terlewat_dari_tcp() {
        // Modal WebUI membaca hasil akhir kernel lewat endpoint ini; ia harus
        // benar-benar terlayani pada jalur TCP nyata, bukan hanya di unit test API.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let our_next = handshake(&mut server, &mut api, &mut cap, 9100);
        let mut hdr = [0u8; 88];
        let mut raw = [0u8; 256];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let req = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/writes/last HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &hdr[..hn],
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut server,
            &rt,
            &mut api,
            9101,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        assert_eq!(server.stats.api_served, 1);
        let (body, len) = reassemble(&cap);
        let text = core::str::from_utf8(&body[..len]).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(
            text.contains("\"applied\":false"),
            "belum ada laporan kernel"
        );
        assert!(text.contains("\"queue_depth\":0"));
    }

    #[test]
    fn test_18_post_ke_luar_sumber_daya_antarmuka_ditolak_405() {
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let our_next = handshake(&mut server, &mut api, &mut cap, 9500);
        let req = b"POST /api/v1/nat HTTP/1.1\r\nHost: 192.0.2.10\r\nContent-Length: 0\r\n\r\n";
        client_frame(
            &mut server,
            &rt,
            &mut api,
            9501,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(text.starts_with("HTTP/1.1 405 Method Not Allowed"));
        assert!(api.take_write().is_none());
    }

    #[test]
    fn test_19_post_power_terlayani_lewat_tcp_bukan_kena_405() {
        // Regresi nyata: gerbang metode di build_response sempat menolak semua
        // POST kecuali /api/v1/interfaces/*, sehingga aksi daya WebUI mati di 405
        // walaupun unit test ApiDispatcher hijau. Uji lewat jalur TCP penuh.
        let rt = runtime();

        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let our_next = handshake(&mut server, &mut api, &mut cap, 9600);
        let mut hdr = [0u8; 88];
        let mut raw = [0u8; 256];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let arm = req_parts(
            &mut raw,
            &[
                b"POST /api/v1/power HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &hdr[..hn],
                b"\r\n",
                b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 10\r\n\r\naction=arm",
            ],
        );
        client_frame(
            &mut server,
            &rt,
            &mut api,
            9601,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            arm,
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(
            text.starts_with("HTTP/1.1 200 OK"),
            "arm seharusnya 200, dapat: {}",
            text
        );
        assert!(text.contains("\"status\":\"armed\""));
        assert!(api.power_armed(rt.tick_100ms), "slot harus ter-arm");

        // Aksi restart memakai slot yang sudah ter-arm dari POST sebelumnya.
        let mut server2 = TcpWebServer::new(80);
        server2.listen(80);
        let mut cap2 = Cap::new();
        let our_next2 = handshake(&mut server2, &mut api, &mut cap2, 9700);
        let restart = req_parts(
            &mut raw,
            &[
                b"POST /api/v1/power HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &hdr[..hn],
                b"\r\n",
                b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 14\r\n\r\naction=restart",
            ],
        );
        client_frame(
            &mut server2,
            &rt,
            &mut api,
            9701,
            our_next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            restart,
            &mut cap2,
        );
        let (resp2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&resp2[..len2]).unwrap();
        assert!(
            text2.starts_with("HTTP/1.1 200 OK"),
            "restart ter-arm seharusnya 200, dapat: {}",
            text2
        );
        assert!(text2.contains("\"status\":\"scheduled\""));
        assert_eq!(
            api.take_power(),
            Some(asl_router_api::POWER_RESTART),
            "kernel harus menerima kinds restart"
        );

        // Metode selain GET/HEAD/POST pada jalur daya tetap 405.
        let mut server3 = TcpWebServer::new(80);
        server3.listen(80);
        let mut cap3 = Cap::new();
        let our_next3 = handshake(&mut server3, &mut api, &mut cap3, 9800);
        client_frame(
            &mut server3,
            &rt,
            &mut api,
            9801,
            our_next3,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            b"PUT /api/v1/power HTTP/1.1\r\nHost: 192.0.2.10\r\nContent-Length: 0\r\n\r\n",
            &mut cap3,
        );
        let (resp3, len3) = reassemble(&cap3);
        let text3 = core::str::from_utf8(&resp3[..len3]).unwrap();
        assert!(text3.starts_with("HTTP/1.1 405"));
        assert!(!api.power_armed(rt.tick_100ms), "PUT tidak boleh meng-arm");
        assert!(api.take_power().is_none());
    }

    #[test]
    fn test_20_conditional_get_dilaporkan_304_bukan_200() {
        // Guard §4.27 lewat jalur TCP sungguhan: `dispatch_static` mengembalikan
        // `Ok` yang sama untuk 200 penuh maupun 304, jadi satu-satunya angka
        // yang boleh dicatat server adalah yang tertulis pada baris status.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let our_next = handshake(&mut server, &mut api, &mut cap, 20000);

        let asset = &registry::VIEW_ARP;
        // Validator `<label>-<panjang>` disusun di tumpukan: crate ini no_std.
        let mut raw = [0u8; 256];
        let mut n = 0usize;
        for b in b"GET /views/arp.html HTTP/1.1\r\nHost: 192.0.2.10\r\nIf-None-Match: \"" {
            raw[n] = *b;
            n += 1;
        }
        for b in asset.etag.as_bytes() {
            raw[n] = *b;
            n += 1;
        }
        raw[n] = b'-';
        n += 1;
        let mut digits = [0u8; 20];
        let mut d = digits.len();
        let mut v = asset.content.len();
        if v == 0 {
            d -= 1;
            digits[d] = b'0';
        }
        while v > 0 {
            d -= 1;
            digits[d] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        for b in &digits[d..] {
            raw[n] = *b;
            n += 1;
        }
        for b in b"\"\r\n\r\n" {
            raw[n] = *b;
            n += 1;
        }

        client_frame(
            &mut server,
            &rt,
            &mut api,
            20001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            &raw[..n],
            &mut cap,
        );
        assert_eq!(
            server.last_status, 304,
            "validasi ulang yang sah sedang dicatat dengan angka lain"
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(
            text.contains("HTTP/1.1 304 Not Modified"),
            "baris status respons bukan 304: {text}"
        );
    }

    #[test]
    fn test_21_framing_ditolak_sebelum_dispatch() {
        // AUD02 DEFER-02/DEFER-03 lewat jalur TCP nyata, bukan hanya unit parser:
        // (a) Transfer-Encoding chunked -> 501, bukan ditafsirkan diam-diam;
        // (b) body kurang dari Content-Length -> server MENUNGGU byte sisa, tidak
        //     men-dispatch POST dengan body kosong;
        // (c) Content-Length di atas batas -> 413 sebelum satu byte body pun tiba.
        // Tiap skenario memakai server baru: satu slot sesi hanya melayani satu
        // permintaan (Connection: close), jadi dua skenario tidak boleh berbagi slot.
        fn put(buf: &mut [u8], off: &mut usize, s: &[u8]) {
            buf[*off..*off + s.len()].copy_from_slice(s);
            *off += s.len();
        }

        // (b) POST terpotong: header tertutup, body baru 3 dari 10 byte.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let our_next = handshake(&mut server, &mut api, &mut cap, 30000);
        let mut raw = [0u8; 512];
        let mut n = 0usize;
        put(&mut raw, &mut n, b"POST /api/v1/power HTTP/1.1\r\nHost: 192.0.2.10\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 10\r\n\r\nact");
        client_frame(
            &mut server,
            &rt,
            &mut api,
            30001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            &raw[..n],
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        assert_eq!(
            len,
            0,
            "permintaan ber-body belum lengkap tidak boleh di-dispatch: {}",
            core::str::from_utf8(&resp[..len]).unwrap_or("?")
        );

        // Sisa body mendarat -> baru dispatch.
        let mut rest = [0u8; 16];
        let mut m = 0usize;
        put(&mut rest, &mut m, b"ion=arm");
        client_frame(
            &mut server,
            &rt,
            &mut api,
            30001 + n as u32,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            &rest[..m],
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(
            text.starts_with("HTTP/1.1 "),
            "POST yang body-nya selesai harus dijawab: {text}"
        );

        // (a) chunked ditolak 501.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let our_next = handshake(&mut server, &mut api, &mut cap, 31000);
        let mut raw = [0u8; 512];
        let mut n = 0usize;
        put(&mut raw, &mut n, b"POST /api/v1/power HTTP/1.1\r\nHost: 192.0.2.10\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n");
        client_frame(
            &mut server,
            &rt,
            &mut api,
            31001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            &raw[..n],
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(
            text.contains("HTTP/1.1 501 Not Implemented"),
            "chunked harus ditolak eksplisit: {text}"
        );
        assert_eq!(server.last_status, 501);

        // (c) body melebihi batas: 413 tanpa menunggu body.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let our_next = handshake(&mut server, &mut api, &mut cap, 32000);
        let mut raw = [0u8; 512];
        let mut n = 0usize;
        put(
            &mut raw,
            &mut n,
            b"POST /api/v1/power HTTP/1.1\r\nHost: 192.0.2.10\r\nContent-Length: 4096\r\n\r\n",
        );
        client_frame(
            &mut server,
            &rt,
            &mut api,
            32001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            &raw[..n],
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(
            text.contains("HTTP/1.1 413 Payload Too Large"),
            "body di atas batas wajib 413 sebelum diproses: {text}"
        );
        assert_eq!(server.last_status, 413);
    }

    /// P0.1 lewat TCP penuh: gerbang autentikasi ada di lapisan server, bukan di
    /// peramban. Empat hal yang dibuktikan di sini:
    /// (a) API anonim -> 401 dengan tantangan Bearer, tanpa data apa pun;
    /// (b) aset statis tetap publik, sehingga kartu masuk bisa dimuat;
    /// (c) login nyata lewat TCP menghasilkan token yang bisa dipakai;
    /// (d) sistem tanpa kredensial menolak seluruh jalur (tidak ada akun gaib).
    #[test]
    fn test_22_gerbang_autentikasi_terlewat_tcp() {
        let rt = runtime();

        // (a) GET API anonim.
        let mut server = TcpWebServer::new(80);
        server.listen(80);
        let mut api = ApiDispatcher::new();
        let mut cap = Cap::new();
        let mut hdr = [0u8; 88];
        let mut raw = [0u8; 512];

        let our_next = handshake(&mut server, &mut api, &mut cap, 33000);
        let req = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/system HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                b"\r\n",
            ],
        );
        client_frame(
            &mut server,
            &rt,
            &mut api,
            33001,
            our_next,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req,
            &mut cap,
        );
        let (resp, len) = reassemble(&cap);
        let text = core::str::from_utf8(&resp[..len]).unwrap();
        assert!(
            text.starts_with("HTTP/1.1 401 Unauthorized"),
            "API anonim harus 401, dapat: {text}"
        );
        assert!(
            text.contains("WWW-Authenticate: Bearer realm=\"rinjani\""),
            "401 tanpa tantangan Bearer: {text}"
        );
        // Respons penolakan tidak boleh membocorkan isi tabel kredensial.
        assert!(!text.contains("admin"));
        assert!(!text.contains("\"uptime"));
        assert_eq!(server.last_status, 401);

        // (b) Aset statis tetap terlayani tanpa sesi: halaman kerja harus bisa
        // dimuat sebelum klien punya kredensial.
        let mut server2 = TcpWebServer::new(80);
        server2.listen(80);
        let mut cap2 = Cap::new();
        let our_next2 = handshake(&mut server2, &mut api, &mut cap2, 33100);
        let req2 = req_parts(
            &mut raw,
            &[
                b"GET /views/arp.html HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                b"\r\n",
            ],
        );
        client_frame(
            &mut server2,
            &rt,
            &mut api,
            33101,
            our_next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req2,
            &mut cap2,
        );
        let (resp2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&resp2[..len2]).unwrap();
        assert!(
            text2.starts_with("HTTP/1.1 200 OK"),
            "jalur statis tidak boleh terkena gerbang API: {text2}"
        );
        assert!(!text2.contains("401"));

        // (c) Login lewat TCP ke dispatcher yang sudah di-bootstrap (mode serial
        // CLI): respons memuat token 64 hex yang kemudian dipakai untuk GET.
        let hn = boot_admin(&mut api, 0, &mut hdr);
        assert_eq!(hn - "Authorization: Bearer ".len(), 64, "token 64 hex");

        let mut server3 = TcpWebServer::new(80);
        server3.listen(80);
        let mut cap3 = Cap::new();
        let our_next3 = handshake(&mut server3, &mut api, &mut cap3, 33200);
        let body = "user=admin&password=rinjani-webui-sesi-uji";
        let mut cl = [0u8; 24];
        let cln = {
            let mut d = [0u8; 20];
            let mut i = d.len();
            let mut v = body.len();
            if v == 0 {
                i -= 1;
                d[i] = b'0';
            }
            while v > 0 {
                i -= 1;
                d[i] = b'0' + (v % 10) as u8;
                v /= 10;
            }
            let k = d.len() - i;
            cl[..k].copy_from_slice(&d[i..]);
            &cl[..k]
        };
        let mut login_raw = [0u8; 512];
        let login = req_parts(
            &mut login_raw,
            &[
                b"POST /api/v1/auth/login HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: ",
                cln,
                b"\r\n\r\n",
                body.as_bytes(),
            ],
        );
        client_frame(
            &mut server3,
            &rt,
            &mut api,
            33201,
            our_next3,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            login,
            &mut cap3,
        );
        let (resp3, len3) = reassemble(&cap3);
        let text3 = core::str::from_utf8(&resp3[..len3]).unwrap();
        assert!(
            text3.starts_with("HTTP/1.1 200 OK"),
            "login sah harus 200, dapat: {text3}"
        );
        assert!(text3.contains("\"token\":\""), "tanpa token sesi: {text3}");
        assert!(text3.contains("\"role\":\"admin\""));
        // Kata sandi tidak pernah kembali ke klien.
        assert!(!text3.contains("rinjani-webui-sesi-uji"));
        assert_eq!(api.auth.logins_ok, 2, "bootstrap + login TCP");

        // Token hasil login harus bisa dipakai lewat GET (dibaca dari respons,
        // bukan dari variabel internal tes).
        let tpos = text3.find("\"token\":\"").expect("token") + 9;
        assert!(
            text3[tpos..tpos + 64]
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
            "token sesi bukan hex 64"
        );
        let mut bearer = [0u8; 88];
        let bn = {
            let line = b"Authorization: Bearer ";
            bearer[..line.len()].copy_from_slice(line);
            bearer[line.len()..line.len() + 64].copy_from_slice(text3[tpos..tpos + 64].as_bytes());
            line.len() + 64
        };
        let mut server4 = TcpWebServer::new(80);
        server4.listen(80);
        let mut cap4 = Cap::new();
        let our_next4 = handshake(&mut server4, &mut api, &mut cap4, 33300);
        let req4 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/system HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &bearer[..bn],
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut server4,
            &rt,
            &mut api,
            33301,
            our_next4,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req4,
            &mut cap4,
        );
        let (resp4, len4) = reassemble(&cap4);
        let text4 = core::str::from_utf8(&resp4[..len4]).unwrap();
        assert!(
            text4.starts_with("HTTP/1.1 200 OK"),
            "sesi hasil login TCP harus diterima: {text4}"
        );

        // (d) Dispatcher kosong (belum ada akun): semua jalur API ditolak dengan
        // kode no_credentials, termasuk login.
        let mut server5 = TcpWebServer::new(80);
        server5.listen(80);
        let mut api5 = ApiDispatcher::new();
        let mut cap5 = Cap::new();
        let our_next5 = handshake(&mut server5, &mut api5, &mut cap5, 33400);
        let req5 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/interfaces HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                b"\r\n",
            ],
        );
        client_frame(
            &mut server5,
            &rt,
            &mut api5,
            33401,
            our_next5,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req5,
            &mut cap5,
        );
        let (resp5, len5) = reassemble(&cap5);
        let text5 = core::str::from_utf8(&resp5[..len5]).unwrap();
        assert!(text5.starts_with("HTTP/1.1 401"), "{text5}");
        assert!(text5.contains("\"error\":\"no_credentials\""), "{text5}");
        assert_eq!(api5.auth.user_count(), 0, "tidak boleh ada akun gaib");
    }

    #[test]
    fn test_23_users_tidak_ke_405_dan_anonim_401_lewat_tcp() {
        // P0.1-D lewat jalur TCP penuh. Gerbang metode di `build_response`
        // dulunya hanya mengenal /api/v1/interfaces/* dan /api/v1/power, jadi
        // seluruh jalur akun mati di 405 sebelum autentikasi sempat berjalan.
        // Empat hal yang dibuktikan: rute lolos gerbang metode, penolakan
        // terjadi di server (anonim 401, viewer 403), tulis admin mengangkat
        // bendera persistensi kernel, dan daftar akun tidak pernah memuat
        // salt/hash/kata sandi.
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut hdr = [0u8; 88];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let mut raw = [0u8; 640];

        const BODY: &[u8] = b"action=set&user=operator&role=viewer&password=sandi-uji-webui-02";
        let bearer = &hdr[..hn];

        // (1) POST anonim -> 401, bukan 405. Akun tidak boleh terbentuk.
        let mut s1 = TcpWebServer::new(80);
        s1.listen(80);
        let mut cap1 = Cap::new();
        let next1 = handshake(&mut s1, &mut api, &mut cap1, 34000);
        let req1 = post_form(&mut raw, b"/api/v1/auth/users", &[], BODY);
        client_frame(
            &mut s1,
            &rt,
            &mut api,
            34001,
            next1,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req1,
            &mut cap1,
        );
        let (resp1, len1) = reassemble(&cap1);
        let text1 = core::str::from_utf8(&resp1[..len1]).unwrap();
        assert!(
            !text1.starts_with("HTTP/1.1 405"),
            "rute akun masih tertutup gerbang metode: {text1}"
        );
        assert!(
            text1.starts_with("HTTP/1.1 401 Unauthorized"),
            "POST akun anonim harus 401, dapat: {text1}"
        );
        assert!(text1.contains("\"error\":\"unauthenticated\""), "{text1}");
        assert_eq!(api.auth.user_count(), 1, "anonim tidak boleh membuat akun");
        assert!(!text1.contains("sandi-uji-webui-02"));

        // (2) POST admin -> 200 + janji antrean persistensi.
        let mut s2 = TcpWebServer::new(80);
        s2.listen(80);
        let mut cap2 = Cap::new();
        let next2 = handshake(&mut s2, &mut api, &mut cap2, 34100);
        let req2 = post_form(&mut raw, b"/api/v1/auth/users", bearer, BODY);
        client_frame(
            &mut s2,
            &rt,
            &mut api,
            34101,
            next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req2,
            &mut cap2,
        );
        let (resp2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&resp2[..len2]).unwrap();
        assert!(
            text2.starts_with("HTTP/1.1 200 OK"),
            "POST akun admin harus 200, dapat: {text2}"
        );
        assert!(text2.contains("\"action\":\"set\""), "{text2}");
        assert!(text2.contains("\"user\":\"operator\""), "{text2}");
        assert!(text2.contains("\"role\":\"viewer\""), "{text2}");
        // ACK hanya menjanjikan antrean; nasib RJCFG01 dibaca dari verdict tulis.
        assert!(text2.contains("\"persist\":\"queued\""), "{text2}");
        assert!(!text2.contains("sandi-uji-webui-02"), "sandi echo: {text2}");
        assert_eq!(api.auth.user_count(), 2);
        assert!(api.auth_dirty, "perubahan akun harus mengangkat bendera");

        // (3) GET daftar akun oleh admin: nama, role, iterasi — tanpa secret.
        let mut s3 = TcpWebServer::new(80);
        s3.listen(80);
        let mut cap3 = Cap::new();
        let next3 = handshake(&mut s3, &mut api, &mut cap3, 34200);
        let req3 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/auth/users HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s3,
            &rt,
            &mut api,
            34201,
            next3,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req3,
            &mut cap3,
        );
        let (resp3, len3) = reassemble(&cap3);
        let text3 = core::str::from_utf8(&resp3[..len3]).unwrap();
        assert!(text3.starts_with("HTTP/1.1 200 OK"), "{text3}");
        assert!(text3.contains("\"user\":\"operator\""), "{text3}");
        assert!(text3.contains("\"rounds\":1024"), "{text3}");
        for banned in ["salt", "hash", "password", "sandi-uji-webui-02", "Bearer "] {
            assert!(
                !text3.contains(banned),
                "daftar akun membocorkan {banned}: {text3}"
            );
        }

        // (4) Role viewer: GET dan POST akun dua-duanya 403 di lapisan server,
        // bukan sekadar tombol yang hilang di peramban.
        let mut s4 = TcpWebServer::new(80);
        s4.listen(80);
        let mut cap4 = Cap::new();
        let next4 = handshake(&mut s4, &mut api, &mut cap4, 34300);
        let login_body = b"user=operator&password=sandi-uji-webui-02";
        let req4 = post_form(&mut raw, b"/api/v1/auth/login", &[], login_body);
        client_frame(
            &mut s4,
            &rt,
            &mut api,
            34301,
            next4,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req4,
            &mut cap4,
        );
        let (resp4, len4) = reassemble(&cap4);
        let text4 = core::str::from_utf8(&resp4[..len4]).unwrap();
        assert!(
            text4.starts_with("HTTP/1.1 200 OK"),
            "login akun baru harus 200, dapat: {text4}"
        );
        let tpos = text4.find("\"token\":\"").expect("token viewer") + 9;
        let mut vbearer = [0u8; 88];
        let vn = {
            let line = b"Authorization: Bearer ";
            vbearer[..line.len()].copy_from_slice(line);
            vbearer[line.len()..line.len() + 64].copy_from_slice(text4[tpos..tpos + 64].as_bytes());
            line.len() + 64
        };

        let mut s5 = TcpWebServer::new(80);
        s5.listen(80);
        let mut cap5 = Cap::new();
        let next5 = handshake(&mut s5, &mut api, &mut cap5, 34400);
        let req5 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/auth/users HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                &vbearer[..vn],
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s5,
            &rt,
            &mut api,
            34401,
            next5,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req5,
            &mut cap5,
        );
        let (resp5, len5) = reassemble(&cap5);
        let text5 = core::str::from_utf8(&resp5[..len5]).unwrap();
        assert!(
            text5.starts_with("HTTP/1.1 403 Forbidden"),
            "viewer tidak boleh membaca tabel akun: {text5}"
        );
        assert!(text5.contains("\"error\":\"insufficient_role\""), "{text5}");
        assert!(!text5.contains("\"user\":\"operator\""));

        let mut s6 = TcpWebServer::new(80);
        s6.listen(80);
        let mut cap6 = Cap::new();
        let next6 = handshake(&mut s6, &mut api, &mut cap6, 34500);
        let req6 = post_form(
            &mut raw,
            b"/api/v1/auth/users",
            &vbearer[..vn],
            b"action=del&user=admin",
        );
        client_frame(
            &mut s6,
            &rt,
            &mut api,
            34501,
            next6,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req6,
            &mut cap6,
        );
        let (resp6, len6) = reassemble(&cap6);
        let text6 = core::str::from_utf8(&resp6[..len6]).unwrap();
        assert!(
            text6.starts_with("HTTP/1.1 403 Forbidden"),
            "viewer tidak boleh menghapus admin: {text6}"
        );
        assert_eq!(
            api.auth.user_count(),
            2,
            "penolakan role tidak mengubah tabel"
        );
    }

    #[test]
    fn test_24_tabel_arp_terlayani_lewat_tcp_dan_hanya_baca() {
        // Modul ARP Table naik dari "desain saja" menjadi data kernel nyata.
        // Yang dibuktikan di sini ada empat: rute baru lolos gerbang metode,
        // pembacaan anonim ditolak di lapisan server (bukan tombol yang
        // disembunyikan), baris yang keluar adalah milik WebRuntime apa adanya
        // (IP/MAC/umur dari tick, tidak ada yang ditambah-tambahi), dan tidak
        // ada jalur tulis menuju tabel sehingga POST mati di 405.
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut hdr = [0u8; 88];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let bearer = &hdr[..hn];
        let mut raw = [0u8; 512];

        // (1) Anonim -> 401 dan tabel tidak bocor sama sekali.
        let mut s1 = TcpWebServer::new(80);
        s1.listen(80);
        let mut cap1 = Cap::new();
        let next1 = handshake(&mut s1, &mut api, &mut cap1, 35000);
        let req1 = req_parts(
            &mut raw,
            &[b"GET /api/v1/arp HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n"],
        );
        client_frame(
            &mut s1,
            &rt,
            &mut api,
            35001,
            next1,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req1,
            &mut cap1,
        );
        let (resp1, len1) = reassemble(&cap1);
        let text1 = core::str::from_utf8(&resp1[..len1]).unwrap();
        assert!(
            text1.starts_with("HTTP/1.1 401 Unauthorized"),
            "GET tabel ARP anonim harus 401, dapat: {text1}"
        );
        assert!(text1.contains("\"error\":\"unauthenticated\""), "{text1}");
        assert!(
            !text1.contains("08:00:27"),
            "MAC bocor ke klien anonim: {text1}"
        );
        assert!(
            !text1.contains("192.0.2.20"),
            "IP bocor ke klien anonim: {text1}"
        );

        // (2) Admin -> 200 berisi dua baris fixture persis seperti snapshot.
        let mut s2 = TcpWebServer::new(80);
        s2.listen(80);
        let mut cap2 = Cap::new();
        let next2 = handshake(&mut s2, &mut api, &mut cap2, 35100);
        let req2 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/arp HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s2,
            &rt,
            &mut api,
            35101,
            next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req2,
            &mut cap2,
        );
        let (resp2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&resp2[..len2]).unwrap();
        assert!(
            text2.starts_with("HTTP/1.1 200 OK"),
            "GET tabel ARP admin harus 200, dapat: {text2}"
        );
        // Baris WAN: ifindex 0 memakai nama antarmuka WAN, opcode reply.
        assert!(text2.contains("\"iface\":\"eth0\""), "{text2}");
        assert!(text2.contains("\"ip\":\"192.0.2.20\""), "{text2}");
        assert!(text2.contains("\"mac\":\"08:00:27:0a:00:02\""), "{text2}");
        assert!(text2.contains("\"source\":\"reply\""), "{text2}");
        assert!(text2.contains("\"seen\":4"), "{text2}");
        // Umur dari tick: (100 - 90) tick x 100 ms = 1 s, dihitung kernel lewat.
        assert!(text2.contains("\"age_s\":1}"), "{text2}");
        // Baris LAN: ifindex 1 memakai nama antarmuka LAN, opcode request.
        assert!(text2.contains("\"iface\":\"eth1\""), "{text2}");
        assert!(text2.contains("\"ip\":\"192.0.2.31\""), "{text2}");
        assert!(text2.contains("\"mac\":\"08:00:27:0a:00:1f\""), "{text2}");
        assert!(text2.contains("\"source\":\"request\""), "{text2}");
        // (100 - 95) / 10 = 0,5 s -> 0 s. Jangan ada angka kesegaran karangan.
        assert!(text2.contains("\"age_s\":0}"), "{text2}");
        assert!(!text2.contains("\"reachable\""), "status karangan: {text2}");
        assert_eq!(
            text2.matches("\"iface\":").count(),
            2,
            "jumlah baris harus sama dengan snapshot: {text2}"
        );

        // (3) Statistik terpisah: kapasitas & penolakan terlihat, bukan hilang
        // di balik tabel yang tampak terisi wajar.
        let mut s3 = TcpWebServer::new(80);
        s3.listen(80);
        let mut cap3 = Cap::new();
        let next3 = handshake(&mut s3, &mut api, &mut cap3, 35200);
        let req3 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/arp/stats HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s3,
            &rt,
            &mut api,
            35201,
            next3,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req3,
            &mut cap3,
        );
        let (resp3, len3) = reassemble(&cap3);
        let text3 = core::str::from_utf8(&resp3[..len3]).unwrap();
        assert!(
            text3.starts_with("HTTP/1.1 200 OK"),
            "GET statistik ARP harus 200, dapat: {text3}"
        );
        assert!(text3.contains("\"entries\":2"), "{text3}");
        assert!(text3.contains("\"capacity\":16"), "{text3}");
        assert!(text3.contains("\"timeout_s\":20"), "{text3}");
        assert!(text3.contains("\"evictions\":0"), "{text3}");
        assert!(text3.contains("\"rejected\":0"), "{text3}");

        // (4) Tidak ada jalur tulis tabel: POST ditolak gerbang metode sebelum
        // menyentuh autentikasi, dan tidak mengangkat bendera persistensi.
        let mut s4 = TcpWebServer::new(80);
        s4.listen(80);
        let mut cap4 = Cap::new();
        let next4 = handshake(&mut s4, &mut api, &mut cap4, 35300);
        let req4 = post_form(&mut raw, b"/api/v1/arp", bearer, b"ip=192.0.2.99");
        client_frame(
            &mut s4,
            &rt,
            &mut api,
            35301,
            next4,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req4,
            &mut cap4,
        );
        let (resp4, len4) = reassemble(&cap4);
        let text4 = core::str::from_utf8(&resp4[..len4]).unwrap();
        assert!(
            text4.starts_with("HTTP/1.1 405"),
            "tabel ARP tidak punya jalur tulis, harus 405: {text4}"
        );
        assert!(!api.auth_dirty, "POST tabel ARP tidak boleh menulis config");
        assert!(api.take_write().is_none());
    }

    #[test]
    fn test_25_rute_terlayani_lewat_tcp_dan_pencacah_tidak_dibagus() {
        // Modul Gateway naik dari "desain saja" menjadi isi FIB kernel. Yang
        // ditagih di sini: rute lolos gerbang metode + gerbang sesi, baris yang
        // keluar hanyalah salinan snapshot (termasuk rute tanpa antarmuka yang
        // tetap strip), statistik memisahkan "tabel terisi" dari "datapath
        // bertanya", dan tidak ada jalur tulis rute lewat HTTP.
        let rt = runtime();
        let mut api = ApiDispatcher::new();
        let mut hdr = [0u8; 88];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let bearer = &hdr[..hn];
        let mut raw = [0u8; 512];

        // (1) Anonim -> 401 di lapisan server, bukan 404 dan bukan tabel.
        let mut s1 = TcpWebServer::new(80);
        s1.listen(80);
        let mut cap1 = Cap::new();
        let next1 = handshake(&mut s1, &mut api, &mut cap1, 36000);
        let req1 = req_parts(
            &mut raw,
            &[b"GET /api/v1/routes HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n"],
        );
        client_frame(
            &mut s1,
            &rt,
            &mut api,
            36001,
            next1,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req1,
            &mut cap1,
        );
        let (resp1, len1) = reassemble(&cap1);
        let text1 = core::str::from_utf8(&resp1[..len1]).unwrap();
        assert!(
            text1.starts_with("HTTP/1.1 401 Unauthorized"),
            "GET rute anonim harus 401, dapat: {text1}"
        );
        assert!(text1.contains("\"error\":\"unauthenticated\""), "{text1}");
        assert!(!text1.contains("\"dst\""), "rute bocor ke anonim: {text1}");

        // (2) Admin -> tiga baris FIB, persis seperti snapshot kernel.
        let mut s2 = TcpWebServer::new(80);
        s2.listen(80);
        let mut cap2 = Cap::new();
        let next2 = handshake(&mut s2, &mut api, &mut cap2, 36100);
        let req2 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/routes HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s2,
            &rt,
            &mut api,
            36101,
            next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req2,
            &mut cap2,
        );
        let (resp2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&resp2[..len2]).unwrap();
        assert!(
            text2.starts_with("HTTP/1.1 200 OK"),
            "GET rute admin harus 200, dapat: {text2}"
        );
        assert!(
            text2.contains(
                "{\"dst\":\"0.0.0.0\",\"prefix\":0,\"gw\":\"192.0.2.1\",\"iface\":\"eth0\",\"metric\":100,\"type\":\"static\"}"
            ),
            "rute default salah: {text2}"
        );
        assert!(
            text2.contains(
                "{\"dst\":\"192.0.2.0\",\"prefix\":24,\"gw\":\"0.0.0.0\",\"iface\":\"eth0\",\"metric\":0,\"type\":\"connected\"}"
            ),
            "rute connected salah: {text2}"
        );
        // Rute tanpa antarmuka keluar tidak boleh diberi nama antarmuka.
        assert!(
            text2.contains(
                "{\"dst\":\"10.99.0.0\",\"prefix\":16,\"gw\":\"0.0.0.0\",\"iface\":\"-\",\"metric\":0,\"type\":\"blackhole\"}"
            ),
            "rute tanpa antarmuka salah: {text2}"
        );
        assert_eq!(text2.matches("\"dst\":").count(), 3, "{text2}");

        // (3) Statistik: angka datapath dilaporkan apa adanya, termasuk nol.
        let mut s3 = TcpWebServer::new(80);
        s3.listen(80);
        let mut cap3 = Cap::new();
        let next3 = handshake(&mut s3, &mut api, &mut cap3, 36200);
        let req3 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/routes/stats HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s3,
            &rt,
            &mut api,
            36201,
            next3,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req3,
            &mut cap3,
        );
        let (resp3, len3) = reassemble(&cap3);
        let text3 = core::str::from_utf8(&resp3[..len3]).unwrap();
        assert!(
            text3.starts_with("HTTP/1.1 200 OK"),
            "GET statistik rute harus 200, dapat: {text3}"
        );
        for want in [
            "\"entries\":3",
            "\"capacity\":64",
            "\"lookups\":5",
            "\"forwards\":0",
            "\"drops\":5",
            "\"default_active\":true",
            "\"default_gw\":\"192.0.2.1\"",
            "\"snapshot\":3",
        ] {
            assert!(text3.contains(want), "{want} hilang: {text3}");
        }
        // `forwards` 0 harus benar-benar tampil 0, bukan dibuang dari respons.
        assert!(text3.contains("\"forwards\":0"), "{text3}");

        // (4) Rute tidak punya jalur tulis HTTP: ditolak gerbang metode, dan
        // tidak mengangkat bendera persistensi maupun mengisi antrean tulis.
        let mut s4 = TcpWebServer::new(80);
        s4.listen(80);
        let mut cap4 = Cap::new();
        let next4 = handshake(&mut s4, &mut api, &mut cap4, 36300);
        let req4 = post_form(&mut raw, b"/api/v1/routes", bearer, b"dst=10.0.0.0");
        client_frame(
            &mut s4,
            &rt,
            &mut api,
            36301,
            next4,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req4,
            &mut cap4,
        );
        let (resp4, len4) = reassemble(&cap4);
        let text4 = core::str::from_utf8(&resp4[..len4]).unwrap();
        assert!(
            text4.starts_with("HTTP/1.1 405"),
            "tabel rute tidak punya jalur tulis, harus 405: {text4}"
        );
        assert!(!api.auth_dirty, "POST rute tidak boleh menulis config");
        assert!(api.take_write().is_none());
    }

    #[test]
    fn test_26_dns_terlayani_lewat_tcp_dengan_resolver_dari_lease() {
        // Modul DNS Server naik dari "desain saja" menjadi pembaca lease + mesin
        // proksi. Yang ditagih di sini: jalur baca terkunci sesi, angka yang
        // keluar adalah pasangan (lease -> mesin) dan bukan resolver publik
        // karangan, ketiadaan opsi 6 dilaporkan sebagai nol (bukan diisi
        // cadangan), dan tidak ada jalur tulis DNS lewat HTTP.
        let mut rt = runtime();
        rt.wan_config_static = false;
        rt.wan_source = "dhcp";
        rt.dhcp_client_active = true;
        let mut api = ApiDispatcher::new();
        let mut hdr = [0u8; 88];
        let hn = boot_admin(&mut api, 0, &mut hdr);
        let bearer = &hdr[..hn];
        let mut raw = [0u8; 512];

        // (1) Anonim -> 401, tanpa kebocoran setelan resolver.
        let mut s1 = TcpWebServer::new(80);
        s1.listen(80);
        let mut cap1 = Cap::new();
        let next1 = handshake(&mut s1, &mut api, &mut cap1, 36500);
        let req1 = req_parts(
            &mut raw,
            &[b"GET /api/v1/dns HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n"],
        );
        client_frame(
            &mut s1,
            &rt,
            &mut api,
            36501,
            next1,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req1,
            &mut cap1,
        );
        let (resp1, len1) = reassemble(&cap1);
        let text1 = core::str::from_utf8(&resp1[..len1]).unwrap();
        assert!(
            text1.starts_with("HTTP/1.1 401 Unauthorized"),
            "GET dns anonim harus 401, dapat: {text1}"
        );
        assert!(text1.contains("\"error\":\"unauthenticated\""), "{text1}");
        assert!(
            !text1.contains("resolver_1"),
            "setelan resolver bocor ke anonim: {text1}"
        );

        // (2) Admin + lease DHCP ber-opsi 6: nilai lease DAN nilai mesin
        //     ditampilkan, keduanya menunjuk angka yang sama.
        let mut s2 = TcpWebServer::new(80);
        s2.listen(80);
        let mut cap2 = Cap::new();
        let next2 = handshake(&mut s2, &mut api, &mut cap2, 36600);
        let req2 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/dns HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s2,
            &rt,
            &mut api,
            36601,
            next2,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req2,
            &mut cap2,
        );
        let (resp2, len2) = reassemble(&cap2);
        let text2 = core::str::from_utf8(&resp2[..len2]).unwrap();
        assert!(
            text2.starts_with("HTTP/1.1 200 OK"),
            "GET dns admin harus 200, dapat: {text2}"
        );
        for want in [
            "\"config_mode\":\"dhcp\"",
            "\"lease_source\":\"dhcp\"",
            "\"resolver_1\":\"192.0.2.53\"",
            "\"resolver_2\":\"0.0.0.0\"",
            "\"resolver_src\":\"option-6-offer\"",
            "\"from_dhcp\":true",
            "\"proxy_upstream\":\"192.0.2.53\"",
            "\"proxy_upstream_ready\":true",
            "\"serving\":false",
            "\"cache_hits\":11",
            "\"cache_misses\":5",
            "\"cache_entries\":3",
            "\"cache_capacity\":16",
            "\"static_hosts\":1",
            "\"hosts_capacity\":8",
        ] {
            assert!(text2.contains(want), "{want} hilang: {text2}");
        }
        // Resolver publik tidak boleh muncul sebagai nilai bawaan.
        for banned in ["8.8.8.8", "8.8.4.4", "1.1.1.1", "192.0.2.254"] {
            assert!(!text2.contains(banned), "dns mengarang {banned}: {text2}");
        }

        // (3) Server tanpa opsi 6: fail-closed, bukan penebakan.
        rt.wan_dns1 = [0, 0, 0, 0];
        rt.wan_dns_src = "none";
        rt.dns_upstream = [0, 0, 0, 0];
        rt.dns_cache_entries = 0;
        rt.dns_cache_hits = 0;
        rt.dns_cache_misses = 0;
        let mut s3 = TcpWebServer::new(80);
        s3.listen(80);
        let mut cap3 = Cap::new();
        let next3 = handshake(&mut s3, &mut api, &mut cap3, 36700);
        let req3 = req_parts(
            &mut raw,
            &[
                b"GET /api/v1/dns HTTP/1.1\r\nHost: 192.0.2.10\r\n",
                bearer,
                b"\r\n\r\n",
            ],
        );
        client_frame(
            &mut s3,
            &rt,
            &mut api,
            36701,
            next3,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req3,
            &mut cap3,
        );
        let (resp3, len3) = reassemble(&cap3);
        let text3 = core::str::from_utf8(&resp3[..len3]).unwrap();
        assert!(
            text3.starts_with("HTTP/1.1 200 OK"),
            "dns tanpa opsi 6 tetap harus terbaca: {text3}"
        );
        for want in [
            "\"resolver_1\":\"0.0.0.0\"",
            "\"resolver_src\":\"none\"",
            "\"from_dhcp\":false",
            "\"proxy_upstream\":\"0.0.0.0\"",
            "\"proxy_upstream_ready\":false",
            "\"cache_entries\":0",
        ] {
            assert!(text3.contains(want), "{want} hilang: {text3}");
        }

        // (4) Setelan DNS belum punya jalur tulis HTTP: gerbang metode menolak,
        //     bendera persistensi tidak naik, antrean tulis tetap kosong.
        let mut s4 = TcpWebServer::new(80);
        s4.listen(80);
        let mut cap4 = Cap::new();
        let next4 = handshake(&mut s4, &mut api, &mut cap4, 36800);
        let req4 = post_form(&mut raw, b"/api/v1/dns", bearer, b"upstream=192.0.2.53");
        client_frame(
            &mut s4,
            &rt,
            &mut api,
            36801,
            next4,
            TCP_FLAG_ACK | TCP_FLAG_PSH,
            req4,
            &mut cap4,
        );
        let (resp4, len4) = reassemble(&cap4);
        let text4 = core::str::from_utf8(&resp4[..len4]).unwrap();
        assert!(
            text4.starts_with("HTTP/1.1 405"),
            "setelan DNS tidak punya jalur tulis, harus 405: {text4}"
        );
        assert!(!api.auth_dirty, "POST dns tidak boleh menulis config");
        assert!(api.take_write().is_none());
    }
}
