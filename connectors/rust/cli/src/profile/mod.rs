//! The connection profiles and the `config.yml` file (doc 19b, section 18.11.2).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::yaml::{self, Node};

/// The fields of a profile, in the order they are written and shown.
pub const FIELDS: [&str; 9] = [
    "url",
    "ops-url",
    "token-file",
    "token",
    "ca-file",
    "output",
    "connect-timeout",
    "timeout",
    "max-attempts",
];

pub const DEFAULT_URL: &str = "http://127.0.0.1:8080";
pub const OUTPUTS: [&str; 3] = ["json", "table", "raw"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    MacOs,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// A profile: only the fields the person set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    values: BTreeMap<String, String>,
}

impl Profile {
    pub fn get(&self, field: &str) -> Option<&str> {
        self.values.get(field).map(String::as_str)
    }

    /// Sets a field, after validating it.
    pub fn set(&mut self, field: &str, value: &str) -> Result<(), String> {
        validate_field(field, value)?;
        self.values.insert(field.to_owned(), value.to_owned());
        Ok(())
    }

    pub fn unset(&mut self, field: &str) {
        self.values.remove(field);
    }

    /// The fields set, in canonical order.
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        FIELDS
            .iter()
            .filter_map(|f| self.values.get(*f).map(|v| (*f, v.as_str())))
            .collect()
    }
}

/// Checks a field's value without calling the server or touching files.
pub fn validate_field(field: &str, value: &str) -> Result<(), String> {
    if !FIELDS.contains(&field) {
        return Err(format!(
            "`{field}` is not a profile field ({})",
            FIELDS.join(", ")
        ));
    }
    let bad = |m: &str| Err(format!("{field}: {m}"));
    match field {
        "url" | "ops-url" => taba_client::http::Url::parse(value)
            .map(|_| ())
            .or_else(|e| bad(&e)),
        "output" if !OUTPUTS.contains(&value) => bad("use json, table or raw"),
        "connect-timeout" => number(value, 1, 300)
            .map(|_| ())
            .or_else(|_| bad("use a number from 1 to 300 seconds")),
        "timeout" => number(value, 1, 3600)
            .map(|_| ())
            .or_else(|_| bad("use a number from 1 to 3600 seconds")),
        "max-attempts" => number(value, 1, 10)
            .map(|_| ())
            .or_else(|_| bad("use a number from 1 to 10")),
        "token-file" | "token" | "ca-file" if value.trim().is_empty() => {
            bad("the value cannot be empty")
        }
        _ => Ok(()),
    }
}

fn number(value: &str, min: u64, max: u64) -> Result<u64, ()> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or(())
}

/// A profile's name: ASCII letters, digits, `-` and `_`, from 1 to 64 characters.
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "the profile name `{name}` is invalid: use 1 to 64 letters, digits, `-` or `_`"
        ))
    }
}

/// The whole file: the default profile and the profiles, in the order they were created.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigFile {
    pub default: Option<String>,
    pub profiles: Vec<(String, Profile)>,
}

impl ConfigFile {
    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Profile> {
        self.profiles
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p)
    }

    pub fn names(&self) -> Vec<&str> {
        self.profiles.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Creates a profile. The file's first profile becomes the default. Error if the name already exists.
    pub fn add(&mut self, name: &str, profile: Profile) -> Result<(), String> {
        validate_name(name)?;
        if self.get(name).is_some() {
            return Err(format!(
                "the profile `{name}` already exists (use `profile set`)"
            ));
        }
        self.profiles.push((name.to_owned(), profile));
        if self.profiles.len() == 1 {
            self.default = Some(name.to_owned());
        }
        Ok(())
    }

    /// Removes a profile; if it was the default, the file is left with no default.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let before = self.profiles.len();
        self.profiles.retain(|(n, _)| n != name);
        if self.profiles.len() == before {
            return Err(format!("the profile `{name}` does not exist"));
        }
        if self.default.as_deref() == Some(name) {
            self.default = None;
        }
        Ok(())
    }

    pub fn set_default(&mut self, name: &str) -> Result<(), String> {
        if self.get(name).is_none() {
            return Err(format!("the profile `{name}` does not exist"));
        }
        self.default = Some(name.to_owned());
        Ok(())
    }

    pub fn parse(text: &str) -> Result<ConfigFile, String> {
        let map = yaml::parse(text).map_err(|e| e.to_string())?;
        let mut file = ConfigFile::default();
        for (key, node) in map {
            match (key.as_str(), node) {
                ("default", Node::Text(name)) => {
                    if !name.is_empty() {
                        file.default = Some(name);
                    }
                }
                ("profiles", Node::Map(profiles)) => {
                    for (name, node) in profiles {
                        validate_name(&name)?;
                        let Node::Map(fields) = node else {
                            return Err(format!("the profile `{name}` must be a map of fields"));
                        };
                        let mut profile = Profile::default();
                        for (field, value) in fields {
                            let Node::Text(value) = value else {
                                return Err(format!("`{name}.{field}` must be a text"));
                            };
                            profile
                                .set(&field, &value)
                                .map_err(|e| format!("profile `{name}`: {e}"))?;
                        }
                        file.profiles.push((name, profile));
                    }
                }
                ("default" | "profiles", _) => return Err(format!("`{key}` has the wrong type")),
                _ => {
                    return Err(format!(
                        "unknown key `{key}` (use `default` and `profiles`)"
                    ))
                }
            }
        }
        if let Some(default) = &file.default {
            if file.get(default).is_none() {
                return Err(format!(
                    "the default profile `{default}` does not exist in `profiles`"
                ));
            }
        }
        Ok(file)
    }

    pub fn render(&self) -> String {
        let mut map = Vec::new();
        if let Some(default) = &self.default {
            map.push(("default".to_owned(), Node::Text(default.clone())));
        }
        let profiles = self
            .profiles
            .iter()
            .map(|(name, profile)| {
                let fields = profile
                    .entries()
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), Node::Text(v.to_owned())))
                    .collect();
                (name.clone(), Node::Map(fields))
            })
            .collect();
        map.push(("profiles".to_owned(), Node::Map(profiles)));
        format!(
            "# taba-cli profiles. The file is rewritten by `taba-cli profile`; comments are not kept.\n{}",
            yaml::render(&map)
        )
    }
}

/// The path of `config.yml`: `TABA_CLI_CONFIG`, or the user's configuration folder on the system.
pub fn config_path(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = env("TABA_CLI_CONFIG").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let missing = |var: &str| {
        format!("could not find the user's configuration folder (the variable {var} does not exist); use TABA_CLI_CONFIG")
    };
    let non_empty = |name: &str| env(name).filter(|v| !v.is_empty());
    let dir = match os {
        Os::Windows => PathBuf::from(non_empty("APPDATA").ok_or_else(|| missing("APPDATA"))?),
        Os::MacOs => PathBuf::from(non_empty("HOME").ok_or_else(|| missing("HOME"))?)
            .join("Library")
            .join("Application Support"),
        Os::Linux => match non_empty("XDG_CONFIG_HOME") {
            Some(xdg) => PathBuf::from(xdg),
            None => {
                PathBuf::from(non_empty("HOME").ok_or_else(|| missing("HOME"))?).join(".config")
            }
        },
    };
    Ok(dir.join("taba").join("config.yml"))
}

/// Reads the file; if it does not exist, it is an empty file. Unreadable or invalid: the error gives the path.
pub fn load(path: &Path) -> Result<ConfigFile, String> {
    match fs::read_to_string(path) {
        Ok(text) => ConfigFile::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

/// Rewrites the whole file atomically: writes a temporary file next to it and renames it
/// over the original. On Unix the file is created with `0600` permission.
pub fn save(path: &Path, file: &ConfigFile) -> Result<(), String> {
    let fail = |what: &str, e: std::io::Error| format!("could not {what} {}: {e}", path.display());
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| fail("create the folder", e))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config.yml"),
        std::process::id()
    ));
    let write = || -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut out = options.open(&tmp)?;
        out.write_all(file.render().as_bytes())?;
        out.sync_all()?;
        drop(out);
        fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        fail("write", e)
    })
}

#[cfg(test)]
mod tests;
