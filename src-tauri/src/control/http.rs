//! A deliberately small HTTP/1.1 server side for the control API: one request per connection,
//! `Content-Length` bodies only, bounded sizes and times, and the checks that keep browsers and
//! other users out (loopback Host only, no Origin / Fetch-Metadata headers, a bearer token).

use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_BODY: usize = 1024 * 1024;
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Lowercased names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// An error the API answers with: an HTTP status and `{ok:false, error:{code, message, details}}`.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
    pub details: Value,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        ApiError { status, code, message: message.into(), details: Value::Null }
    }
    pub fn with(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
    pub fn body(&self) -> Value {
        let mut e = json!({"code": self.code, "message": self.message});
        if !self.details.is_null() {
            e["details"] = self.details.clone();
        }
        e
    }
    pub fn to_json(&self) -> Value {
        json!({"ok": false, "error": self.body()})
    }
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads the request line and headers (`head`, without the blank line).
pub fn parse_head(head: &[u8]) -> Result<(String, String, Vec<(String, String)>, Vec<(String, String)>), ApiError> {
    let text = std::str::from_utf8(head).map_err(|_| ApiError::new(400, "bad_request", "The request head isn't UTF-8."))?;
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let (method, target, version) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method.is_empty() || !target.starts_with('/') || !(version == "HTTP/1.1" || version == "HTTP/1.0") || parts.next().is_some() {
        return Err(ApiError::new(400, "bad_request", "Malformed request line."));
    }
    let mut headers = vec![];
    for l in lines {
        let Some((n, v)) = l.split_once(':') else {
            return Err(ApiError::new(400, "bad_request", "Malformed header line."));
        };
        let n = n.trim();
        if n.is_empty() || n.contains(' ') || n.contains('\t') {
            return Err(ApiError::new(400, "bad_request", "Malformed header name."));
        }
        headers.push((n.to_ascii_lowercase(), v.trim().to_string()));
        if headers.len() > 64 {
            return Err(ApiError::new(431, "headers_too_large", "Too many headers."));
        }
    }
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let query = q.split('&').filter(|s| !s.is_empty()).map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (pct_decode(k), pct_decode(v))
    }).collect();
    Ok((method.to_string(), pct_decode(path), query, headers))
}

/// Reads one request, within the size and time limits.
pub async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S, continue_ok: impl Fn(&Request) -> Result<(), ApiError>) -> Result<Request, ApiError> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let head_end = tokio::time::timeout(READ_TIMEOUT, async {
        loop {
            if let Some(i) = find_head_end(&buf) {
                return Ok(i);
            }
            if buf.len() > MAX_HEAD {
                return Err(ApiError::new(431, "headers_too_large", "The request head is too large."));
            }
            let n = s.read(&mut chunk).await.map_err(|_| ApiError::new(400, "bad_request", "Read failed."))?;
            if n == 0 {
                return Err(ApiError::new(400, "bad_request", "The connection closed before the request was complete."));
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    })
    .await
    .map_err(|_| ApiError::new(408, "timeout", "The request took too long to arrive."))??;
    if head_end > MAX_HEAD {
        return Err(ApiError::new(431, "headers_too_large", "The request head is too large."));
    }
    let (method, path, query, headers) = parse_head(&buf[..head_end])?;
    let mut req = Request { method, path, query, headers, body: vec![] };
    if req.header("transfer-encoding").is_some() {
        return Err(ApiError::new(501, "unsupported_transfer_encoding", "Send the body with Content-Length."));
    }
    let lens: Vec<&str> = req.headers.iter().filter(|(n, _)| n == "content-length").map(|(_, v)| v.as_str()).collect();
    if lens.len() > 1 {
        return Err(ApiError::new(400, "bad_request", "More than one Content-Length."));
    }
    let len = match lens.first() {
        Some(v) => v.parse::<usize>().map_err(|_| ApiError::new(400, "bad_request", "Content-Length isn't a number."))?,
        None if req.method == "POST" => return Err(ApiError::new(411, "length_required", "POST needs Content-Length.")),
        None => 0,
    };
    if len > MAX_BODY {
        return Err(ApiError::new(413, "payload_too_large", format!("The body is limited to {MAX_BODY} bytes.")));
    }
    // Checked before reading the body, so an unauthorized sender can't make the server read more.
    continue_ok(&req)?;
    if req.header("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) && len > 0 {
        let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
    }
    let mut body = buf[head_end + 4..].to_vec();
    if body.len() > len {
        return Err(ApiError::new(400, "bad_request", "More body than Content-Length says."));
    }
    tokio::time::timeout(READ_TIMEOUT, async {
        while body.len() < len {
            let n = s.read(&mut chunk).await.map_err(|_| ApiError::new(400, "bad_request", "Read failed."))?;
            if n == 0 {
                return Err(ApiError::new(400, "bad_request", "The body is shorter than Content-Length."));
            }
            body.extend_from_slice(&chunk[..n]);
            if body.len() > len {
                return Err(ApiError::new(400, "bad_request", "More body than Content-Length says."));
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| ApiError::new(408, "timeout", "The body took too long to arrive."))??;
    req.body = body;
    Ok(req)
}

/// Equal-time comparison, so the token can't be guessed a byte at a time.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The checks every request passes before anything else: it's addressed to this loopback port by
/// name (no DNS rebinding), it doesn't come from a web page (browsers always add Origin or
/// Sec-Fetch-* to the requests a page makes), and it carries the token.
pub fn guard(req: &Request, port: u16, token: &str) -> Result<(), ApiError> {
    let host = req.header("host").unwrap_or("");
    let ok_hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !ok_hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return Err(ApiError::new(403, "bad_host", "The Host header must be this loopback address and port."));
    }
    if req.header("origin").is_some() || req.headers.iter().any(|(n, _)| n.starts_with("sec-fetch-")) {
        return Err(ApiError::new(403, "browser_request_forbidden", "Requests from web pages aren't accepted."));
    }
    if req.method == "OPTIONS" {
        return Err(ApiError::new(405, "method_not_allowed", "No CORS here."));
    }
    let given = req.header("authorization").and_then(|a| a.strip_prefix("Bearer ").or_else(|| a.strip_prefix("bearer "))).map(str::trim).unwrap_or("");
    if given.is_empty() || !same(given.as_bytes(), token.as_bytes()) {
        return Err(ApiError::new(401, "unauthorized", "A valid bearer token is required (see the endpoint file)."));
    }
    if req.method == "POST" {
        let ct = req.header("content-type").unwrap_or("").to_ascii_lowercase();
        if !(ct == "application/json" || ct.starts_with("application/json;")) {
            return Err(ApiError::new(415, "unsupported_media_type", "POST bodies must be application/json."));
        }
    }
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}

pub fn response_bytes(status: u16, body: Option<&Value>) -> Vec<u8> {
    let b = body.map(|v| serde_json::to_vec(v).unwrap_or_default()).unwrap_or_default();
    let mut h = format!("HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n", reason(status), b.len());
    if body.is_some() {
        h.push_str("Content-Type: application/json\r\n");
    }
    if status == 401 {
        h.push_str("WWW-Authenticate: Bearer\r\n");
    }
    h.push_str("\r\n");
    let mut out = h.into_bytes();
    out.extend_from_slice(&b);
    out
}

pub async fn write_response<S: AsyncWrite + Unpin>(s: &mut S, status: u16, body: Option<&Value>) {
    let bytes = response_bytes(status, body);
    let _ = tokio::time::timeout(WRITE_TIMEOUT, async {
        let _ = s.write_all(&bytes).await;
        let _ = s.flush().await;
        let _ = s.shutdown().await;
    })
    .await;
}

/// After refusing a request whose body wasn't read: reads and drops what the client is still
/// sending (bounded in bytes and time), so closing doesn't reset the connection before the client
/// has read the refusal.
pub async fn drain<S: AsyncRead + Unpin>(s: &mut S) {
    let mut chunk = [0u8; 16 * 1024];
    let mut left = 2 * MAX_BODY;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while left > 0 {
            match s.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => left = left.saturating_sub(n),
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, headers: &[(&str, &str)]) -> Request {
        Request { method: method.into(), path: "/v1/status".into(), query: vec![], headers: headers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), body: vec![] }
    }

    #[test]
    fn heads_parse_and_bad_ones_are_refused() {
        let (m, p, q, h) = parse_head(b"GET /v1/events?after=5&names=install%2Csetup HTTP/1.1\r\nHost: 127.0.0.1:9\r\nX-A: b").unwrap();
        assert_eq!((m.as_str(), p.as_str()), ("GET", "/v1/events"));
        assert_eq!(q, vec![("after".to_string(), "5".to_string()), ("names".to_string(), "install,setup".to_string())]);
        assert_eq!(h[0], ("host".to_string(), "127.0.0.1:9".to_string()));
        assert!(parse_head(b"GET http://evil/ HTTP/1.1").is_err());
        assert!(parse_head(b"GET / HTTP/2\r\n").is_err());
        assert!(parse_head(b"GET / HTTP/1.1\r\nno colon").is_err());
        assert!(parse_head(b"GET / HTTP/1.1\r\nBad Name: x").is_err());
    }

    #[test]
    fn only_loopback_callers_with_the_token_get_in() {
        let t = "s3cret-token";
        let good = [("host", "127.0.0.1:4242"), ("authorization", "Bearer s3cret-token")];
        assert!(guard(&req("GET", &good), 4242, t).is_ok());
        assert!(guard(&req("GET", &[("host", "localhost:4242"), ("authorization", "Bearer s3cret-token")]), 4242, t).is_ok());
        // DNS rebinding: a page on evil.example resolving to 127.0.0.1 sends its own Host.
        assert_eq!(guard(&req("GET", &[("host", "evil.example:4242"), ("authorization", "Bearer s3cret-token")]), 4242, t).unwrap_err().code, "bad_host");
        assert_eq!(guard(&req("GET", &[("host", "127.0.0.1:4243"), ("authorization", "Bearer s3cret-token")]), 4242, t).unwrap_err().code, "bad_host");
        // Pages: Origin or Fetch Metadata, even with the token.
        let mut with_origin = good.to_vec();
        with_origin.push(("origin", "http://127.0.0.1:4242"));
        assert_eq!(guard(&req("POST", &with_origin), 4242, t).unwrap_err().code, "browser_request_forbidden");
        let mut with_fetch = good.to_vec();
        with_fetch.push(("sec-fetch-mode", "no-cors"));
        assert_eq!(guard(&req("GET", &with_fetch), 4242, t).unwrap_err().code, "browser_request_forbidden");
        assert_eq!(guard(&req("OPTIONS", &good), 4242, t).unwrap_err().status, 405);
        // Token.
        assert_eq!(guard(&req("GET", &[("host", "127.0.0.1:4242")]), 4242, t).unwrap_err().status, 401);
        assert_eq!(guard(&req("GET", &[("host", "127.0.0.1:4242"), ("authorization", "Bearer s3cret-tokeN")]), 4242, t).unwrap_err().status, 401);
        assert_eq!(guard(&req("GET", &[("host", "127.0.0.1:4242"), ("authorization", "Basic s3cret-token")]), 4242, t).unwrap_err().status, 401);
        // Simple (no-preflight) content types are refused for POST.
        assert_eq!(guard(&req("POST", &good), 4242, t).unwrap_err().code, "unsupported_media_type");
        let mut form = good.to_vec();
        form.push(("content-type", "text/plain"));
        assert_eq!(guard(&req("POST", &form), 4242, t).unwrap_err().status, 415);
        let mut json_ct = good.to_vec();
        json_ct.push(("content-type", "application/json; charset=utf-8"));
        assert!(guard(&req("POST", &json_ct), 4242, t).is_ok());
    }

    #[test]
    fn equal_time_compare() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
    }

    #[test]
    fn responses_never_carry_cors_headers() {
        let r = String::from_utf8(response_bytes(401, Some(&json!({"ok": false})))).unwrap();
        assert!(r.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
        assert!(r.contains("WWW-Authenticate: Bearer"));
        assert!(r.contains("Cache-Control: no-store"));
        assert!(!r.to_ascii_lowercase().contains("access-control"));
    }

    #[tokio::test]
    async fn sizes_are_bounded_and_the_guard_runs_before_the_body() {
        use tokio::io::duplex;
        // Too big a body: refused from the head alone.
        let (mut c, mut s) = duplex(64 * 1024);
        c.write_all(format!("POST /v1/invoke HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1).as_bytes()).await.unwrap();
        assert_eq!(read_request(&mut s, |_| Ok(())).await.unwrap_err().status, 413);
        // Chunked: refused.
        let (mut c, mut s) = duplex(64 * 1024);
        c.write_all(b"POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        assert_eq!(read_request(&mut s, |_| Ok(())).await.unwrap_err().status, 501);
        // A head that never ends: refused once it's too long.
        let (mut c, mut s) = duplex(64 * 1024);
        let long = format!("GET / HTTP/1.1\r\nX: {}", "a".repeat(MAX_HEAD + 10));
        c.write_all(long.as_bytes()).await.unwrap();
        assert_eq!(read_request(&mut s, |_| Ok(())).await.unwrap_err().status, 431);
        // The guard's answer comes before any of the body is read.
        let (mut c, mut s) = duplex(64 * 1024);
        c.write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 500000\r\n\r\n").await.unwrap();
        let e = read_request(&mut s, |_| Err(ApiError::new(401, "unauthorized", "no"))).await.unwrap_err();
        assert_eq!(e.status, 401);
        // A complete one.
        let (mut c, mut s) = duplex(64 * 1024);
        c.write_all(b"POST /mcp?tools=meta HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}").await.unwrap();
        let r = read_request(&mut s, |_| Ok(())).await.unwrap();
        assert_eq!((r.path.as_str(), r.param("tools"), r.body.as_slice()), ("/mcp", Some("meta"), &b"{}"[..]));
    }
}
