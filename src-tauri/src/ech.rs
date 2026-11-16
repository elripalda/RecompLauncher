//! Encrypted Client Hello (ECH) for the launcher's own server.
//!
//! A TLS connection starts by naming the site it wants (the SNI), in the clear. Some networks read
//! that name and cut the connection, so the launcher can't get its list where a browser on the same
//! PC opens the website fine: browsers use ECH, which encrypts the real name with a key the site
//! publishes in DNS and shows only a shared public name (cloudflare-ech.com for this site).
//!
//! This does the same for requests to the launcher's server, and only those: an ECH key belongs
//! to one site, so GitHub, downloads and everything else stay on the usual client. The key is
//! read from the site's HTTPS DNS record over DNS-over-HTTPS, since a network that filters by name
//! usually answers plain DNS for it wrongly too.
//!
//! It never makes things worse than before: no key, or a connection that fails with one, and the
//! request goes out the way it always did.

use base64::Engine;
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The sites asked over ECH, each with its subdomains. Add a name here to cover another one.
const SITES: [&str; 1] = ["recomplauncher.com"];

/// Where the key is looked up: public resolvers that answer in JSON, tried in this order.
const RESOLVERS: [&str; 2] = ["https://cloudflare-dns.com/dns-query", "https://dns.google/resolve"];

/// The DNS record type that carries the key (HTTPS, RFC 9460).
const HTTPS_RECORD: u64 = 65;

/// How long one resolver gets to answer. Requests to the site wait for this the first time.
const LOOKUP: Duration = Duration::from_secs(4);
/// How long a key is used before it's looked up again (the site changes it now and then).
const KEEP: Duration = Duration::from_secs(600);
/// How long to go without ECH after no key was found, or a connection with one failed.
const RETRY: Duration = Duration::from_secs(300);

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

/// One per site. Held while a key is looked up, so requests sent together wait for one lookup
/// instead of each making their own.
static SLOTS: tokio::sync::Mutex<Option<HashMap<String, Slot>>> = tokio::sync::Mutex::const_new(None);

/// Is this a request to one of `SITES`? Only https: there's no name to hide without TLS.
fn covered(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let ours = SITES.iter().any(|site| host == *site || host.ends_with(&format!(".{site}")));
    ours && url.scheme() == "https"
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
async fn lookup(http: &reqwest::Client, host: &str) -> Option<Record> {
    for resolver in RESOLVERS {
        let question = http.get(resolver).query(&[("name", host), ("type", "HTTPS")]);
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

/// A client like the launcher's usual one, but with ECH for `host`.
fn hidden_client(host: &str, found: Record) -> Option<reqwest::Client> {
    let tls = tls_config(found.ech).ok()?;
    let mut builder = crate::net::client_builder().use_preconfigured_tls(tls);
    if !found.addrs.is_empty() {
        // The record's own addresses, so a wrong answer from the network's DNS doesn't matter.
        builder = builder.resolve_to_addrs(host, &found.addrs);
    }
    builder.build().ok()
}

/// The ECH client for `host`, looking its key up when there's none yet or it's old. `plain` asks
/// the resolvers (through the player's proxy, like everything else).
async fn client(plain: &reqwest::Client, host: &str) -> Option<reqwest::Client> {
    let mut slots = SLOTS.lock().await;
    let slots = slots.get_or_insert_with(HashMap::new);
    let before = slots.get(host).map(|slot| slot.client.is_some());
    if let Some(slot) = slots.get(host).filter(|slot| Instant::now() < slot.until) {
        return slot.client.clone();
    }
    let found = lookup(plain, host).await;
    let client = found.and_then(|found| hidden_client(host, found));
    if before != Some(client.is_some()) {
        let how = if client.is_some() { "with its name hidden (ECH)" } else { "without ECH (no key found)" };
        crate::trail::note(format!("note: {host} is asked {how}"));
    }
    let keep = if client.is_some() { KEEP } else { RETRY };
    slots.insert(host.to_string(), Slot { client: client.clone(), until: Instant::now() + keep });
    client
}

/// A connection with the key failed (an old key, or a network that breaks ECH): go without for a
/// while, then look the key up again.
async fn forget(host: &str) {
    crate::trail::note(format!("note: {host} didn't connect with ECH; asking without it for a while"));
    let mut slots = SLOTS.lock().await;
    let slot = Slot { client: None, until: Instant::now() + RETRY };
    slots.get_or_insert_with(HashMap::new).insert(host.to_string(), slot);
}

/// Sends `rb`: over ECH when it's for one of `SITES` and the site has a key, and the way it was
/// built otherwise. What was set on the request itself (headers, body, timeout) is kept; the ECH
/// client has the same settings as the launcher's usual one.
pub async fn send(rb: reqwest::RequestBuilder) -> reqwest::Result<reqwest::Response> {
    let (plain, request) = rb.build_split();
    let request = request?;
    if !covered(request.url()) {
        return plain.execute(request).await;
    }
    let host = request.url().host_str().unwrap_or_default().to_ascii_lowercase();
    let Some(hidden) = client(&plain, &host).await else {
        return plain.execute(request).await;
    };
    // A copy to send the usual way if ECH doesn't connect (None for a body that's a stream, which
    // can only be sent once).
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
    forget(&host).await;
    match spare {
        Some(request) => plain.execute(request).await,
        None => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key list as recomplauncher.com published it (public name cloudflare-ech.com).
    const KEYS: &str = "AEX+DQBBkQAgACBoT5VhYTiaWYcaj5Fl2xh9sYET7gH350Ca603RZNj1cgAEAAEAAQASY2xvdWRmbGFyZS1lY2guY29tAAA=";

    fn answer(data: &str) -> Value {
        serde_json::json!({ "Status": 0, "Answer": [{ "name": "recomplauncher.com", "type": 65, "TTL": 300, "data": data }] })
    }

    #[test]
    fn only_the_launchers_own_site_is_covered() {
        let covers = |url: &str| covered(&reqwest::Url::parse(url).unwrap());
        assert!(covers("https://recomplauncher.com/pc-catalog.json"));
        assert!(covers("https://api.recomplauncher.com/api/launcher.php?a=me"));
        assert!(covers("https://RecompLauncher.com/"));
        for other in ["https://github.com/a/b", "https://notrecomplauncher.com/", "https://recomplauncher.com.evil.example/", "https://recompstore.com/api/thumb.php"] {
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
}
