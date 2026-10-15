//! The effective configuration of a call: `TABA_CLI_*` variable, chosen profile and built-in value,
//! field by field, remembering where each value came from (doc 19b, section 18.11.2).

use crate::profile::{self, ConfigFile, Profile, DEFAULT_URL, FIELDS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// An environment variable (`TABA_CLI_URL`).
    Variable(String),
    /// The profile with this name.
    Profile(String),
    /// The built-in value.
    Default,
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Variable(v) => format!("variable {v}"),
            Source::Profile(p) => format!("profile {p}"),
            Source::Default => "default".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    /// `None` when the field has no value or default (token, ca-file).
    pub value: Option<String>,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// The profile in use and why it was chosen (`--profile`, `TABA_CLI_PROFILE` or the file's default).
    pub profile: Option<(String, Source)>,
    settings: Vec<(&'static str, Setting)>,
}

pub fn variable_name(field: &str) -> String {
    format!("TABA_CLI_{}", field.to_ascii_uppercase().replace('-', "_"))
}

/// Chooses the profile: `--profile`, then `TABA_CLI_PROFILE`, then the file's `default`.
fn choose_profile(
    file: &ConfigFile,
    flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<(String, Source)>, String> {
    let from_env = env("TABA_CLI_PROFILE").filter(|v| !v.is_empty());
    let (name, source) = match (flag, from_env, &file.default) {
        (Some(name), _, _) => (name.to_owned(), Source::Variable("--profile".to_owned())),
        (None, Some(name), _) => (name, Source::Variable("TABA_CLI_PROFILE".to_owned())),
        (None, None, Some(name)) => (name.clone(), Source::Default),
        (None, None, None) => return Ok(None),
    };
    if file.get(&name).is_none() {
        let known = file.names();
        return Err(format!(
            "the profile `{name}` does not exist{}",
            if known.is_empty() {
                " (there are no profiles; create one with `taba-cli profile add`)".to_owned()
            } else {
                format!(" (profiles: {})", known.join(", "))
            }
        ));
    }
    Ok(Some((name, source)))
}

pub fn resolve(
    file: &ConfigFile,
    profile_flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Effective, String> {
    let chosen = choose_profile(file, profile_flag, env)?;
    let empty = Profile::default();
    let profile = chosen
        .as_ref()
        .and_then(|(name, _)| file.get(name))
        .unwrap_or(&empty);
    let profile_name = chosen.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();

    let mut settings = Vec::new();
    for field in FIELDS {
        let var = variable_name(field);
        let setting = if let Some(value) = env(&var).filter(|v| !v.is_empty()) {
            profile::validate_field(field, &value).map_err(|e| format!("{var}: {e}"))?;
            Setting {
                value: Some(value),
                source: Source::Variable(var),
            }
        } else if let Some(value) = profile.get(field) {
            Setting {
                value: Some(value.to_owned()),
                source: Source::Profile(profile_name.clone()),
            }
        } else {
            Setting {
                value: builtin(field).map(str::to_owned),
                source: Source::Default,
            }
        };
        settings.push((field, setting));
    }
    Ok(Effective {
        profile: chosen,
        settings,
    })
}

fn builtin(field: &str) -> Option<&'static str> {
    match field {
        "url" => Some(DEFAULT_URL),
        "output" => Some("json"),
        "connect-timeout" => Some("5"),
        "timeout" => Some("30"),
        "max-attempts" => Some("3"),
        _ => None,
    }
}

impl Effective {
    pub fn setting(&self, field: &str) -> &Setting {
        &self
            .settings
            .iter()
            .find(|(f, _)| *f == field)
            .expect("known field")
            .1
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.setting(field).value.as_deref()
    }

    pub fn url(&self) -> &str {
        self.get("url").unwrap_or(DEFAULT_URL)
    }

    /// The operations API address: the `ops-url`, or the `url` itself. Without `operations.port`, the node
    /// serves the operations routes on the client API port.
    pub fn ops_url(&self) -> String {
        self.get("ops-url").unwrap_or_else(|| self.url()).to_owned()
    }

    pub fn number(&self, field: &str) -> u64 {
        self.get(field).and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    /// Which of the two applies when there is both `token` and `token-file`: the one of higher precedence (variable, then
    /// profile) and, at the same level, the `token-file`.
    pub fn token_source(&self) -> TokenSource<'_> {
        let rank = |s: &Setting| match (&s.value, &s.source) {
            (None, _) => 0,
            (_, Source::Variable(_)) => 2,
            (_, _) => 1,
        };
        let (file, text) = (self.setting("token-file"), self.setting("token"));
        match (rank(file), rank(text)) {
            (0, 0) => TokenSource::None,
            (f, t) if f >= t => TokenSource::File(file.value.as_deref().unwrap_or_default()),
            _ => TokenSource::Text(text.value.as_deref().unwrap_or_default()),
        }
    }

    /// All the fields, for `config show`.
    pub fn settings(&self) -> impl Iterator<Item = (&'static str, &Setting)> {
        self.settings.iter().map(|(f, s)| (*f, s))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource<'a> {
    None,
    File(&'a str),
    Text(&'a str),
}

#[cfg(test)]
mod tests;
