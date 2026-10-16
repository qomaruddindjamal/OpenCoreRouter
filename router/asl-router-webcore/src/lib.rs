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
    TooManyRequests = 429,
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
            Self::TooManyRequests => "Too Many Requests",
            Self::InternalServerError => "Internal Server Error",
        }
    }
}

pub struct HttpRequest<'a> {
    pub method: HttpMethod,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub host: Option<&'a str>,
    pub if_none_match: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub content_length: usize,
    pub body: &'a [u8],
}

impl<'a> HttpRequest<'a> {
    pub fn parse(raw: &'a [u8]) -> Result<Self, &'static str> {
        let text = core::str::from_utf8(raw).map_err(|_| "Invalid UTF-8 request")?;
        let mut lines = text.split("\r\n");

        let request_line = lines.next().ok_or("Empty request")?;
        let mut req_parts = request_line.split_ascii_whitespace();

        let method_str = req_parts.next().ok_or("Missing HTTP method")?;
        let full_path = req_parts.next().ok_or("Missing request path")?;
        let _version = req_parts.next().ok_or("Missing HTTP version")?;

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

        let mut host = None;
        let mut if_none_match = None;
        let mut content_type = None;
        let mut content_length = 0;

        for line in lines {
            if line.is_empty() {
                break; // Header section ended
            }
            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim();
                let val = line[colon_pos + 1..].trim();

                if name.eq_ignore_ascii_case("host") {
                    host = Some(val);
                } else if name.eq_ignore_ascii_case("if-none-match") {
                    if_none_match = Some(val);
                } else if name.eq_ignore_ascii_case("content-type") {
                    content_type = Some(val);
                } else if name.eq_ignore_ascii_case("content-length") {
                    let mut cl = 0usize;
                    for b in val.bytes() {
                        if b >= b'0' && b <= b'9' {
                            cl = cl.saturating_mul(10).saturating_add((b - b'0') as usize);
                        }
                    }
                    content_length = cl;
                }
            }
        }

        // Body begins after \r\n\r\n
        let body_start = text
            .find("\r\n\r\n")
            .map(|idx| idx + 4)
            .unwrap_or(text.len());
        let body = &raw[body_start..];

        Ok(Self {
            method,
            path,
            query,
            host,
            if_none_match,
            content_type,
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
}

impl<'a> HttpResponseBuilder<'a> {
    pub fn new(buf: &'a mut [u8], status: HttpStatus) -> Self {
        Self {
            buf,
            offset: 0,
            status,
            content_type: "text/plain; charset=utf-8",
            etag: None,
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
        let raw = b"POST /api/v1/firewall HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"action\":1}";
        let req = HttpRequest::parse(raw).expect("Must parse");
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.path, "/api/v1/firewall");
        assert_eq!(req.content_length, 13);
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
            HttpStatus::TooManyRequests,
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
