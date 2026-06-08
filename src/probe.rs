// ABOUTME: Captive-portal connectivity probe over plain HTTP.
// ABOUTME: Classifies the network as Online, CaptivePortal, or Offline.

// `probe_once` runs from a spawned task, and the `url` carried by `CaptivePortal` is read
// through `App.portal` by the TUI connectivity banner — another module. A binary crate can't
// see those cross-module uses, so the field reads as dead here; suppress it module-wide.
#![allow(dead_code)]

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Online,
    CaptivePortal { url: String },
    Offline,
}

/// Classify an HTTP status line + body length from a known captive-check endpoint.
/// Apple's `captive.apple.com` returns 200 with the exact body "Success" when open;
/// a portal returns a redirect (3xx) or a different 200 body.
fn classify(status: u16, body_is_success: bool, probe_url: &str) -> ProbeResult {
    match status {
        200 if body_is_success => ProbeResult::Online,
        200 => ProbeResult::CaptivePortal {
            url: probe_url.to_string(),
        },
        300..=399 => ProbeResult::CaptivePortal {
            url: probe_url.to_string(),
        },
        _ => ProbeResult::Offline,
    }
}

/// Parse `host` and `path` from a plain `http://host[/path]` URL. Returns None for https/other.
fn parse_http_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), "/".to_string()),
    };
    Some((host, path))
}

/// Perform one probe. Any connect/read failure or DNS failure => Offline.
pub async fn probe_once(url: &str) -> ProbeResult {
    let Some((host, path)) = parse_http_url(url) else {
        return ProbeResult::Offline;
    };
    let addr = format!("{host}:80");
    let fut = async {
        let mut stream = TcpStream::connect(&addr).await.ok()?;
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: pingpong\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.ok()?;
        let mut buf = Vec::new();
        // Cap the read so a portal serving a huge page can't exhaust memory;
        // the 5s timeout below guards against a server that never closes.
        let mut limited = stream.take(8192);
        limited.read_to_end(&mut buf).await.ok()?;
        let text = String::from_utf8_lossy(&buf);
        let status = text
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse::<u16>().ok())?;
        let body_is_success = text.contains("Success");
        Some(classify(status, body_is_success, url))
    };
    match tokio::time::timeout(Duration::from_secs(5), fut).await {
        Ok(Some(r)) => r,
        _ => ProbeResult::Offline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_online_on_success_body() {
        assert_eq!(classify(200, true, "u"), ProbeResult::Online);
    }

    #[test]
    fn classify_portal_on_redirect() {
        assert_eq!(
            classify(302, false, "u"),
            ProbeResult::CaptivePortal { url: "u".into() }
        );
    }

    #[test]
    fn classify_portal_on_unexpected_200() {
        assert_eq!(
            classify(200, false, "u"),
            ProbeResult::CaptivePortal { url: "u".into() }
        );
    }

    #[test]
    fn classify_offline_on_server_error() {
        assert_eq!(classify(500, false, "u"), ProbeResult::Offline);
    }

    #[test]
    fn parse_url_splits_host_and_path() {
        assert_eq!(
            parse_http_url("http://captive.apple.com"),
            Some(("captive.apple.com".into(), "/".into()))
        );
        assert_eq!(
            parse_http_url("http://h/x"),
            Some(("h".into(), "/x".into()))
        );
        assert_eq!(parse_http_url("https://h"), None);
    }
}
