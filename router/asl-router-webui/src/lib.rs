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
use asl_router_webcore::{serve_static, HttpMethod, HttpRequest, HttpResponseBuilder, HttpStatus};

pub fn dispatch_static(
    req: &HttpRequest,
    out_buf: &mut [u8],
) -> Option<Result<usize, &'static str>> {
    let path = if req.path == "/"
        || req.path == "/status"
        || req.path == "/interfaces"
        || req.path == "/firewall"
        || req.path == "/services"
    {
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

/// Jumlah sesi TCP paralel yang dapat dilayani (browser membuka beberapa
/// koneksi sekaligus untuk aset + polling API).
pub const WEB_SESSIONS: usize = 4;
/// Batas bytes header permintaan yang dikumpulkan per sesi. Header browser
/// modern (sec-ch-ua, Accept-Encoding, Range) bisa melewati 1 KiB.
pub const WEB_REQ_MAX: usize = 2048;
/// Batas bytes respons yang dibangun per sesi. Harus menampung aset terbesar
/// yang di-embed (js/status.js) + header HTTP; Lihat gerbang konstan di bawah.
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

        // Dispatch HTTP ketika permintaan lengkap (terminator header \r\n\r\n).
        if s.state == TcpSocketState::Established
            && !s.responded
            && find_double_crlf(&self.req[slot][..s.req_len]).is_some()
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
            if s.state == TcpSocketState::FinWait || s.state == TcpSocketState::SynReceived {
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
            Err(_) => {
                let b = HttpResponseBuilder::new(out, HttpStatus::BadRequest)
                    .with_content_type("application/json");
                if let Ok(n) = b.finish_with_body(b"{\"ok\":false,\"error\":\"bad_request\"}") {
                    s.resp_len = n;
                    self.stats.responses_400 += 1;
                    self.last_status = 400;
                }
                s.responded = true;
                log("[R28] HTTP 400 (request tidak valid secara sintaksis)\r\n");
            }
            Ok(req) => {
                let is_head = req.method == HttpMethod::Head;
                let pl = req.path.len().min(64);
                self.last_path[..pl].copy_from_slice(&req.path.as_bytes()[..pl]);
                self.last_path_len = pl as u8;
                if req.method != HttpMethod::Get && !is_head {
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
                    match api.dispatch(&req, rt.tick_100ms, out, rt) {
                        Ok(n) => {
                            s.resp_len = n;
                            self.stats.api_served += 1;
                            self.last_status = 200;
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
                            s.resp_len = n;
                            self.last_status = 200;
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
        assert_eq!(registry::MODULES.len(), 4);
        assert_eq!(registry::MODULES[0].name, "Dashboard");
        assert!(registry::MODULES[0].enabled);
    }

    #[test]
    fn test_02_all_assets_present() {
        assert_eq!(registry::ALL_ASSETS.len(), 9);
        assert_eq!(
            registry::SHELL_HTML.content_type,
            "text/html; charset=utf-8"
        );
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
        assert!(resp.contains("--pf-bg-main"));
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

    #[test]
    fn test_06_dispatch_logo_svg() {
        let req = HttpRequest::parse(b"GET /images/logo.svg HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 4096];
        let res = dispatch_static(&req, &mut buf).expect("Must find logo asset");
        let len = res.unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("Content-Type: image/svg+xml"));
        assert!(resp.contains("MitraNet Logo"));
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
            wan_ip: SRV_IP,
            wan_netmask: [255, 255, 255, 0],
            wan_gw: [192, 0, 2, 1],
            wan_up: true,
            wan_source: "static",
            wan_mac: SRV_MAC,
            http_listening: true,
            http_port: 80,
            serial_cli_active: true,
            ..Default::default()
        }
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
        let req = b"GET /api/v1/interfaces HTTP/1.1\r\nHost: 192.0.2.10\r\n\r\n";
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
        // eth1 tidak ada pada topologi satu-NIC -> wajib dilaporkan absent.
        assert!(text.contains("\"status\":\"absent\""));
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
}
