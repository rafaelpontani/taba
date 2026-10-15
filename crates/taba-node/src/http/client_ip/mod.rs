//! The client IP (doc 12, "Client identification"): the connection's address, or the one in the
//! `client-ip.headers` header when the node is behind a load balancer. Applies to the rate
//! limit and to `operations.allowed-ips`.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;
use ipnet::IpNet;

use crate::config::ClientIp;

#[derive(Debug, Clone)]
pub struct ClientIpRules {
    headers_enabled: bool,
    headers: Vec<String>,
    trusted_proxies: Vec<IpNet>,
}

impl ClientIpRules {
    pub fn from_config(config: &ClientIp) -> Self {
        Self {
            headers_enabled: config.headers_enabled,
            headers: config.headers.clone(),
            trusted_proxies: config.trusted_proxies.clone(),
        }
    }

    /// The client IP. The headers only apply if they are on and the connection comes from a
    /// trusted proxy (empty list: from any origin). The **first header present** applies and,
    /// in it, the first address; absent or invalid, it falls back to the connection's address.
    pub fn resolve(&self, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
        let peer_ip = peer.ip().to_canonical();
        let trusted = self.trusted_proxies.is_empty()
            || self
                .trusted_proxies
                .iter()
                .any(|net| net.contains(&peer_ip));
        if !self.headers_enabled || !trusted {
            return peer_ip;
        }
        for name in &self.headers {
            let Some(value) = headers.get(name.as_str()) else {
                continue;
            };
            return value
                .to_str()
                .ok()
                .and_then(|text| text.split(',').next())
                .and_then(|first| parse_address(first.trim()))
                .unwrap_or(peer_ip);
        }
        peer_ip
    }
}

/// `1.2.3.4`, `1.2.3.4:5678`, `::1` or `[::1]:5678`.
fn parse_address(text: &str) -> Option<IpAddr> {
    text.parse::<IpAddr>()
        .or_else(|_| text.parse::<SocketAddr>().map(|addr| addr.ip()))
        .ok()
        .map(|ip| ip.to_canonical())
}

#[cfg(test)]
mod tests;
