//! Encrypted Client Hello (ECH) for the launcher's own sites, as a backup.
//!
//! A TLS connection starts by naming the site it wants (the SNI), in the clear. Some networks read
//! that name and cut the connection, so the launcher can't get its list where a browser on the same
//! PC opens the website fine: browsers use ECH, which encrypts the real name with a key the site
//! publishes in DNS and shows only a shared public name (cloudflare-ech.com for these sites).
//!
//! Settings → Networking, Hide the Site's Name:
//! - Automatic (the default): a request goes out the usual way. Only when that connection is cut
//!   the way a block by name cuts it is the site's key looked up and the request sent again with
//!   ECH. When that works, the site is asked with ECH straight away for the rest of the run on that
//!   network. Nothing waits on the lookup unless the usual connection already failed.
//! - Always: ECH first, the usual way when there's no key or ECH doesn't connect.
//! - Never: the usual way only, and the key is never looked up.
//!
//! Only `SITES` are asked this way: an ECH key belongs to one site, so GitHub, downloads from other
//! servers and everything else stay on the usual client. The key is read from the site's HTTPS DNS
//! record over DNS-over-HTTPS (Cloudflare's resolver, then Google's), since a network that filters
//! by name usually answers plain DNS for it wrongly too.

use base64::Engine;
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The sites asked over ECH, each with its subdomains. Both are behind Cloudflare, which publishes
/// a key for each.
const SITES: [&str; 2] = ["recomplauncher.com", "recompstore.com"];

/// Where the key is looked up: public resolvers that answer in JSON, tried in this order.
const RESOLVERS: [&str; 2] = ["https://cloudflare-dns.com/dns-query", "https://dns.google/resolve"];

/// The DNS record type that carries the key (HTTPS, RFC 9460).
const HTTPS_RECORD: u64 = 65;

/// How long one resolver gets to answer.
const LOOKUP: Duration = Duration::from_secs(4);
/// How long a key is used before it's looked up again (the site changes it now and then).
const KEEP: Duration = Duration::from_secs(600);
/// How long to go without ECH after no key was found, or a connection with one failed.
const RETRY: Duration = Duration::from_secs(300);

/// What Settings → Networking says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Automatic,
    Always,
    Never,
}

static MODE: AtomicU8 = AtomicU8::new(0);

/// Takes the setting as it's saved: "always", "never", or anything else for Automatic.
pub fn set_mode(saved: &str) {
    let mode = match saved {
        "always" => 1,
        "never" => 2,
        _ => 0,
    };
    MODE.store(mode, Ordering::Relaxed);
}

fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        1 => Mode::Always,
        2 => Mode::Never,
        _ => Mode::Automatic,
    }
}

/// What the site's HTTPS record says: its ECH keys, and the addresses it hints at.
#[derive(Debug, PartialEq)]
struct Record {
    ech: Vec<u8>,
    addrs: Vec<SocketAddr>,
}

/// The client for one site, or None while it's asked without ECH.
struct Slot {
    client: Option<reqwest::Client>,
    until: Instant,
}

/// One per site. Held while a key is looked up, so requests that fail together wait for one
/// lookup instead of each making their own.
static SLOTS: tokio::sync::Mutex<Option<HashMap<String, Slot>>> = tokio::sync::Mutex::const_new(None);

/// Sites that needed ECH, each with the network it was needed on (see `network`).
static NEEDED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Is this a request to one of `SITES`? Only https: there's no name to hide without TLS.
fn covered(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let ours = SITES.iter().any(|site| host == *site || host.ends_with(&format!(".{site}")));
    ours && url.scheme() == "https"
}

/// Which network this PC is on, as far as it's cheap to tell: the address it would send from.
/// Connecting a UDP socket sends nothing; it only picks the route. Empty when there's none.
fn network() -> String {
    let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") else { return String::new() };
    if socket.connect("1.1.1.1:443").is_err() {
        return String::new();
    }
    socket.local_addr().map(|a| a.ip().to_string()).unwrap_or_default()
}

/// Did `host` need ECH earlier in this run, on the network the PC is on now?
fn needed(host: &str) -> bool {
    let sites = NEEDED.lock().unwrap();
    let Some(on) = sites.as_ref().and_then(|s| s.get(host)) else { return false };
    *on == network()
}

fn mark_needed(host: &str, yes: bool) {
    let mut sites = NEEDED.lock().unwrap();
    let sites = sites.get_or_insert_with(HashMap::new);
    if yes {
        sites.insert(host.to_string(), network());
    } else {
        sites.remove(host);
    }
}

/// The connection was cut the way a block by the site's name cuts it: while the secure handshake
/// was under way, with something that isn't TLS (net::tls_failed) or by closing it.
fn name_blocked(e: &reqwest::Error) -> bool {
    e.is_connect() && (crate::net::tls_failed(e) || crate::net::handshake_cut(e))
}

/// The keys as DNS shows them: base64, with or without its padding.
fn decode(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

/// Reads a resolver's JSON answer. The record's data looks like
/// `1 . alpn=h3,h2 ipv4hint=1.2.3.4 ech=AEX+... ipv6hint=2606::1`.
fn record(answer: &Value) -> Option<Record> {
    let rows = answer.get("Answer")?.as_array()?;
    let ours = rows.iter().filter(|row| row["type"] == HTTPS_RECORD);
    let data = ours.filter_map(|row| row["data"].as_str()).find(|data| data.contains("ech="))?;
    let mut ech = None;
    let mut addrs = Vec::new();
    for part in data.split_whitespace() {
        let Some((name, value)) = part.split_once('=') else { continue };
        let value = value.trim_matches('"');
        if name == "ech" {
            ech = decode(value);
        }
        if name == "ipv4hint" || name == "ipv6hint" {
            // Port 0: reqwest then uses the port the address being asked has.
            let hinted = value.split(',').filter_map(|ip| ip.parse::<IpAddr>().ok());
            addrs.extend(hinted.map(|ip| SocketAddr::new(ip, 0)));
        }
    }
    Some(Record { ech: ech?, addrs })
}

/// Asks the resolvers for the site's record. The first one that answers decides: a site without
/// a key isn't asked about twice.
async fn lookup(http: &reqwest::Client, host: &str, resolvers: &[&str]) -> Option<Record> {
    for resolver in resolvers {
        let question = http.get(*resolver).query(&[("name", host), ("type", "HTTPS")]);
        let sent = question.header(reqwest::header::ACCEPT, "application/dns-json").timeout(LOOKUP).send().await;
        let Ok(answer) = sent else { continue };
        let Ok(answer) = answer.json::<Value>().await else { continue };
        return record(&answer);
    }
    None
}

/// TLS settings that encrypt the real server name with the site's key.
fn tls_config(ech: Vec<u8>) -> Result<rustls::ClientConfig, rustls::Error> {
    // aws-lc-rs rather than ring (which the other clients use): rustls has the HPKE encryption
    // ECH needs only with it.
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let suites = rustls::crypto::aws_lc_rs::hpke::ALL_SUPPORTED_SUITES;
    let key = rustls::client::EchConfig::new(rustls::pki_types::EchConfigListBytes::from(ech), suites)?;
    // The same trusted roots as reqwest's own rustls clients.
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let builder = rustls::ClientConfig::builder_with_provider(provider).with_ech(rustls::client::EchMode::Enable(key))?;
    let mut config = builder.with_root_certificates(roots).with_no_client_auth();
    // reqwest doesn't set this on a config it's handed. HTTP/1.1 only, like the other clients
    // (reqwest is built without HTTP/2).
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// A client like `base`, but with ECH for `host`.
fn hidden_client(base: reqwest::ClientBuilder, host: &str, found: Record) -> Option<reqwest::Client> {
    let tls = tls_config(found.ech).ok()?;
    let mut builder = base.use_preconfigured_tls(tls);
    if !found.addrs.is_empty() {
        // The record's own addresses, so a wrong answer from the network's DNS doesn't matter.
        builder = builder.resolve_to_addrs(host, &found.addrs);
    }
    builder.build().ok()
}

/// What `send` needs besides the request: in the launcher, the setting, the public resolvers and
/// the launcher's client settings; in tests, local stand-ins.
struct Route<'a> {
    mode: Mode,
    resolvers: &'a [&'a str],
    base: fn() -> reqwest::ClientBuilder,
}

/// The ECH client for `host`, looking its key up when there's none yet or it's old. `plain` asks
/// the resolvers (through the player's proxy, like everything else).
async fn client(plain: &reqwest::Client, host: &str, route: &Route<'_>) -> Option<reqwest::Client> {
    let mut slots = SLOTS.lock().await;
    let slots = slots.get_or_insert_with(HashMap::new);
    if let Some(slot) = slots.get(host).filter(|slot| Instant::now() < slot.until) {
        return slot.client.clone();
    }
    let found = lookup(plain, host, route.resolvers).await;
    let client = found.and_then(|found| hidden_client((route.base)(), host, found));
    if client.is_none() {
        crate::trail::note(format!("note: no ECH key found for {host}"));
    }
    let keep = if client.is_some() { KEEP } else { RETRY };
    slots.insert(host.to_string(), Slot { client: client.clone(), until: Instant::now() + keep });
    client
}

/// A connection with the key failed (an old key, or a network that breaks ECH): go without for a
/// while, then look the key up again.
async fn forget(host: &str) {
    mark_needed(host, false);
    let mut slots = SLOTS.lock().await;
    let slot = Slot { client: None, until: Instant::now() + RETRY };
    slots.get_or_insert_with(HashMap::new).insert(host.to_string(), slot);
}

/// Sends `rb` for one of `SITES`, the way Settings → Networking says (see the top of this file),
/// and any other request the way it was built. What was set on the request itself (headers, body,
/// timeout) is kept; the ECH client has the same settings as the launcher's usual one.
pub async fn send(rb: reqwest::RequestBuilder) -> reqwest::Result<reqwest::Response> {
    let route = Route { mode: mode(), resolvers: &RESOLVERS, base: crate::net::client_builder };
    send_by(rb, &route).await
}

async fn send_by(rb: reqwest::RequestBuilder, route: &Route<'_>) -> reqwest::Result<reqwest::Response> {
    let (plain, request) = rb.build_split();
    let request = request?;
    if route.mode == Mode::Never || !covered(request.url()) {
        return plain.execute(request).await;
    }
    let host = request.url().host_str().unwrap_or_default().to_ascii_lowercase();
    if route.mode == Mode::Always || needed(&host) {
        return hidden_first(&plain, request, &host, route).await;
    }
    // Automatic: the usual way first. A copy to send again with ECH (None for a body that's a
    // stream, which can only be sent once).
    let spare = request.try_clone();
    let error = match plain.execute(request).await {
        Ok(answer) => return Ok(answer),
        Err(error) => error,
    };
    if !name_blocked(&error) {
        return Err(error);
    }
    let Some(spare) = spare else { return Err(error) };
    let Some(hidden) = client(&plain, &host, route).await else { return Err(error) };
    match hidden.execute(spare).await {
        Ok(answer) => {
            if !needed(&host) {
                crate::trail::note(format!("note: {host} was blocked by its name; it's asked with the name hidden (ECH) from now on"));
            }
            mark_needed(&host, true);
            Ok(answer)
        }
        Err(hidden_error) => {
            crate::trail::note(format!("note: {host} didn't connect with ECH either ({})", crate::net::cause(&hidden_error)));
            forget(&host).await;
            // The first error: what the window says about it stays the same as without ECH.
            Err(error)
        }
    }
}

/// ECH first (Always, or a site that needed it on this network), then the usual way when there's
/// no key or ECH doesn't connect.
async fn hidden_first(plain: &reqwest::Client, request: reqwest::Request, host: &str, route: &Route<'_>) -> reqwest::Result<reqwest::Response> {
    let Some(hidden) = client(plain, host, route).await else {
        return plain.execute(request).await;
    };
    let spare = request.try_clone();
    let error = match hidden.execute(request).await {
        Ok(answer) => return Ok(answer),
        Err(error) => error,
    };
    // Only a connection that was never made is tried again: the server hasn't seen the request.
    // That includes a redirect to another site, which this client's key isn't for.
    if !error.is_connect() {
        return Err(error);
    }
    crate::trail::note(format!("note: {host} didn't connect with ECH; asking without it for a while"));
    forget(host).await;
    match spare {
        Some(request) => plain.execute(request).await,
        None => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    /// A key list as recomplauncher.com published it (public name cloudflare-ech.com).
    const KEYS: &str = "AEX+DQBBkQAgACBoT5VhYTiaWYcaj5Fl2xh9sYET7gH350Ca603RZNj1cgAEAAEAAQASY2xvdWRmbGFyZS1lY2guY29tAAA=";

    fn answer(data: &str) -> Value {
        serde_json::json!({ "Status": 0, "Answer": [{ "name": "recomplauncher.com", "type": 65, "TTL": 300, "data": data }] })
    }

    #[test]
    fn only_the_launchers_own_sites_are_covered() {
        let covers = |url: &str| covered(&reqwest::Url::parse(url).unwrap());
        assert!(covers("https://recomplauncher.com/pc-catalog.json"));
        assert!(covers("https://api.recomplauncher.com/api/launcher.php?a=me"));
        assert!(covers("https://RecompLauncher.com/"));
        assert!(covers("https://recompstore.com/api/thumb.php"));
        assert!(covers("https://www.recompstore.com/media/x.jpg"));
        for other in ["https://github.com/a/b", "https://notrecomplauncher.com/", "https://recomplauncher.com.evil.example/", "https://dns.google/resolve"] {
            assert!(!covers(other), "{other}");
        }
    }

    #[test]
    fn plain_http_and_a_local_test_server_are_left_alone() {
        let covers = |url: &str| covered(&reqwest::Url::parse(url).unwrap());
        assert!(!covers("http://recomplauncher.com/x"));
        assert!(!covers("http://127.0.0.1:8080/pc-catalog.json"));
    }

    #[test]
    fn the_setting_reads_as_saved() {
        let read = |saved: &str| {
            set_mode(saved);
            mode()
        };
        assert_eq!(read("always"), Mode::Always);
        assert_eq!(read("never"), Mode::Never);
        assert_eq!(read(""), Mode::Automatic);
        assert_eq!(read("something else"), Mode::Automatic);
    }

    #[test]
    fn the_key_and_addresses_are_read_from_the_record() {
        let data = format!("1 . alpn=h3,h2 ipv4hint=104.21.50.219,172.67.211.10 ech={KEYS} ipv6hint=2606:4700:3036::6815:32db");
        let found = record(&answer(&data)).unwrap();
        assert_eq!(found.ech, decode(KEYS).unwrap());
        let addrs: Vec<String> = found.addrs.iter().map(|a| a.ip().to_string()).collect();
        assert_eq!(addrs, ["104.21.50.219", "172.67.211.10", "2606:4700:3036::6815:32db"]);
    }

    #[test]
    fn quoted_values_are_read_too() {
        let data = format!("1 . alpn=\"h3,h2\" ech=\"{KEYS}\"");
        let found = record(&answer(&data)).unwrap();
        assert_eq!(found.ech, decode(KEYS).unwrap());
        assert!(found.addrs.is_empty());
    }

    #[test]
    fn a_record_without_a_key_is_no_record() {
        assert_eq!(record(&answer("1 . alpn=h3,h2 ipv4hint=104.21.50.219")), None);
    }

    #[test]
    fn an_answer_with_no_records_is_no_record() {
        assert_eq!(record(&serde_json::json!({ "Status": 3 })), None);
    }

    #[test]
    fn another_record_type_is_skipped() {
        let cname = serde_json::json!({ "Answer": [{ "name": "recomplauncher.com", "type": 5, "data": format!("ech={KEYS}") }] });
        assert_eq!(record(&cname), None);
    }

    #[test]
    fn a_published_key_makes_tls_settings() {
        let config = tls_config(decode(KEYS).unwrap()).unwrap();
        assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
    }

    #[test]
    fn a_broken_key_is_refused() {
        assert!(tls_config(b"not a key list".to_vec()).is_err());
    }

    #[test]
    fn a_site_that_needed_ech_is_remembered_until_it_fails() {
        let host = "remember.recomplauncher.com";
        assert!(!needed(host));
        mark_needed(host, true);
        assert!(needed(host));
        mark_needed(host, false);
        assert!(!needed(host));
    }

    // ---------- the order things are tried in, against local stand-ins ----------

    /// A server that answers anything with a plain-text page, the way a filter that blocks a site
    /// by name answers its secure connection. Its port, and how many connections it has had.
    fn blocking_server() -> (u16, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(AtomicUsize::new(0));
        let count = seen.clone();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                let _ = s.read(&mut [0u8; 2048]);
                let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
            }
        });
        (port, seen)
    }

    /// A DNS-over-HTTPS stand-in whose answer has a key and points the site at 127.0.0.1. Its
    /// address, and how many questions it has had.
    fn resolver() -> (String, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let asked = Arc::new(AtomicUsize::new(0));
        let count = asked.clone();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                let _ = s.read(&mut [0u8; 2048]);
                let body = answer(&format!("1 . ech={KEYS} ipv4hint=127.0.0.1")).to_string();
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/dns-json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(format!("{head}{body}").as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}/dns-query"), asked)
    }

    fn no_proxy() -> reqwest::ClientBuilder {
        reqwest::Client::builder().no_proxy()
    }

    /// Sends a request for `host` (a covered name) to the server on `port`, with `mode`.
    fn try_send(host: &str, port: u16, mode: Mode, resolver: &str) -> reqwest::Result<reqwest::Response> {
        let to = SocketAddr::from(([127, 0, 0, 1], port));
        let plain = no_proxy().resolve(host, to).build().unwrap();
        let rb = plain.get(format!("https://{host}:{port}/pc-catalog.json"));
        let resolvers = [resolver];
        let route = Route { mode, resolvers: &resolvers, base: no_proxy };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(send_by(rb, &route))
    }

    #[test]
    fn automatic_connects_the_usual_way_first_then_with_ech_after_a_block_by_name() {
        let (port, connections) = blocking_server();
        let (doh, questions) = resolver();
        let e = try_send("automatic.recomplauncher.com", port, Mode::Automatic, &doh).unwrap_err();
        // The usual connection, then one with ECH after the key was looked up once.
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert_eq!(questions.load(Ordering::SeqCst), 1);
        // ECH failing too gives back the first error, as before.
        assert!(crate::net::tls_failed(&e), "{}", crate::net::cause(&e));
        assert!(!needed("automatic.recomplauncher.com"));
    }

    #[test]
    fn automatic_doesnt_look_the_key_up_when_the_site_just_doesnt_answer() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let (doh, questions) = resolver();
        let e = try_send("refused.recomplauncher.com", port, Mode::Automatic, &doh).unwrap_err();
        assert!(e.is_connect());
        assert_eq!(questions.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn never_makes_no_dns_over_https_request() {
        let (port, connections) = blocking_server();
        let (doh, questions) = resolver();
        try_send("never.recomplauncher.com", port, Mode::Never, &doh).unwrap_err();
        assert_eq!(connections.load(Ordering::SeqCst), 1);
        assert_eq!(questions.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn always_tries_ech_first_then_the_usual_way() {
        let (port, connections) = blocking_server();
        let (doh, questions) = resolver();
        try_send("always.recomplauncher.com", port, Mode::Always, &doh).unwrap_err();
        assert_eq!(questions.load(Ordering::SeqCst), 1);
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }
}
