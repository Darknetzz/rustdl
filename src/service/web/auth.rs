use std::net::{IpAddr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

pub const AUTH_HEADER: &str = "X-Rustdl-Token";

/// Reads the token from request headers/query-style values and compares to `expected` (trimmed).
pub fn token_matches(expected: &str, presented: Option<&str>) -> bool {
    let expected = expected.trim();
    if expected.is_empty() {
        return false;
    }
    presented
        .map(str::trim)
        .is_some_and(|t| !t.is_empty() && t == expected)
}

pub fn token_from_request(request: &Request<Body>, expected: &str) -> bool {
    let header_ok = request
        .headers()
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| token_matches(expected, Some(t)));
    let bearer_ok = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| token_matches(expected, Some(t)));
    let query_ok = request.uri().query().is_some_and(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .any(|(k, v)| k == "token" && token_matches(expected, Some(v.as_ref())))
    });
    header_ok || bearer_ok || query_ok
}

pub fn client_ip(request: &Request<Body>) -> Option<IpAddr> {
    if let Some(ConnectInfo(addr)) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return Some(addr.ip());
    }
    if let Some(xff) = request
        .headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            if let Ok(ip) = first.trim().parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }
    if let Some(xri) = request
        .headers()
        .get("X-Real-IP")
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(ip) = xri.trim().parse::<IpAddr>() {
            return Some(ip);
        }
    }
    None
}

pub fn ip_whitelisted(ip: IpAddr, whitelist: &[String]) -> bool {
    whitelist
        .iter()
        .filter_map(|entry| parse_whitelist_entry(entry.trim()))
        .any(|rule| ip_matches_rule(ip, rule))
}

fn parse_whitelist_entry(entry: &str) -> Option<WhitelistRule> {
    if entry.is_empty() {
        return None;
    }
    if let Some((net, prefix)) = entry.split_once('/') {
        let net = net.trim().parse::<IpAddr>().ok()?;
        let prefix = prefix.trim().parse::<u8>().ok()?;
        Some(WhitelistRule { net, prefix })
    } else {
        let net = entry.parse::<IpAddr>().ok()?;
        let prefix = match net {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        Some(WhitelistRule { net, prefix })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WhitelistRule {
    net: IpAddr,
    prefix: u8,
}

fn ip_matches_rule(ip: IpAddr, rule: WhitelistRule) -> bool {
    match (ip, rule.net) {
        (IpAddr::V4(ip), IpAddr::V4(net)) if rule.prefix <= 32 => {
            let ip_bits = u32::from_be_bytes(ip.octets());
            let net_bits = u32::from_be_bytes(net.octets());
            let mask = if rule.prefix == 0 {
                0
            } else {
                !0u32 << (32 - rule.prefix)
            };
            (ip_bits & mask) == (net_bits & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(net)) if rule.prefix <= 128 => {
            let ip_bits = u128::from_be_bytes(ip.octets());
            let net_bits = u128::from_be_bytes(net.octets());
            let mask = if rule.prefix == 0 {
                0
            } else {
                !0u128 << (128 - rule.prefix)
            };
            (ip_bits & mask) == (net_bits & mask)
        }
        _ => false,
    }
}

pub fn request_authorized(request: &Request<Body>, expected: &str, whitelist: &[String]) -> bool {
    if !whitelist.is_empty() {
        if let Some(ip) = client_ip(request) {
            if ip_whitelisted(ip, whitelist) {
                return true;
            }
        }
    }
    token_from_request(request, expected)
}

pub async fn require_auth(
    expected: String,
    whitelist: Vec<String>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if request_authorized(&request, &expected, &whitelist) {
        return Ok(next.run(request).await);
    }
    if expected.trim().is_empty() {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ip_whitelist_match() {
        let ip = "192.168.1.42".parse().unwrap();
        assert!(ip_whitelisted(ip, &["192.168.1.42".to_owned()]));
        assert!(!ip_whitelisted(ip, &["192.168.1.43".to_owned()]));
    }

    #[test]
    fn cidr_whitelist_match() {
        let ip = "192.168.1.42".parse().unwrap();
        assert!(ip_whitelisted(ip, &["192.168.1.0/24".to_owned()]));
        assert!(!ip_whitelisted(ip, &["192.168.2.0/24".to_owned()]));
    }

    #[test]
    fn ignores_blank_whitelist_entries() {
        let ip = "127.0.0.1".parse().unwrap();
        assert!(ip_whitelisted(
            ip,
            &[" ".to_owned(), "127.0.0.1".to_owned()]
        ));
    }
}
