//! AWS Signature Version 4 (the `Authorization` header), checked on the receiving side (doc 39).
//!
//! The SDK signs the method, the path, the query, some headers and the SHA-256 of the body with a key derived from the
//! secret, the date, the region and the service. The gateway repeats the computation with the secret from the
//! credentials file and compares: a mismatch is `SignatureDoesNotMatch`.
//!
//! Not supported: signatures in the query string (presigned URLs), the chunked/streaming payloads and the
//! `x-amz-security-token` as an authority (it is accepted when signed and ignored).

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
/// How far from the node's clock the signature date may be (the same 15 minutes as AWS).
pub const MAX_SKEW_MS: u64 = 15 * 60 * 1000;

/// Why a signature was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigError {
    /// No `Authorization` header (or it is not SigV4).
    Missing,
    /// The header exists but cannot be read.
    Malformed(String),
    /// The date of the signature is too far from the node's clock.
    Expired,
    /// The signature does not match what the node computed.
    Mismatch,
}

/// The parts of the `Authorization` header.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub access_key: String,
    pub date: String,
    pub region: String,
    pub service: String,
    signed_headers: Vec<String>,
    signature: String,
}

/// `AWS4-HMAC-SHA256 Credential=AKID/20231104/us-east-1/sqs/aws4_request, SignedHeaders=host;x-amz-date, Signature=...`
pub fn parse(headers: &HeaderMap) -> Result<Parsed, SigError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(SigError::Missing)?
        .to_str()
        .map_err(|_| SigError::Malformed("the Authorization header is not text".into()))?;
    let rest = value
        .strip_prefix(ALGORITHM)
        .ok_or(SigError::Missing)?
        .trim();
    let (mut credential, mut signed, mut signature) = (None, None, None);
    for part in rest.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("Credential=") {
            credential = Some(v);
        } else if let Some(v) = part.strip_prefix("SignedHeaders=") {
            signed = Some(v);
        } else if let Some(v) = part.strip_prefix("Signature=") {
            signature = Some(v);
        }
    }
    let bad = |what: &str| SigError::Malformed(format!("the Authorization header has no {what}"));
    let credential = credential.ok_or_else(|| bad("Credential"))?;
    let signed = signed.ok_or_else(|| bad("SignedHeaders"))?;
    let signature = signature.ok_or_else(|| bad("Signature"))?;
    // The access key may not contain `/`; the scope is the last four parts.
    let pieces: Vec<&str> = credential.split('/').collect();
    if pieces.len() != 5 || pieces[4] != "aws4_request" || pieces[0].is_empty() {
        return Err(SigError::Malformed(
            "the Credential scope must be <key>/<date>/<region>/<service>/aws4_request".into(),
        ));
    }
    let signed_headers: Vec<String> = signed.split(';').map(str::to_ascii_lowercase).collect();
    if signed_headers.iter().any(String::is_empty) || !signed_headers.iter().any(|h| h == "host") {
        return Err(SigError::Malformed("SignedHeaders must list `host`".into()));
    }
    Ok(Parsed {
        access_key: pieces[0].to_owned(),
        date: pieces[1].to_owned(),
        region: pieces[2].to_owned(),
        service: pieces[3].to_owned(),
        signed_headers,
        signature: signature.to_ascii_lowercase(),
    })
}

/// What the signature covers besides the headers.
pub struct Signed<'a> {
    pub method: &'a str,
    /// The path as it came on the wire (percent-encoded).
    pub path: &'a str,
    /// The query as it came on the wire, without the `?`.
    pub query: &'a str,
    pub headers: &'a HeaderMap,
    pub body: &'a [u8],
}

impl Parsed {
    /// Checks the date and the signature with the secret of the key. `now_ms` is the node's clock.
    pub fn verify(&self, secret: &str, request: &Signed<'_>, now_ms: u64) -> Result<(), SigError> {
        let amz_date = request
            .headers
            .get("x-amz-date")
            .or_else(|| request.headers.get(axum::http::header::DATE))
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| SigError::Malformed("the request has no x-amz-date".into()))?;
        let signed_at = parse_amz_date(amz_date)
            .ok_or_else(|| SigError::Malformed("x-amz-date is not YYYYMMDDTHHMMSSZ".into()))?;
        if now_ms.abs_diff(signed_at) > MAX_SKEW_MS {
            return Err(SigError::Expired);
        }
        if !amz_date.starts_with(&self.date) {
            return Err(SigError::Mismatch);
        }
        let payload_hash = match request
            .headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
        {
            Some("UNSIGNED-PAYLOAD") => "UNSIGNED-PAYLOAD".to_owned(),
            Some(declared) if declared.starts_with("STREAMING-") => {
                return Err(SigError::Malformed(
                    "streaming (chunked) payload signatures are not supported".into(),
                ));
            }
            Some(declared) => {
                // The declared hash is part of what is signed, so it must also be the real one.
                if !declared.eq_ignore_ascii_case(&hex(&Sha256::digest(request.body))) {
                    return Err(SigError::Mismatch);
                }
                declared.to_ascii_lowercase()
            }
            None => hex(&Sha256::digest(request.body)),
        };
        let canonical = self.canonical_request(request, &payload_hash)?;
        let scope = format!(
            "{}/{}/{}/aws4_request",
            self.date, self.region, self.service
        );
        let to_sign = format!(
            "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
            hex(&Sha256::digest(canonical.as_bytes()))
        );
        let key = signing_key(secret, &self.date, &self.region, &self.service);
        let expected = hex(&hmac_sha256(&key, to_sign.as_bytes()));
        if constant_time_eq(expected.as_bytes(), self.signature.as_bytes()) {
            Ok(())
        } else {
            Err(SigError::Mismatch)
        }
    }

    fn canonical_request(
        &self,
        request: &Signed<'_>,
        payload_hash: &str,
    ) -> Result<String, SigError> {
        let mut headers = String::new();
        for name in &self.signed_headers {
            let values: Vec<String> = request
                .headers
                .get_all(name.as_str())
                .iter()
                .map(|v| collapse_spaces(&String::from_utf8_lossy(v.as_bytes())))
                .collect();
            if values.is_empty() {
                return Err(SigError::Mismatch);
            }
            headers.push_str(name);
            headers.push(':');
            headers.push_str(&values.join(","));
            headers.push('\n');
        }
        Ok(format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            request.method,
            canonical_path(request.path),
            canonical_query(request.query),
            headers,
            self.signed_headers.join(";"),
            payload_hash
        ))
    }
}

/// Signs a request: used by the tests (and by whoever needs a client) to produce what an SDK would.
pub fn sign(
    access_key: &str,
    secret: &str,
    region: &str,
    service: &str,
    amz_date: &str,
    request: &Signed<'_>,
    signed_headers: &[&str],
) -> String {
    let date = &amz_date[..8];
    let parsed = Parsed {
        access_key: access_key.to_owned(),
        date: date.to_owned(),
        region: region.to_owned(),
        service: service.to_owned(),
        signed_headers: signed_headers.iter().map(|h| (*h).to_owned()).collect(),
        signature: String::new(),
    };
    let payload_hash = hex(&Sha256::digest(request.body));
    let canonical = parsed
        .canonical_request(request, &payload_hash)
        .unwrap_or_default();
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let signature = hex(&hmac_sha256(
        &signing_key(secret, date, region, service),
        to_sign.as_bytes(),
    ));
    format!(
        "{ALGORITHM} Credential={access_key}/{scope}, SignedHeaders={}, Signature={signature}",
        signed_headers.join(";")
    )
}

/// `20231104T123456Z` in milliseconds since the epoch.
pub fn parse_amz_date(text: &str) -> Option<u64> {
    if text.len() != 16 || !text.ends_with('Z') || text.as_bytes()[8] != b'T' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<u32>().ok();
    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    let (hour, minute, second) = (number(9..11)?, number(11..13)?, number(13..15)?);
    let date = time::Date::from_calendar_date(
        i32::try_from(year).ok()?,
        time::Month::try_from(u8::try_from(month).ok()?).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()?;
    let at = time::PrimitiveDateTime::new(
        date,
        time::Time::from_hms(
            u8::try_from(hour).ok()?,
            u8::try_from(minute).ok()?,
            u8::try_from(second).ok()?,
        )
        .ok()?,
    )
    .assume_utc();
    u64::try_from(at.unix_timestamp()).ok().map(|s| s * 1000)
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Trims and collapses runs of spaces, as the canonical headers ask.
fn collapse_spaces(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The characters AWS leaves as they are: `A-Za-z0-9-_.~`.
fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

fn encode(text: &[u8], keep_slash: bool) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(text.len());
    for &byte in text {
        if is_unreserved(byte) || (keep_slash && byte == b'/') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Percent-decoding of a path or query component (`+` is **not** a space here; the query decoder handles it).
pub fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(value) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Services other than S3 encode the path a second time: the wire path (already encoded once) goes through the
/// encoder again, keeping the `/`.
fn canonical_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    encode(path.as_bytes(), true)
}

/// The query pairs, decoded, encoded again with the strict set and sorted.
fn canonical_query(query: &str) -> String {
    let mut pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = percent_decode(&name.replace('+', " "));
            let value = percent_decode(&value.replace('+', " "));
            (encode(&name, false), encode(&value, false))
        })
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests;
