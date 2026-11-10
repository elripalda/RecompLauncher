//! The launcher's own download, with GitHub's asset API as a fallback for a busy release link.

use std::future::Future;
use std::time::Duration;

fn github_url(raw: &str, host: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(raw).ok()?;
    (url.scheme() == "https" && url.host_str() == Some(host) && url.port().is_none()
        && url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none())
        .then_some(url)
}

fn release_api(download: &str) -> Option<String> {
    let url = github_url(download, "github.com")?;
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 6 || parts[2..4] != ["releases", "download"] || parts[4].is_empty() || parts[5].is_empty()
        || !crate::catalog::repo_ok(&format!("{}/{}", parts[0], parts[1])) {
        return None;
    }
    // Keep the tag's escaping: a tag can contain an encoded slash.
    Some(format!("https://api.github.com/repos/{}/{}/releases/tags/{}", parts[0], parts[1], parts[4]))
}

fn asset_api(release: &serde_json::Value, download: &str) -> Option<String> {
    release_api(download)?;
    let original = github_url(download, "github.com")?;
    let repo: Vec<_> = original.path_segments()?.take(2).collect();
    let asset = release.get("assets")?.as_array()?.iter().find(|a| a.get("browser_download_url").and_then(serde_json::Value::as_str) == Some(download))?;
    let raw = asset.get("url")?.as_str()?;
    let url = github_url(raw, "api.github.com")?;
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 6 || parts[..5] != ["repos", repo[0], repo[1], "releases", "assets"]
        || parts[5].is_empty() || !parts[5].bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(raw.to_string())
}

async fn github_asset_api(http: &reqwest::Client, download: &str) -> Option<String> {
    let url = release_api(download)?;
    let response = http.get(url).timeout(crate::net::QUICK).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    asset_api(&response.json().await.ok()?, download)
}

// The lookup is lazy: normal downloads never spend GitHub's unauthenticated API allowance.
async fn response_with<F, Fut>(http: &reqwest::Client, download: &str, fallback: F) -> Result<reqwest::Response, crate::Error>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let response = http.get(download).timeout(Duration::from_secs(900)).send().await?;
    if response.status().is_server_error() {
        if let Some(api) = fallback().await {
            crate::trail::note(format!("Launcher update: GitHub answered {}; trying its asset API", response.status().as_u16()));
            return Ok(http.get(api).header(reqwest::header::ACCEPT, "application/octet-stream").timeout(Duration::from_secs(900)).send().await?);
        }
    }
    Ok(response)
}

pub async fn response(http: &reqwest::Client, download: &str) -> Result<reqwest::Response, crate::Error> {
    response_with(http, download, || github_asset_api(http, download)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};

    const DOWNLOAD: &str = "https://github.com/owner/launcher/releases/download/v1.2/Launcher.exe";
    const API: &str = "https://api.github.com/repos/owner/launcher/releases/assets/42";

    fn site(replies: Vec<(u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in replies {
                let until = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
                        Err(e) => panic!("request did not arrive: {e}"),
                    }
                };
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut buf = [0; 1024];
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buf[..n]);
                }
                requests.push(String::from_utf8(request).unwrap());
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (base, thread)
    }

    #[tokio::test]
    async fn a_busy_download_uses_the_binary_asset_and_returns_its_bytes() {
        for status in [500, 502, 503, 504] {
            let (base, server) = site(vec![(status, "gateway timeout"), (200, "installer bytes")]);
            let api = format!("{base}/asset");
            let response = response_with(&reqwest::Client::new(), &format!("{base}/download"), || async { Some(api) }).await.unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(response.bytes().await.unwrap().as_ref(), b"installer bytes");
            let requests = server.join().unwrap();
            assert!(requests[1].starts_with("GET /asset HTTP/1.1\r\n"));
            assert!(requests[1].to_ascii_lowercase().contains("\r\naccept: application/octet-stream\r\n"));
        }
    }

    #[tokio::test]
    async fn normal_downloads_and_missing_files_do_not_spend_api_requests() {
        for status in [200, 404] {
            let (base, server) = site(vec![(status, "original answer")]);
            let response = response_with(&reqwest::Client::new(), &base, || async { panic!("only a server failure should ask GitHub's API") }).await.unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.text().await.unwrap(), "original answer");
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn an_unavailable_api_lookup_preserves_the_original_failure() {
        let (base, server) = site(vec![(504, "gateway timeout")]);
        let response = response_with(&reqwest::Client::new(), &base, || async { None }).await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(response.text().await.unwrap(), "gateway timeout");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_failed_api_download_is_reported_without_another_retry() {
        let (base, server) = site(vec![(504, "gateway timeout"), (403, "rate limited")]);
        let api = format!("{base}/asset");
        let response = response_with(&reqwest::Client::new(), &format!("{base}/download"), || async { Some(api) }).await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(response.text().await.unwrap(), "rate limited");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn non_github_downloads_keep_their_original_failure() {
        let (base, server) = site(vec![(504, "site unavailable")]);
        let answer = response(&reqwest::Client::new(), &base).await.unwrap();
        assert_eq!(answer.status(), reqwest::StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(answer.text().await.unwrap(), "site unavailable");
        server.join().unwrap();
    }

    #[test]
    fn release_lookup_uses_the_exact_github_tag() {
        assert_eq!(release_api(DOWNLOAD).as_deref(), Some("https://api.github.com/repos/owner/launcher/releases/tags/v1.2"));
        assert_eq!(release_api("https://github.com/owner/launcher/releases/download/beta%2Fv2/Launcher%20Setup.exe").as_deref(), Some("https://api.github.com/repos/owner/launcher/releases/tags/beta%2Fv2"));
        for url in [
            "http://github.com/owner/launcher/releases/download/v1/file.exe",
            "https://github.com.evil.example/owner/launcher/releases/download/v1/file.exe",
            "https://github.com@evil.example/owner/launcher/releases/download/v1/file.exe",
            "https://name@github.com/owner/launcher/releases/download/v1/file.exe",
            "https://github.com:444/owner/launcher/releases/download/v1/file.exe",
            "https://github.com/owner/launcher/releases/latest/download/file.exe",
            "https://example.com/launcher.exe",
        ] {
            assert_eq!(release_api(url), None, "{url}");
        }
    }

    #[test]
    fn the_fallback_is_only_the_matching_assets_validated_api_link() {
        let release = json!({"assets": [
            {"browser_download_url": "https://github.com/owner/launcher/releases/download/v1.2/Other.exe", "url": "https://api.github.com/repos/owner/launcher/releases/assets/41"},
            {"browser_download_url": DOWNLOAD, "url": API}
        ]});
        assert_eq!(asset_api(&release, DOWNLOAD).as_deref(), Some(API));
        for api in [
            "http://api.github.com/repos/owner/launcher/releases/assets/42",
            "https://api.github.com.evil.example/repos/owner/launcher/releases/assets/42",
            "https://api.github.com@evil.example/repos/owner/launcher/releases/assets/42",
            "https://name@api.github.com/repos/owner/launcher/releases/assets/42",
            "https://api.github.com:444/repos/owner/launcher/releases/assets/42",
            "https://api.github.com/repos/other/project/releases/assets/42",
            "https://api.github.com/repos/owner/launcher/releases/assets/no-id",
        ] {
            assert_eq!(asset_api(&json!({"assets": [{"browser_download_url": DOWNLOAD, "url": api}]}), DOWNLOAD), None, "{api}");
        }
        assert_eq!(asset_api(&json!({"assets": [{"browser_download_url": "https://github.com/owner/launcher/releases/download/v1.2/Other.exe", "url": API}]}), DOWNLOAD), None);
        assert_eq!(asset_api(&release, "https://example.com/unrelated.exe"), None);
        assert_eq!(asset_api(&json!({"message":"rate limited"}), DOWNLOAD), None);
    }
}
