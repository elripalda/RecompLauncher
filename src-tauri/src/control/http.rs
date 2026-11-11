//! A deliberately small HTTP/1.1 server side: one request per connection, `Content-Length`
//! bodies only, bounded sizes and times, and the checks that keep web pages and other users out
//! (loopback Host only, no Origin or Fetch Metadata headers, a bearer token, JSON bodies only).

use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_HEAD: usize = 8 * 1024;
/// Play's body is one id: anything near this is not a real request.
pub const MAX_BODY: usize = 64 * 1024;
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Lowercased names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// An error the API answers with: an HTTP status and `{ok: false, error: {code, message}}`.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        ApiError { status, code, message: message.into() }
    }

    pub fn to_json(&self) -> Value {
        json!({"ok": false, "error": {"code": self.code, "message": self.message}})
    }
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// A request's line and headers.
#[derive(Debug)]
pub struct Head {
    pub method: String,
    pub path: String,
    /// Lowercased names.
    pub headers: Vec<(String, String)>,
}

/// The request line and headers (`head`, without the blank line). The query string is dropped:
/// no route takes one.
pub fn parse_head(head: &[u8]) -> Result<Head, ApiError> {
    let text = std::str::from_utf8(head).map_err(|_| ApiError::new(400, "bad_request", "The request head isn't UTF-8."))?;
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let (method, target, version) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let version_ok = version == "HTTP/1.1" || version == "HTTP/1.0";
    if method.is_empty() || !target.starts_with('/') || !version_ok || parts.next().is_some() {
        return Err(ApiError::new(400, "bad_request", "Malformed request line."));
    }
    let mut headers = vec![];
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(ApiError::new(400, "bad_request", "Malformed header line."));
        };
        let name = name.trim();
        if name.is_empty() || name.contains(' ') || name.contains('\t') {
            return Err(ApiError::new(400, "bad_request", "Malformed header name."));
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
        if headers.len() > 64 {
            return Err(ApiError::new(431, "headers_too_large", "Too many headers."));
        }
    }
    let path = target.split('?').next().unwrap_or(target);
    Ok(Head { method: method.to_string(), path: path.to_string(), headers })
}

/// Reads one request within the size and time limits. `allowed` runs on the head before any of
/// the body is read, so a sender without the token can't make the server read more.
pub async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S, allowed: impl Fn(&Request) -> Result<(), ApiError>) -> Result<Request, ApiError> {
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
    let head = parse_head(&buf[..head_end])?;
    let mut req = Request { method: head.method, path: head.path, headers: head.headers, body: vec![] };
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
    allowed(&req)?;
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
/// name (no DNS rebinding), it doesn't come from a web page (browsers add Origin or Sec-Fetch-*
/// to the requests a page makes), it carries the token, and a POST body is JSON (a page can't
/// send that cross-origin without a preflight, which is refused).
pub fn guard(req: &Request, port: u16, token: &str) -> Result<(), ApiError> {
    let host = req.header("host").unwrap_or("");
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return Err(ApiError::new(403, "bad_host", "The Host header must be this loopback address and port."));
    }
    let from_page = req.header("origin").is_some() || req.headers.iter().any(|(n, _)| n.starts_with("sec-fetch-"));
    if from_page {
        return Err(ApiError::new(403, "browser_request_forbidden", "Requests from web pages aren't accepted."));
    }
    if req.method == "OPTIONS" {
        return Err(ApiError::new(405, "method_not_allowed", "No CORS here."));
    }
    let auth = req.header("authorization").unwrap_or("");
    let given = auth.strip_prefix("Bearer ").or_else(|| auth.strip_prefix("bearer ")).map(str::trim).unwrap_or("");
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
        200 => "OK",
        202 => "Accepted",
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
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
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
        let headers = headers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        Request { method: method.into(), path: "/v1/status".into(), headers, body: vec![] }
    }

    const TOKEN: &str = "s3cret-token";
    const GOOD: [(&str, &str); 2] = [("host", "127.0.0.1:4242"), ("authorization", "Bearer s3cret-token")];

    #[test]
    fn heads_parse_and_bad_ones_are_refused() {
        let h = parse_head(b"GET /v1/status?x=1 HTTP/1.1\r\nHost: 127.0.0.1:9\r\nX-A: b").unwrap();
        assert_eq!((h.method.as_str(), h.path.as_str()), ("GET", "/v1/status"));
        assert_eq!(h.headers[0], ("host".to_string(), "127.0.0.1:9".to_string()));
        assert!(parse_head(b"GET http://evil/ HTTP/1.1").is_err());
        assert!(parse_head(b"GET / HTTP/2\r\n").is_err());
        assert!(parse_head(b"GET / HTTP/1.1\r\nno colon").is_err());
        assert!(parse_head(b"GET / HTTP/1.1\r\nBad Name: x").is_err());
    }

    #[test]
    fn the_token_and_loopback_host_let_a_request_in() {
        assert!(guard(&req("GET", &GOOD), 4242, TOKEN).is_ok());
        assert!(guard(&req("GET", &[("host", "localhost:4242"), ("authorization", "Bearer s3cret-token")]), 4242, TOKEN).is_ok());
    }

    #[test]
    fn another_host_name_is_refused() {
        // DNS rebinding: a page on evil.example resolving to 127.0.0.1 sends its own Host.
        let evil = req("GET", &[("host", "evil.example:4242"), ("authorization", "Bearer s3cret-token")]);
        assert_eq!(guard(&evil, 4242, TOKEN).unwrap_err().code, "bad_host");
        let other_port = req("GET", &[("host", "127.0.0.1:4243"), ("authorization", "Bearer s3cret-token")]);
        assert_eq!(guard(&other_port, 4242, TOKEN).unwrap_err().code, "bad_host");
    }

    #[test]
    fn web_pages_are_refused_even_with_the_token() {
        let mut with_origin = GOOD.to_vec();
        with_origin.push(("origin", "http://127.0.0.1:4242"));
        assert_eq!(guard(&req("POST", &with_origin), 4242, TOKEN).unwrap_err().code, "browser_request_forbidden");
        let mut with_fetch = GOOD.to_vec();
        with_fetch.push(("sec-fetch-mode", "no-cors"));
        assert_eq!(guard(&req("GET", &with_fetch), 4242, TOKEN).unwrap_err().code, "browser_request_forbidden");
        assert_eq!(guard(&req("OPTIONS", &GOOD), 4242, TOKEN).unwrap_err().status, 405);
    }

    #[test]
    fn a_missing_or_wrong_token_is_refused() {
        assert_eq!(guard(&req("GET", &[("host", "127.0.0.1:4242")]), 4242, TOKEN).unwrap_err().status, 401);
        let wrong = req("GET", &[("host", "127.0.0.1:4242"), ("authorization", "Bearer s3cret-tokeN")]);
        assert_eq!(guard(&wrong, 4242, TOKEN).unwrap_err().status, 401);
        let basic = req("GET", &[("host", "127.0.0.1:4242"), ("authorization", "Basic s3cret-token")]);
        assert_eq!(guard(&basic, 4242, TOKEN).unwrap_err().status, 401);
    }

    #[test]
    fn posts_must_be_json() {
        // Content types a page may send without a preflight are refused.
        assert_eq!(guard(&req("POST", &GOOD), 4242, TOKEN).unwrap_err().code, "unsupported_media_type");
        let mut form = GOOD.to_vec();
        form.push(("content-type", "text/plain"));
        assert_eq!(guard(&req("POST", &form), 4242, TOKEN).unwrap_err().status, 415);
        let mut json_ct = GOOD.to_vec();
        json_ct.push(("content-type", "application/json; charset=utf-8"));
        assert!(guard(&req("POST", &json_ct), 4242, TOKEN).is_ok());
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
    async fn a_body_over_the_limit_is_refused_from_the_head() {
        let (mut c, mut s) = tokio::io::duplex(64 * 1024);
        c.write_all(format!("POST /v1/play HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1).as_bytes()).await.unwrap();
        assert_eq!(read_request(&mut s, |_| Ok(())).await.unwrap_err().status, 413);
    }

    #[tokio::test]
    async fn chunked_bodies_are_refused() {
        let (mut c, mut s) = tokio::io::duplex(64 * 1024);
        c.write_all(b"POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        assert_eq!(read_request(&mut s, |_| Ok(())).await.unwrap_err().status, 501);
    }

    #[tokio::test]
    async fn a_head_that_never_ends_is_refused() {
        let (mut c, mut s) = tokio::io::duplex(64 * 1024);
        c.write_all(format!("GET / HTTP/1.1\r\nX: {}", "a".repeat(MAX_HEAD + 10)).as_bytes()).await.unwrap();
        assert_eq!(read_request(&mut s, |_| Ok(())).await.unwrap_err().status, 431);
    }

    #[tokio::test]
    async fn the_guard_answers_before_the_body_is_read() {
        let (mut c, mut s) = tokio::io::duplex(64 * 1024);
        c.write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 50000\r\n\r\n").await.unwrap();
        let e = read_request(&mut s, |_| Err(ApiError::new(401, "unauthorized", "no"))).await.unwrap_err();
        assert_eq!(e.status, 401);
    }

    #[tokio::test]
    async fn a_complete_request_is_read() {
        let (mut c, mut s) = tokio::io::duplex(64 * 1024);
        c.write_all(b"POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}").await.unwrap();
        let r = read_request(&mut s, |_| Ok(())).await.unwrap();
        assert_eq!((r.path.as_str(), r.body.as_slice()), ("/mcp", &b"{}"[..]));
    }
}
