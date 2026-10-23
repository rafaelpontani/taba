//! Where the values come from and how they become types: `.yml` file, environment variables,
//! table defaults. Precedence, from highest to lowest: environment variable,
//! file, default (doc 10, "How to configure").

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::time::Duration;

use ipnet::IpNet;

use super::error::ConfigError;
use super::params::{self, PARAMS, REMOVED};

/// The environment variables, behind a trait so the tests do not touch the process.
pub trait EnvSource {
    fn get(&self, name: &str) -> Option<String>;
    fn names(&self) -> Vec<String>;
}

pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn names(&self) -> Vec<String> {
        std::env::vars_os()
            .filter_map(|(k, _)| k.into_string().ok())
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
pub struct MapEnv(pub BTreeMap<String, String>);

impl MapEnv {
    pub fn new<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        )
    }
}

impl EnvSource for MapEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }

    fn names(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

/// The values the user set (file and environment) plus the table defaults.
pub struct Source {
    explicit: BTreeMap<String, String>,
    read: RefCell<BTreeSet<String>>,
}

impl Source {
    /// Joins the file (already read; the label goes in the messages) and the environment.
    /// Also returns the warnings (`TABA_*` variables nobody knows).
    pub fn build(
        yaml: Option<(&str, &str)>,
        env: &dyn EnvSource,
    ) -> Result<(Self, Vec<String>), ConfigError> {
        let mut explicit = BTreeMap::new();
        if let Some((label, content)) = yaml {
            flatten_yaml(label, content, &mut explicit)?;
        }

        let mut known_env = BTreeSet::new();
        known_env.insert("TABA_CONFIG_FILE".to_owned());
        for key in PARAMS.iter().map(|p| p.key).chain(REMOVED.iter().copied()) {
            let name = params::env_name(key);
            if let Some(value) = env.get(&name).filter(|v| !v.trim().is_empty()) {
                explicit.insert(key.to_owned(), value);
            }
            known_env.insert(name);
        }

        let warnings = env
            .names()
            .into_iter()
            .filter(|n| n.starts_with("TABA_") && !known_env.contains(n))
            .map(|n| format!("environment variable `{n}` unknown: ignored"))
            .collect();

        Ok((
            Self {
                explicit,
                read: RefCell::new(BTreeSet::new()),
            },
            warnings,
        ))
    }

    /// `true` if the user set the key (file or environment), not the default.
    #[cfg(test)]
    pub fn is_set(&self, key: &str) -> bool {
        self.explicit.contains_key(key)
    }

    /// The keys set by the user that start with any of the prefixes.
    pub fn set_keys_with_prefix(&self, prefixes: &[&str]) -> Vec<String> {
        self.explicit
            .keys()
            .filter(|k| prefixes.iter().any(|p| k.starts_with(p)))
            .cloned()
            .collect()
    }

    /// The table keys that `Config` read. The tests check that they are all of them.
    #[cfg(test)]
    pub fn keys_read(&self) -> BTreeSet<String> {
        self.read.borrow().clone()
    }

    fn raw(&self, key: &'static str) -> Result<Option<&str>, ConfigError> {
        let param = params::find(key).ok_or(ConfigError::NotRegistered(key))?;
        self.read.borrow_mut().insert(key.to_owned());
        Ok(self.explicit.get(key).map(String::as_str).or(param.default))
    }

    /// Required value: set by the user or with a default in the table.
    pub fn get<T: Parse>(&self, key: &'static str) -> Result<T, ConfigError> {
        self.opt(key)?.ok_or(ConfigError::Missing(key))
    }

    /// Optional value: `None` when there is neither a value nor a default.
    pub fn opt<T: Parse>(&self, key: &'static str) -> Result<Option<T>, ConfigError> {
        match self.raw(key)? {
            None => Ok(None),
            Some(raw) => T::parse(raw.trim())
                .map(Some)
                .ok_or_else(|| ConfigError::InvalidValue {
                    key,
                    env: params::env_name(key),
                    value: raw.to_owned(),
                    reason: format!("expected {}", T::EXPECTED),
                }),
        }
    }
}

/// Flattens the YAML into dotted keys: each dot of the key is a nesting level.
fn flatten_yaml(
    label: &str,
    content: &str,
    out: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    let file_err = |reason: String| ConfigError::File {
        path: label.to_owned(),
        reason,
    };
    let value: serde_norway::Value =
        serde_norway::from_str(content).map_err(|e| file_err(e.to_string()))?;

    fn walk(
        prefix: &str,
        value: &serde_norway::Value,
        out: &mut BTreeMap<String, String>,
        file_err: &dyn Fn(String) -> ConfigError,
    ) -> Result<(), ConfigError> {
        use serde_norway::Value;
        let leaf = |out: &mut BTreeMap<String, String>, text: String| -> Result<(), ConfigError> {
            if params::find(prefix).is_none() && !REMOVED.contains(&prefix) {
                return Err(ConfigError::UnknownKey(prefix.to_owned()));
            }
            out.insert(prefix.to_owned(), text);
            Ok(())
        };
        match value {
            Value::Null => Ok(()),
            Value::Bool(b) => leaf(out, b.to_string()),
            Value::Number(n) => leaf(out, n.to_string()),
            Value::String(s) => leaf(out, s.clone()),
            Value::Sequence(items) => {
                let mut parts = Vec::new();
                for item in items {
                    match item {
                        Value::String(s) => parts.push(s.clone()),
                        Value::Number(n) => parts.push(n.to_string()),
                        Value::Bool(b) => parts.push(b.to_string()),
                        _ => {
                            return Err(file_err(format!(
                                "`{prefix}`: the list only accepts simple values"
                            )));
                        }
                    }
                }
                leaf(out, parts.join(","))
            }
            Value::Mapping(map) => {
                for (k, v) in map {
                    let Value::String(name) = k else {
                        return Err(file_err(format!("non-text key below `{prefix}`")));
                    };
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}.{name}")
                    };
                    walk(&path, v, out, file_err)?;
                }
                Ok(())
            }
            Value::Tagged(_) => Err(file_err(format!("`{prefix}`: YAML tags are not accepted"))),
        }
    }

    walk("", &value, out, &file_err)
}

/// Converts a parameter's text into the requested type.
pub trait Parse: Sized {
    /// What the parameter expects, for the error message.
    const EXPECTED: &'static str;
    fn parse(s: &str) -> Option<Self>;
}

macro_rules! parse_with_from_str {
    ($($t:ty => $expected:literal),+ $(,)?) => {$(
        impl Parse for $t {
            const EXPECTED: &'static str = $expected;
            fn parse(s: &str) -> Option<Self> { <$t>::from_str(s).ok() }
        }
    )+};
}

parse_with_from_str! {
    u16 => "a port (0 to 65535)",
    u32 => "an integer",
    u64 => "an integer",
    f64 => "a number",
}

impl Parse for bool {
    const EXPECTED: &'static str = "`true` or `false`";
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

impl Parse for String {
    const EXPECTED: &'static str = "a text";
    fn parse(s: &str) -> Option<Self> {
        Some(s.to_owned())
    }
}

impl Parse for Duration {
    const EXPECTED: &'static str = "a duration with a unit (`100ms`, `60s`, `5m`, `2h` or `30d`)";
    fn parse(s: &str) -> Option<Self> {
        let split = s.find(|c: char| !c.is_ascii_digit())?;
        let (digits, unit) = s.split_at(split);
        let n: u64 = digits.parse().ok()?;
        let millis = match unit {
            "ms" => n,
            "s" => n.checked_mul(1000)?,
            "m" => n.checked_mul(60_000)?,
            "h" => n.checked_mul(3_600_000)?,
            "d" => n.checked_mul(86_400_000)?,
            _ => return None,
        };
        Some(Duration::from_millis(millis))
    }
}

/// A size in bytes. The units are binary: `5MB` is 5,242,880 bytes (doc 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bytes(pub u64);

impl Parse for Bytes {
    const EXPECTED: &'static str = "a size (`512KB`, `5MB`, `1GB`) or a number of bytes";
    fn parse(s: &str) -> Option<Self> {
        let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let (digits, unit) = s.split_at(split);
        let n: u64 = digits.parse().ok()?;
        let factor: u64 = match unit.to_ascii_uppercase().as_str() {
            "" | "B" => 1,
            "KB" => 1 << 10,
            "MB" => 1 << 20,
            "GB" => 1 << 30,
            "TB" => 1 << 40,
            _ => return None,
        };
        n.checked_mul(factor).map(Bytes)
    }
}

/// List: text separated by commas (a YAML list already arrives like this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List(pub Vec<String>);

impl Parse for List {
    const EXPECTED: &'static str = "a comma-separated list";
    fn parse(s: &str) -> Option<Self> {
        Some(List(
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
        ))
    }
}

/// List of IPs or CIDR ranges (`10.0.0.0/8, 192.168.1.10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetList(pub Vec<IpNet>);

impl Parse for NetList {
    const EXPECTED: &'static str = "a comma-separated list of IPs or CIDR ranges";
    fn parse(s: &str) -> Option<Self> {
        let mut nets = Vec::new();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let net = IpNet::from_str(part)
                .ok()
                .or_else(|| part.parse::<std::net::IpAddr>().ok().map(IpNet::from))?;
            nets.push(net);
        }
        Some(NetList(nets))
    }
}

#[cfg(test)]
mod tests;
