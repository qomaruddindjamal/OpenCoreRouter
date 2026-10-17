//! `asl-router-webcore` — HTTP/1.1 Engine & Core Web Dispatcher
//!
//! Bare-metal `#![no_std]` HTTP/1.1 engine adhering to RFC 7230 / RFC 7231:
//! - Method parsing (GET, POST, OPTIONS, HEAD, PUT, DELETE)
//! - Path & query string separation
//! - Header parsing (Host, Content-Type, Content-Length, If-None-Match, Connection)
//! - Security headers: `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Cache-Control`
//! - Static resource delivery with content-derived ETag validation & HTTP 304
//!   Not Modified (validator = `"<label>-<panjang isi>"`, sehingga suntingan aset
//!   selalu membatalkan cache browser walau URL dan label versi tetap)
//! - Fixed-buffer response serialization (zero heap allocations)

#![no_std]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Head,
    Options,
    Put,
    Delete,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    Ok = 200,
    Created = 201,
    NoContent = 204,
    NotModified = 304,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    PayloadTooLarge = 413,
    TooManyRequests = 429,
    NotImplemented = 501,
    InternalServerError = 500,
}

impl HttpStatus {
    pub const fn code(self) -> u16 {
        self as u16
    }

    pub const fn reason_phrase(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Created => "Created",
            Self::NoContent => "No Content",
            Self::NotModified => "Not Modified",
            Self::BadRequest => "Bad Request",
            Self::Unauthorized => "Unauthorized",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::TooManyRequests => "Too Many Requests",
            Self::NotImplemented => "Not Implemented",
            Self::InternalServerError => "Internal Server Error",
        }
    }
}

/// Batas body yang diterima server. Nilai ini menegakkan DEFER-03: permintaan
/// yang mendeklarasikan body lebih besar ditolak sebelum satu byte body pun
/// diproses, bukan dibiarkan mengisi buffer sesi.
pub const MAX_BODY_BYTES: usize = 1024;

/// Penolakan permintaan. `status` adalah kode yang dikirim ke klien; `reason`
/// hanya untuk log konsol dan tidak pernah masuk respons, supaya bentuk internal
/// parser tidak bocor ke jaringan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub status: HttpStatus,
    pub reason: &'static str,
}

impl ParseError {
    const fn bad(reason: &'static str) -> Self {
        Self {
            status: HttpStatus::BadRequest,
            reason,
        }
    }

    const fn too_large() -> Self {
        Self {
            status: HttpStatus::PayloadTooLarge,
            reason: "body melebihi MAX_BODY_BYTES",
        }
    }

    const fn not_implemented(reason: &'static str) -> Self {
        Self {
            status: HttpStatus::NotImplemented,
            reason,
        }
    }
}

/// Pemisah baris versi HTTP/1.0 yang tidak didahului CR tidak boleh dianggap
/// penutup header (RFC 7230 §3.1.1 melarang penerimanya menerjemahkan LF telanjang).
fn has_bare_lf(text: &str) -> bool {
    let b = text.as_bytes();
    b.iter()
        .enumerate()
        .any(|(i, &c)| c == b'\n' && (i == 0 || b[i - 1] != b'\r'))
}

/// Urai nilai `Content-Length` secara ketat: desimal murni tanpa tanda, tanpa
/// spasi internal, maksimum 9 digit (sehingga tidak mungkin overflow), dan tidak
/// boleh melebihi `MAX_BODY_BYTES`.
fn parse_content_length(val: &str) -> Result<usize, ParseError> {
    let b = val.as_bytes();
    if b.is_empty() || b.len() > 9 {
        return Err(ParseError::bad("Content-Length bukan desimal valid"));
    }
    let mut n = 0usize;
    for c in b {
        if !c.is_ascii_digit() {
            return Err(ParseError::bad("Content-Length bukan desimal valid"));
        }
        n = n * 10 + (c - b'0') as usize;
    }
    if n > MAX_BODY_BYTES {
        return Err(ParseError::too_large());
    }
    Ok(n)
}

#[derive(Debug, Default)]
struct HeaderFields<'a> {
    host: Option<&'a str>,
    if_none_match: Option<&'a str>,
    content_type: Option<&'a str>,
    authorization: Option<&'a str>,
    content_length: Option<usize>,
}

/// Satu-satunya penafsir blok header. Gerbang kelengkapan byte dan pengurai
/// memakai fungsi yang sama, sehingga tidak ada perbedaan pembacaan framing
/// antara lapisan terima dan lapisan dispatch.
///
/// `block` wajib berisi request-line + baris header dan diakhiri baris kosong.
fn scan_headers<'a>(block: &'a str) -> Result<HeaderFields<'a>, ParseError> {
    let mut f = HeaderFields::default();
    for (idx, line) in block.split("\r\n").enumerate() {
        if idx == 0 {
            continue; // request-line, bukan header
        }
        if line.is_empty() {
            break; // penutup header
        }
        let first = line.as_bytes()[0];
        if first == b' ' || first == b'\t' {
            return Err(ParseError::bad("obs-fold (kelanjutan baris header)"));
        }
        let Some((raw_name, raw_val)) = line.split_once(':') else {
            return Err(ParseError::bad("baris header tanpa kolon"));
        };
        let name = raw_name.trim();
        let val = raw_val.trim();
        if name.is_empty() {
            return Err(ParseError::bad("nama header kosong"));
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            // Server tidak mendukung chunked: ditolak eksplisit, bukan
            // diam-diam dibaca sebagai Content-Length.
            return Err(ParseError::not_implemented(
                "Transfer-Encoding tidak didukung server ini",
            ));
        } else if name.eq_ignore_ascii_case("content-length") {
            let n = parse_content_length(val)?;
            match f.content_length {
                Some(prev) if prev != n => {
                    return Err(ParseError::bad("Content-Length ganda tidak identik"))
                }
                Some(_) => {}
                None => f.content_length = Some(n),
            }
        } else if name.eq_ignore_ascii_case("host") {
            if f.host.is_some() {
                return Err(ParseError::bad("header Host ganda"));
            }
            f.host = Some(val);
        } else if name.eq_ignore_ascii_case("if-none-match") {
            if f.if_none_match.is_none() {
                f.if_none_match = Some(val);
            }
        } else if name.eq_ignore_ascii_case("content-type") {
            if f.content_type.is_some() {
                return Err(ParseError::bad("header Content-Type ganda"));
            }
            f.content_type = Some(val);
        } else if name.eq_ignore_ascii_case("authorization") {
            if f.authorization.is_some() {
                return Err(ParseError::bad("header Authorization ganda"));
            }
            f.authorization = Some(val);
        }
    }
    Ok(f)
}

/// Indeks byte tepat setelah `\r\n\r\n` pada `raw`, atau `None` bila header
/// belum tertutup.
#[must_use]
pub fn body_start_of(raw: &[u8]) -> Option<usize> {
    let text = core::str::from_utf8(raw).ok()?;
    text.find("\r\n\r\n").map(|idx| idx + 4)
}

/// Panjang body yang dideklarasikan blok header, dipakai gerbang kelengkapan
/// sebelum dispatch. `Err` berarti permintaan harus ditolak sekarang — pemanggil
/// tidak boleh menunggu byte tambahan untuk framing yang sudah pasti salah.
pub fn declared_body_len(header_block: &[u8]) -> Result<Option<usize>, ParseError> {
    let text = core::str::from_utf8(header_block)
        .map_err(|_| ParseError::bad("blok header bukan UTF-8"))?;
    if has_bare_lf(text) {
        return Err(ParseError::bad("LF telanjang di blok header"));
    }
    Ok(scan_headers(text)?.content_length)
}

pub struct HttpRequest<'a> {
    pub method: HttpMethod,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub host: Option<&'a str>,
    pub if_none_match: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub authorization: Option<&'a str>,
    pub content_length: usize,
    pub body: &'a [u8],
}

impl<'a> HttpRequest<'a> {
    /// Urai satu permintaan penuh. Semua penolakan menghasilkan `ParseError`
    /// dengan kode status yang benar (400/413/501), bukan `Ok` dengan field
    /// setengah terisi.
    pub fn parse(raw: &'a [u8]) -> Result<Self, ParseError> {
        let text =
            core::str::from_utf8(raw).map_err(|_| ParseError::bad("permintaan bukan UTF-8"))?;
        let body_start = text
            .find("\r\n\r\n")
            .ok_or(ParseError::bad("header belum tertutup"))?
            + 4;
        let header_block = &text[..body_start];
        // Pemeriksaan LF telanjang hanya pada blok header: body sah memuat byte
        // apa pun, termasuk LF tanpa CR.
        if has_bare_lf(header_block) {
            return Err(ParseError::bad("LF telanjang di blok header"));
        }
        let mut lines = header_block.split("\r\n");

        let request_line = lines.next().ok_or(ParseError::bad("permintaan kosong"))?;
        let mut req_parts = request_line.split_ascii_whitespace();
        let method_str = req_parts
            .next()
            .ok_or(ParseError::bad("metode HTTP hilang"))?;
        let full_path = req_parts
            .next()
            .ok_or(ParseError::bad("path permintaan hilang"))?;
        let version = req_parts
            .next()
            .ok_or(ParseError::bad("versi HTTP hilang"))?;
        if req_parts.next().is_some() {
            return Err(ParseError::bad(
                "request-line memiliki lebih dari tiga bagian",
            ));
        }
        if version != "HTTP/1.1" && version != "HTTP/1.0" {
            return Err(ParseError::bad("versi HTTP tidak dikenal"));
        }

        let method = match method_str {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "HEAD" => HttpMethod::Head,
            "OPTIONS" => HttpMethod::Options,
            "PUT" => HttpMethod::Put,
            "DELETE" => HttpMethod::Delete,
            _ => HttpMethod::Unknown,
        };

        let (path, query) = if let Some(q_idx) = full_path.find('?') {
            (&full_path[..q_idx], Some(&full_path[q_idx + 1..]))
        } else {
            (full_path, None)
        };
        if !path.starts_with('/') {
            return Err(ParseError::bad("path bukan origin-form"));
        }

        let fields = scan_headers(header_block)?;
        let content_length = fields.content_length.unwrap_or(0);

        // Body yang benar-benar tiba harus sepanjang yang dideklarasikan. Tanpa
        // gerbang ini, permintaan terpotong di-dispatch dengan body kosong dan
        // sisa byte milik permintaan berikutnya ikut ditafsirkan.
        let body = &raw[body_start..];
        if body.len() != content_length {
            return Err(ParseError::bad(
                "panjang body tidak sama dengan Content-Length",
            ));
        }

        Ok(Self {
            method,
            path,
            query,
            host: fields.host,
            if_none_match: fields.if_none_match,
            content_type: fields.content_type,
            authorization: fields.authorization,
            content_length,
            body,
        })
    }
}

pub struct HttpResponseBuilder<'a> {
    buf: &'a mut [u8],
    offset: usize,
    status: HttpStatus,
    content_type: &'static str,
    etag: Option<&'static str>,
    challenge: Option<&'static str>,
}

impl<'a> HttpResponseBuilder<'a> {
    pub fn new(buf: &'a mut [u8], status: HttpStatus) -> Self {
        Self {
            buf,
            offset: 0,
            status,
            content_type: "text/plain; charset=utf-8",
            etag: None,
            challenge: None,
        }
    }

    pub fn with_content_type(mut self, ct: &'static str) -> Self {
        self.content_type = ct;
        self
    }

    pub fn with_etag(mut self, etag: &'static str) -> Self {
        self.etag = Some(etag);
        self
    }

    /// Tantangan skema autentikasi untuk respons 401 (RFC 6750 3). Hanya baris
    /// statis yang boleh dipakai, supaya jalur error tidak pernah menulis data
    /// pengguna ke header.
    pub fn with_www_authenticate(mut self, challenge: &'static str) -> Self {
        self.challenge = Some(challenge);
        self
    }

    fn write_bytes(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if self.offset + data.len() > self.buf.len() {
            return Err("Buffer capacity exceeded");
        }
        self.buf[self.offset..self.offset + data.len()].copy_from_slice(data);
        self.offset += data.len();
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> Result<(), &'static str> {
        self.write_bytes(s.as_bytes())
    }

    fn write_usize(&mut self, mut num: usize) -> Result<(), &'static str> {
        if num == 0 {
            return self.write_bytes(b"0");
        }
        let mut tmp = [0u8; 20];
        let mut idx = 20;
        while num > 0 {
            idx -= 1;
            tmp[idx] = b'0' + (num % 10) as u8;
            num /= 10;
        }
        self.write_bytes(&tmp[idx..])
    }

    /// Menulis `ETag: "<label>-<panjang isi>"`. Bagian kedua membuat validator
    /// berubah otomatis setiap kali isi aset disunting, tanpa perlu menaikkan
    /// label versi secara manual.
    fn write_etag(&mut self, etag: &'static str, content_len: usize) -> Result<(), &'static str> {
        self.write_str("ETag: \"")?;
        self.write_str(etag)?;
        self.write_bytes(b"-")?;
        self.write_usize(content_len)?;
        self.write_bytes(b"\"\r\n")
    }

    pub fn finish_with_body(mut self, body: &[u8]) -> Result<usize, &'static str> {
        // Status line
        self.write_str("HTTP/1.1 ")?;
        self.write_usize(self.status.code() as usize)?;
        self.write_str(" ")?;
        self.write_str(self.status.reason_phrase())?;
        self.write_str("\r\n")?;

        // Mandatory security headers (DECISION_REGISTER & Carrier Compliance)
        self.write_str("Server: OpenCR-Rinjani/1.0.0\r\n")?;
        self.write_str("X-Frame-Options: DENY\r\n")?;
        self.write_str("X-Content-Type-Options: nosniff\r\n")?;
        if let Some(c) = self.challenge {
            self.write_str("WWW-Authenticate: ")?;
            self.write_str(c)?;
            self.write_str("\r\n")?;
        }
        self.write_str("Connection: close\r\n")?;

        // Content-Type
        self.write_str("Content-Type: ")?;
        self.write_str(self.content_type)?;
        self.write_str("\r\n")?;

        // Validator: label sumber + panjang isi. Aset WebUI ter-embed di image
        // kernel sehingga URL tidak pernah berubah; tanpa bagian panjang ini,
        // suntingan isi aset tidak membatalkan cache browser (304 vs markup lama).
        if let Some(et) = self.etag {
            self.write_etag(et, body.len())?;
            // no-cache (bukan max-age): browser wajib revalidasi setiap load,
            // dan kita jawab 304 hanya bila validator masih cocok persis.
            self.write_str("Cache-Control: no-cache, must-revalidate\r\n")?;
        } else {
            self.write_str("Cache-Control: no-cache, no-store, must-revalidate\r\n")?;
        }

        // Content-Length
        self.write_str("Content-Length: ")?;
        self.write_usize(body.len())?;
        self.write_str("\r\n\r\n")?;

        // Payload
        self.write_bytes(body)?;
        Ok(self.offset)
    }

    pub fn finish_not_modified(
        mut self,
        etag: &'static str,
        content_len: usize,
    ) -> Result<usize, &'static str> {
        self.status = HttpStatus::NotModified;
        self.write_str("HTTP/1.1 304 Not Modified\r\n")?;
        self.write_str("Server: OpenCR-Rinjani/1.0.0\r\n")?;
        self.write_etag(etag, content_len)?;
        self.write_str("Connection: close\r\n")?;
        self.write_str("Content-Length: 0\r\n\r\n")?;
        Ok(self.offset)
    }
}

/// Kode status yang benar-benar tercetak pada baris status sebuah respons, atau
/// `0` bila buffer bukan respons HTTP.
///
/// Pemanggil wajib memakai fungsi ini alih-alih menulis angka yang direncanakan.
/// `dispatch`/`dispatch_static` mengembalikan `Ok(n)` untuk 200, 304, 403 dan 429
/// sekaligus, sehingga angka asumsi pemanggil membuat log konsol melaporkan
/// `-> 200` untuk respons yang sebenarnya tidak berubah atau ditolak.
#[must_use]
pub fn response_status(buf: &[u8]) -> u16 {
    const PFX: &[u8] = b"HTTP/1.1 ";
    if buf.len() < PFX.len() + 3 || &buf[..PFX.len()] != PFX {
        return 0;
    }
    let mut code = 0u16;
    for i in 0..3 {
        let b = buf[PFX.len() + i];
        if !b.is_ascii_digit() {
            return 0;
        }
        code = code * 10 + (b - b'0') as u16;
    }
    code
}

pub struct StaticAsset {
    pub path: &'static str,
    pub content_type: &'static str,
    pub etag: &'static str,
    pub content: &'static [u8],
}

/// Penyangga validator terkecil yang muat untuk label ETag proyek + 20 digit.
const VALIDATOR_BUF: usize = 96;

/// Susun validator `<etag>-<panjang isi>` di stack, tanpa heap. `None` bila
/// label terlalu panjang: penganggil wajib melayani 200 penuh, bukan 304,
/// supaya kegagalan penyusunan tidak pernah menyembunyikan konten baru.
fn build_validator<'a>(
    etag: &str,
    content_len: usize,
    buf: &'a mut [u8; VALIDATOR_BUF],
) -> Option<&'a str> {
    let label = etag.as_bytes();
    let mut tmp = [0u8; 20];
    let mut t = tmp.len();
    let mut n = content_len;
    if n == 0 {
        t -= 1;
        tmp[t] = b'0';
    }
    while n > 0 {
        t -= 1;
        tmp[t] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let digits = &tmp[t..];
    if label.len() + 1 + digits.len() > buf.len() {
        return None;
    }
    buf[..label.len()].copy_from_slice(label);
    buf[label.len()] = b'-';
    buf[label.len() + 1..label.len() + 1 + digits.len()].copy_from_slice(digits);
    core::str::from_utf8(&buf[..label.len() + 1 + digits.len()]).ok()
}

/// If-None-Match boleh berupa daftar koma (RFC 7232 §3.2); cocokkan tiap butir.
fn validator_list_matches(inm: &str, validator: &str) -> bool {
    inm.split(',')
        .any(|cand| cand.trim().trim_matches('"') == validator)
}

pub fn serve_static(
    asset: &StaticAsset,
    req: &HttpRequest,
    out_buf: &mut [u8],
) -> Result<usize, &'static str> {
    let mut vbuf = [0u8; VALIDATOR_BUF];
    let validator = build_validator(asset.etag, asset.content.len(), &mut vbuf);

    if let (Some(v), Some(inm)) = (validator, req.if_none_match) {
        if validator_list_matches(inm, v) {
            let builder = HttpResponseBuilder::new(out_buf, HttpStatus::NotModified);
            return builder.finish_not_modified(asset.etag, asset.content.len());
        }
    }

    let builder = HttpResponseBuilder::new(out_buf, HttpStatus::Ok)
        .with_content_type(asset.content_type)
        .with_etag(asset.etag);
    builder.finish_with_body(asset.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_parse_valid_get_request() {
        let raw = b"GET /status?filter=all HTTP/1.1\r\nHost: 192.168.56.1\r\nIf-None-Match: \"12345\"\r\n\r\n";
        let req = HttpRequest::parse(raw).expect("Must parse");
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.path, "/status");
        assert_eq!(req.query, Some("filter=all"));
        assert_eq!(req.host, Some("192.168.56.1"));
        assert_eq!(req.if_none_match, Some("\"12345\""));
    }

    #[test]
    fn test_02_parse_post_with_body() {
        let raw = b"POST /api/v1/firewall HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"action\":1}";
        let req = HttpRequest::parse(raw).expect("Must parse");
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.path, "/api/v1/firewall");
        assert_eq!(req.content_length, 12);
        assert_eq!(req.body, b"{\"action\":1}");
    }

    #[test]
    fn test_03_response_builder_security_headers() {
        let mut buf = [0u8; 512];
        let builder = HttpResponseBuilder::new(&mut buf, HttpStatus::Ok);
        let len = builder.finish_with_body(b"Hello Rinjani").unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK\r\n"));
        assert!(resp.contains("X-Frame-Options: DENY\r\n"));
        assert!(resp.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(resp.contains("Content-Length: 13\r\n"));
        assert!(resp.ends_with("\r\n\r\nHello Rinjani"));
    }

    #[test]
    fn test_03b_www_authenticate_hanya_terpilih() {
        // Respons 401 wajib membawa tantangan skema (RFC 6750 3), tetapi header
        // itu tidak boleh muncul pada respons lain.
        let mut buf = [0u8; 512];
        let len = HttpResponseBuilder::new(&mut buf, HttpStatus::Unauthorized)
            .with_content_type("application/json")
            .with_www_authenticate("Bearer realm=\"rinjani\"")
            .finish_with_body(b"{\"ok\":false}")
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("WWW-Authenticate: Bearer realm=\"rinjani\"\r\n"));

        let mut plain = [0u8; 512];
        let n2 = HttpResponseBuilder::new(&mut plain, HttpStatus::Ok)
            .finish_with_body(b"x")
            .unwrap();
        assert!(!core::str::from_utf8(&plain[..n2])
            .unwrap()
            .contains("WWW-Authenticate"));
    }

    #[test]
    fn test_04_etag_and_304_not_modified() {
        let asset = StaticAsset {
            path: "/shell.html",
            content_type: "text/html",
            etag: "sha-abc",
            content: b"<html>Rinjani</html>",
        };

        // Request with matching validator (label + panjang isi = 20)
        let req_raw = b"GET /shell.html HTTP/1.1\r\nIf-None-Match: \"sha-abc-20\"\r\n\r\n";
        let req = HttpRequest::parse(req_raw).unwrap();
        let mut out = [0u8; 512];
        let len = serve_static(&asset, &req, &mut out).unwrap();
        let resp = core::str::from_utf8(&out[..len]).unwrap();

        assert!(resp.contains("304 Not Modified"));
        assert!(resp.contains("ETag: \"sha-abc-20\""));
        assert!(resp.contains("Content-Length: 0"));
    }

    #[test]
    fn test_04b_if_none_match_list_is_honoured() {
        let asset = StaticAsset {
            path: "/css/base.css",
            content_type: "text/css",
            etag: "css-base",
            content: b"abcdef",
        };
        let req_raw =
            b"GET /css/base.css HTTP/1.1\r\nIf-None-Match: \"lama-1\", \"css-base-6\"\r\n\r\n";
        let req = HttpRequest::parse(req_raw).unwrap();
        let mut out = [0u8; 512];
        let len = serve_static(&asset, &req, &mut out).unwrap();
        assert!(core::str::from_utf8(&out[..len])
            .unwrap()
            .contains("304 Not Modified"));
    }

    #[test]
    fn test_04c_validator_changes_when_content_changes() {
        // Guard regression: suntingan aset tidak boleh lagi dibalas 304 hanya
        // karena label versinya tidak dinaikkan.
        let small = StaticAsset {
            path: "/ui.js",
            content_type: "application/javascript",
            etag: "js-ui-v1",
            content: b"old",
        };
        let grown = StaticAsset {
            content: b"old and much longer body",
            ..small
        };

        let req_raw = b"GET /ui.js HTTP/1.1\r\nIf-None-Match: \"js-ui-v1-3\"\r\n\r\n";
        let req = HttpRequest::parse(req_raw).unwrap();
        let mut out = [0u8; 512];

        let len = serve_static(&small, &req, &mut out).unwrap();
        assert!(core::str::from_utf8(&out[..len])
            .unwrap()
            .contains("304 Not Modified"));

        let len = serve_static(&grown, &req, &mut out).unwrap();
        let resp = core::str::from_utf8(&out[..len]).unwrap();
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("ETag: \"js-ui-v1-24\""));
    }

    #[test]
    fn test_05_etag_mismatch_returns_200() {
        let asset = StaticAsset {
            path: "/shell.html",
            content_type: "text/html",
            etag: "sha-abc",
            content: b"<html>Rinjani</html>",
        };

        let req_raw = b"GET /shell.html HTTP/1.1\r\nIf-None-Match: \"diff-tag\"\r\n\r\n";
        let req = HttpRequest::parse(req_raw).unwrap();
        let mut out = [0u8; 512];
        let len = serve_static(&asset, &req, &mut out).unwrap();
        let resp = core::str::from_utf8(&out[..len]).unwrap();

        assert!(resp.contains("200 OK"));
        assert!(resp.contains("ETag: \"sha-abc-20\""));
        assert!(resp.ends_with("<html>Rinjani</html>"));
    }

    #[test]
    fn test_06_error_statuses() {
        let mut buf = [0u8; 512];
        let builder = HttpResponseBuilder::new(&mut buf, HttpStatus::TooManyRequests);
        let len = builder
            .finish_with_body(b"{\"error\":\"rate_limited\"}")
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 429 Too Many Requests\r\n"));
    }

    #[test]
    fn test_07_body_length_must_match_declaration() {
        // DEFER-02: panjang yang dideklarasikan wajib sama dengan byte yang
        // benar-benar tiba. Tubuh di bawah/atas deklarasi ditolak, bukan
        // dipotong diam-diam atau di-dispatch dengan body kosong.
        let short = b"POST /api/v1/power HTTP/1.1\r\nContent-Length: 11\r\n\r\naction=arm";
        assert_eq!(
            HttpRequest::parse(short)
                .err()
                .map(|e| (e.status, e.reason)),
            Some((
                HttpStatus::BadRequest,
                "panjang body tidak sama dengan Content-Length"
            ))
        );
        let long = b"POST /api/v1/power HTTP/1.1\r\nContent-Length: 4\r\n\r\naction=arm";
        assert_eq!(HttpRequest::parse(long).err().unwrap().status.code(), 400);
        let exact = b"POST /api/v1/power HTTP/1.1\r\nContent-Length: 10\r\n\r\naction=arm";
        let req = HttpRequest::parse(exact).expect("body sepanjang deklarasi harus diterima");
        assert_eq!(req.content_length, 10);
        assert_eq!(req.body, b"action=arm");
    }

    /// Ambil error penolakan. `HttpRequest` sengaja tidak derives `Debug` supaya
    /// isi permintaan tidak pernah tercetak di log.
    fn err_of(raw: &[u8]) -> ParseError {
        match HttpRequest::parse(raw) {
            Ok(_) => panic!("permintaan seharusnya ditolak"),
            Err(e) => e,
        }
    }

    #[test]
    fn test_07b_content_length_value_is_strict() {
        // Digit terselip, tanda minus, nilai kosong, dan overflow harus ditolak.
        // Parser lama menghapus byte non-digit sehingga "1a2" terbaca 12 dan
        // "-5" terbaca 5.
        for raw in [
            b"POST /a HTTP/1.1\r\nContent-Length: 1a2\r\n\r\nab".as_slice(),
            b"POST /a HTTP/1.1\r\nContent-Length: -5\r\n\r\n".as_slice(),
            b"POST /a HTTP/1.1\r\nContent-Length: \r\n\r\n".as_slice(),
            b"POST /a HTTP/1.1\r\nContent-Length: 99999999999999999999\r\n\r\n".as_slice(),
            b"POST /a HTTP/1.1\r\nContent-Length: 12 34\r\n\r\n12345678901234".as_slice(),
        ] {
            let err = err_of(raw);
            assert_eq!(err.status, HttpStatus::BadRequest, "raw: {:?}", raw);
        }
    }

    #[test]
    fn test_07c_body_cap_is_enforced_before_dispatch() {
        // DEFER-03: body di atas batas ditolak dengan 413, tepat pada batas diterima.
        let mut over = [0u8; 1300];
        let n = build_post(&mut over, MAX_BODY_BYTES + 1, 0);
        let err = err_of(&over[..n]);
        assert_eq!(err.status, HttpStatus::PayloadTooLarge);

        let mut at = [0u8; 1300];
        let n = build_post(&mut at, MAX_BODY_BYTES, MAX_BODY_BYTES);
        let req = HttpRequest::parse(&at[..n]).expect("tepat pada batas harus diterima");
        assert_eq!(req.content_length, MAX_BODY_BYTES);
    }

    #[test]
    fn test_07d_duplicate_and_ambiguous_framing_are_rejected() {
        // Content-Length ganda tidak identik = permintaan ambigu (RFC 7230 3.3.3).
        let dup = b"POST /a HTTP/1.1\r\nContent-Length: 4\r\nContent-Length: 10\r\n\r\naction=arm";
        assert_eq!(
            HttpRequest::parse(dup).err().unwrap().status,
            HttpStatus::BadRequest
        );
        // Identik boleh, tetap satu interpretasi.
        let same =
            b"POST /a HTTP/1.1\r\nContent-Length: 10\r\nContent-Length: 10\r\n\r\naction=arm";
        assert!(HttpRequest::parse(same).is_ok());
        // Transfer-Encoding tidak didukung server: 501, bukan diam-diam dibaca
        // sebagai body berpanjangan (pemisahan interpretasi = smuggling).
        let chunked =
            b"POST /a HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 3\r\n\r\nabc";
        let err = err_of(chunked);
        assert_eq!(err.status, HttpStatus::NotImplemented);
        let combo =
            b"POST /a HTTP/1.1\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\nabc";
        assert_eq!(
            HttpRequest::parse(combo).err().unwrap().status,
            HttpStatus::NotImplemented
        );
    }

    #[test]
    fn test_07e_header_shape_rules() {
        // obs-fold (kelanjutan baris) dan baris tanpa kolon tidak boleh
        // menghasilkan header hantu.
        let fold = b"GET /a HTTP/1.1\r\nHost: x\r\n  folded\r\n\r\n";
        assert_eq!(HttpRequest::parse(fold).err().unwrap().status.code(), 400);
        let nocolon = b"GET /a HTTP/1.1\r\nGarbageLine\r\n\r\n";
        assert_eq!(
            HttpRequest::parse(nocolon).err().unwrap().status.code(),
            400
        );
        // LF telanjang di blok header: penutup baris tidak sah untuk HTTP/1.1.
        let bare = b"GET /a HTTP/1.1\nHost: x\r\n\r\n";
        assert_eq!(HttpRequest::parse(bare).err().unwrap().status.code(), 400);
        // LF telanjang di dalam body sah: body form-encoded tidak memuat LF,
        // tetapi aturan framing tidak boleh melarang byte body.
        let body_lf = b"POST /a HTTP/1.1\r\nContent-Length: 3\r\n\r\na\nb";
        assert!(HttpRequest::parse(body_lf).is_ok());
        // Header belum tertutup bukan permintaan sah untuk di-dispatch.
        let partial = b"GET /a HTTP/1.1\r\nHost: x\r\n";
        assert_eq!(
            HttpRequest::parse(partial).err().unwrap().reason,
            "header belum tertutup"
        );
        // request-line dengan bagian keempat dan versi asing.
        let extra = b"GET /a HTTP/1.1 junk\r\n\r\n";
        assert_eq!(HttpRequest::parse(extra).err().unwrap().status.code(), 400);
        let badver = b"GET /a HTTP/2.0\r\n\r\n";
        assert_eq!(HttpRequest::parse(badver).err().unwrap().status.code(), 400);
        // origin-form wajib: bentuk absolut/authority tidak dilayani.
        let absolut = b"GET http://example.com/a HTTP/1.1\r\n\r\n";
        assert_eq!(
            HttpRequest::parse(absolut).err().unwrap().status.code(),
            400
        );
    }

    #[test]
    fn test_07f_declared_body_len_agrees_with_parser() {
        // Gerbang kelengkapan byte dan pengurai wajib membaca framing yang sama.
        let head = b"POST /a HTTP/1.1\r\nContent-Length: 9\r\n\r\n";
        assert_eq!(declared_body_len(head), Ok(Some(9)));
        let none = b"GET /a HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(declared_body_len(none), Ok(None));
        let te = b"POST /a HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            declared_body_len(te).err().unwrap().status,
            HttpStatus::NotImplemented
        );
        let big = b"POST /a HTTP/1.1\r\nContent-Length: 99999\r\n\r\n";
        assert_eq!(
            declared_body_len(big).err().unwrap().status,
            HttpStatus::PayloadTooLarge
        );
    }

    #[test]
    fn test_07g_authorization_header_is_captured() {
        let raw = b"GET /api/v1/system HTTP/1.1\r\nAuthorization: Bearer abc123\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.authorization, Some("Bearer abc123"));
        let none = b"GET /api/v1/system HTTP/1.1\r\n\r\n";
        assert_eq!(HttpRequest::parse(none).unwrap().authorization, None);
    }

    /// Susun `POST /a` dengan Content-Length deklarasi `cl` dan body `body_len`
    /// byte, di buffer stack (crate ini `no_std`, jadi tidak ada `Vec`).
    /// Mengembalikan jumlah byte yang tertulis. Deklarasi dan body sengaja boleh
    /// berbeda untuk menguji gerbang pencocokan.
    fn build_post(buf: &mut [u8], cl: usize, body_len: usize) -> usize {
        let head = b"POST /a HTTP/1.1\r\nContent-Length: ";
        buf[..head.len()].copy_from_slice(head);
        let mut off = head.len();
        let mut digits = [0u8; 10];
        let mut d = digits.len();
        let mut v = cl;
        if v == 0 {
            d -= 1;
            digits[d] = b'0';
        }
        while v > 0 {
            d -= 1;
            digits[d] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        buf[off..off + digits[d..].len()].copy_from_slice(&digits[d..]);
        off += digits[d..].len();
        buf[off..off + 4].copy_from_slice(b"\r\n\r\n");
        off += 4;
        for b in buf[off..off + body_len].iter_mut() {
            *b = b'x';
        }
        off + body_len
    }

    #[test]
    fn test_09_response_status_reads_the_emitted_line() {
        // Guard kejujuran log: angka yang dilaporkan server harus dibaca dari
        // baytes yang benar-benar dikirim, bukan dari niat pemanggil.
        let mut buf = [0u8; 256];
        let cases = [
            HttpStatus::Ok,
            HttpStatus::NotModified,
            HttpStatus::BadRequest,
            HttpStatus::Unauthorized,
            HttpStatus::Forbidden,
            HttpStatus::NotFound,
            HttpStatus::MethodNotAllowed,
            HttpStatus::PayloadTooLarge,
            HttpStatus::TooManyRequests,
            HttpStatus::NotImplemented,
            HttpStatus::InternalServerError,
        ];
        for status in cases {
            let len = HttpResponseBuilder::new(&mut buf, status)
                .with_content_type("text/plain")
                .finish_with_body(b"x")
                .unwrap();
            assert_eq!(response_status(&buf[..len]), status.code());
        }

        let len = HttpResponseBuilder::new(&mut buf, HttpStatus::Ok)
            .finish_not_modified("css-base", 5923)
            .unwrap();
        assert_eq!(response_status(&buf[..len]), 304);
    }

    #[test]
    fn test_09b_response_status_rejects_non_responses() {
        assert_eq!(response_status(b""), 0);
        assert_eq!(response_status(b"GET / HTTP/1.1\r\n"), 0);
        assert_eq!(response_status(b"HTTP/1.1 20"), 0);
        assert_eq!(response_status(b"HTTP/1.1 2xx OK\r\n"), 0);
    }
}
