//! O IP do cliente (doc 12, "Identificação do cliente"): o endereço da conexão, ou o do
//! cabeçalho de `client-ip.headers` quando o nó está atrás de balanceador. Vale para o rate
//! limit e para `operations.allowed-ips`.

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

    /// O IP do cliente. Os cabeçalhos só valem se estão ligados e a conexão vem de um proxy
    /// confiável (lista vazia: de qualquer origem). Vale o **primeiro cabeçalho presente** e,
    /// nele, o primeiro endereço; ausente ou inválido, cai no endereço da conexão.
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

/// `1.2.3.4`, `1.2.3.4:5678`, `::1` ou `[::1]:5678`.
fn parse_address(text: &str) -> Option<IpAddr> {
    text.parse::<IpAddr>()
        .or_else(|_| text.parse::<SocketAddr>().map(|addr| addr.ip()))
        .ok()
        .map(|ip| ip.to_canonical())
}

#[cfg(test)]
mod tests;
