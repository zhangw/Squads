//! Remote-content safety helpers (codex-security remediation).
//! Origin allowlists, private-address checks, media budgets and web-link gating.

use std::net::{IpAddr, ToSocketAddrs};

/// Maximum size of any automatically downloaded media response.
pub const MAX_MEDIA_BYTES: usize = 20 * 1024 * 1024; // 20 MiB

/// Exact host-or-subdomain match ("evil-teams.microsoft.com.attacker.com" does NOT match).
fn host_is_exact_subdomain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{}", domain))
}

/// Strict HTTPS-only URL parse: no userinfo, no explicit port, host present.
fn parse_https_no_credentials(url_str: &str) -> Option<url::Url> {
    let u = url::Url::parse(url_str).ok()?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
        return None;
    }
    Some(u)
}

/// Microsoft-owned hosts allowed to receive the Skype token for AMS images.
pub fn is_allowed_ams_image_url(url_str: &str) -> bool {
    match parse_https_no_credentials(url_str) {
        Some(u) => u.host_str().map(|h| {
            let h = h.to_ascii_lowercase();
            host_is_exact_subdomain(&h, "skype.com")
                || host_is_exact_subdomain(&h, "teams.microsoft.com")
                || host_is_exact_subdomain(&h, "office.net")
        }).unwrap_or(false),
        None => false,
    }
}

/// Giphy media origins for the GIF download path.
pub fn is_allowed_giphy_url(url_str: &str) -> bool {
    match parse_https_no_credentials(url_str) {
        Some(u) => u.host_str().map(|h| host_is_exact_subdomain(&h.to_ascii_lowercase(), "giphy.com")).unwrap_or(false),
        None => false,
    }
}

/// Only http/https links may be dispatched to OS URL handlers.
pub fn is_safe_web_link(url_str: &str) -> bool {
    match url::Url::parse(url_str) {
        Ok(u) => matches!(u.scheme(), "http" | "https") && u.host_str().is_some(),
        Err(_) => false,
    }
}

/// True when the address is loopback, private, link-local, CGNAT or multicast.
pub fn ip_is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 0
                || o[0] == 10
                || o[0] == 127
                || (o[0] == 100 && (o[1] & 0xc0) == 0x40) // 100.64.0.0/10
                || (o[0] == 169 && o[1] == 254) // 169.254.0.0/16
                || (o[0] == 172 && (16..=31).contains(&o[1])) // 172.16.0.0/12
                || (o[0] == 192 && o[1] == 168) // 192.168.0.0/16
                || o[0] >= 224
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            (s[0] & 0xfe00) == 0xfc00 // fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // fe80::/10
                || v6.is_loopback()
                || v6.is_multicast()
        }
    }
}

/// True if the host is an IP literal, an obviously local name, or resolves
/// exclusively to loopback/private/link-local addresses. Fail closed.
pub fn host_is_local_or_private(host: &str) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_is_private(&ip);
    }
    let h = host.to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".local") || h.ends_with(".internal") {
        return true;
    }
    match (host, 443u16).to_socket_addrs() {
        Ok(addrs) => {
            let addrs: Vec<_> = addrs.collect();
            !addrs.is_empty() && addrs.iter().all(|a| ip_is_private(&a.ip()))
        }
        Err(_) => true, // fail closed on resolution errors
    }
}

/// Read a response body with a hard byte cap (no full-body buffering beyond the cap).
pub async fn read_limited(res: &mut reqwest::Response, max: usize) -> Result<bytes::Bytes, String> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        if buf.len() + chunk.len() > max {
            return Err("media response exceeds size limit".into());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(bytes::Bytes::from(buf))
}

/// Magic-byte validation for the two media formats this client downloads.
pub fn validate_media_bytes(extension: &str, data: &[u8]) -> bool {
    match extension {
        "jpeg" => is_jpeg(data),
        "gif" => is_gif(data),
        _ => true,
    }
}

pub fn is_jpeg(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF
}

pub fn is_gif(data: &[u8]) -> bool {
    data.len() >= 6 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ams_urls_allowed() {
        assert!(is_allowed_ams_image_url("https://api.spaces.skype.com/api/ams/v1/objects/x/views/imgo"));
        assert!(is_allowed_ams_image_url("https://eu-api.asm.skype.com/v1/objects/x/views/imgo"));
        assert!(is_allowed_ams_image_url("https://eu-prod.asyncgw.teams.microsoft.com/v1/objects/x"));
    }

    #[test]
    fn ams_urls_rejected() {
        assert!(!is_allowed_ams_image_url("https://attacker.example/img"));
        assert!(!is_allowed_ams_image_url("http://api.spaces.skype.com/x"));
        assert!(!is_allowed_ams_image_url("https://teams.microsoft.com.evil.com/x"));
        assert!(!is_allowed_ams_image_url("https://user:pass@api.spaces.skype.com/x"));
        assert!(!is_allowed_ams_image_url("https://api.spaces.skype.com:8443/x"));
        assert!(!is_allowed_ams_image_url("https://127.0.0.1/x"));
        assert!(!is_allowed_ams_image_url("not a url"));
    }

    #[test]
    fn giphy_urls() {
        assert!(is_allowed_giphy_url("https://media.giphy.com/media/abc/giphy.gif"));
        assert!(is_allowed_giphy_url("https://i.giphy.com/x.gif"));
        assert!(!is_allowed_giphy_url("https://giphy.com.attacker.example/x.gif"));
        assert!(!is_allowed_giphy_url("https://127.0.0.1/x.gif"));
        assert!(!is_allowed_giphy_url("https://media.giphy.com:8080/x.gif"));
    }

    #[test]
    fn web_links() {
        assert!(is_safe_web_link("https://example.com/a"));
        assert!(is_safe_web_link("http://example.com"));
        assert!(!is_safe_web_link("file:///etc/passwd"));
        assert!(!is_safe_web_link("javascript:alert(1)"));
        assert!(!is_safe_web_link("data:text/html,x"));
        assert!(!is_safe_web_link("msteams://teams.example"));
        assert!(!is_safe_web_link(""));
    }

    #[test]
    fn private_ips() {
        assert!(ip_is_private(&"127.0.0.1".parse().unwrap()));
        assert!(ip_is_private(&"10.1.2.3".parse().unwrap()));
        assert!(ip_is_private(&"192.168.1.1".parse().unwrap()));
        assert!(ip_is_private(&"172.16.0.1".parse().unwrap()));
        assert!(ip_is_private(&"169.254.1.1".parse().unwrap()));
        assert!(!ip_is_private(&"8.8.8.8".parse().unwrap()));
        assert!(!ip_is_private(&"1.1.1.1".parse().unwrap()));
        assert!(ip_is_private(&"::1".parse().unwrap()));
        assert!(ip_is_private(&"fe80::1".parse().unwrap()));
    }

    #[test]
    fn media_magic() {
        assert!(is_gif(b"GIF89a123"));
        assert!(is_gif(b"GIF87a123"));
        assert!(!is_gif(b"PNG123456"));
        assert!(is_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_jpeg(b"GIF89a"));
    }
}
