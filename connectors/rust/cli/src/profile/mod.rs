//! Os perfis de conexão e o arquivo `config.yml` (doc 19b, seção 18.11.2).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::yaml::{self, Node};

/// Os campos de um perfil, na ordem em que são escritos e mostrados.
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

/// Um perfil: só os campos que a pessoa definiu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    values: BTreeMap<String, String>,
}

impl Profile {
    pub fn get(&self, field: &str) -> Option<&str> {
        self.values.get(field).map(String::as_str)
    }

    /// Define um campo, depois de validá-lo.
    pub fn set(&mut self, field: &str, value: &str) -> Result<(), String> {
        validate_field(field, value)?;
        self.values.insert(field.to_owned(), value.to_owned());
        Ok(())
    }

    pub fn unset(&mut self, field: &str) {
        self.values.remove(field);
    }

    /// Os campos definidos, na ordem canônica.
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        FIELDS
            .iter()
            .filter_map(|f| self.values.get(*f).map(|v| (*f, v.as_str())))
            .collect()
    }
}

/// Confere o valor de um campo sem chamar o servidor nem tocar em arquivos.
pub fn validate_field(field: &str, value: &str) -> Result<(), String> {
    if !FIELDS.contains(&field) {
        return Err(format!(
            "`{field}` não é um campo de perfil ({})",
            FIELDS.join(", ")
        ));
    }
    let bad = |m: &str| Err(format!("{field}: {m}"));
    match field {
        "url" | "ops-url" => taba_client::http::Url::parse(value)
            .map(|_| ())
            .or_else(|e| bad(&e)),
        "output" if !OUTPUTS.contains(&value) => bad("use json, table ou raw"),
        "connect-timeout" => number(value, 1, 300)
            .map(|_| ())
            .or_else(|_| bad("use um número de 1 a 300 segundos")),
        "timeout" => number(value, 1, 3600)
            .map(|_| ())
            .or_else(|_| bad("use um número de 1 a 3600 segundos")),
        "max-attempts" => number(value, 1, 10)
            .map(|_| ())
            .or_else(|_| bad("use um número de 1 a 10")),
        "token-file" | "token" | "ca-file" if value.trim().is_empty() => {
            bad("o valor não pode ser vazio")
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

/// O nome de um perfil: letras ASCII, dígitos, `-` e `_`, de 1 a 64 caracteres.
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
            "o nome de perfil `{name}` é inválido: use de 1 a 64 letras, dígitos, `-` ou `_`"
        ))
    }
}

/// O arquivo inteiro: o perfil padrão e os perfis, na ordem em que foram criados.
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

    /// Cria um perfil. O primeiro perfil do arquivo vira o padrão. Erro se o nome já existe.
    pub fn add(&mut self, name: &str, profile: Profile) -> Result<(), String> {
        validate_name(name)?;
        if self.get(name).is_some() {
            return Err(format!("o perfil `{name}` já existe (use `profile set`)"));
        }
        self.profiles.push((name.to_owned(), profile));
        if self.profiles.len() == 1 {
            self.default = Some(name.to_owned());
        }
        Ok(())
    }

    /// Remove um perfil; se era o padrão, o arquivo fica sem padrão.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let before = self.profiles.len();
        self.profiles.retain(|(n, _)| n != name);
        if self.profiles.len() == before {
            return Err(format!("o perfil `{name}` não existe"));
        }
        if self.default.as_deref() == Some(name) {
            self.default = None;
        }
        Ok(())
    }

    pub fn set_default(&mut self, name: &str) -> Result<(), String> {
        if self.get(name).is_none() {
            return Err(format!("o perfil `{name}` não existe"));
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
                            return Err(format!("o perfil `{name}` precisa ser um mapa de campos"));
                        };
                        let mut profile = Profile::default();
                        for (field, value) in fields {
                            let Node::Text(value) = value else {
                                return Err(format!("`{name}.{field}` precisa ser um texto"));
                            };
                            profile
                                .set(&field, &value)
                                .map_err(|e| format!("perfil `{name}`: {e}"))?;
                        }
                        file.profiles.push((name, profile));
                    }
                }
                ("default" | "profiles", _) => return Err(format!("`{key}` tem o tipo errado")),
                _ => {
                    return Err(format!(
                        "chave desconhecida `{key}` (use `default` e `profiles`)"
                    ))
                }
            }
        }
        if let Some(default) = &file.default {
            if file.get(default).is_none() {
                return Err(format!(
                    "o perfil padrão `{default}` não existe em `profiles`"
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
            "# Perfis do taba-cli. O arquivo é reescrito por `taba-cli profile`; comentários não são mantidos.\n{}",
            yaml::render(&map)
        )
    }
}

/// O caminho do `config.yml`: `TABA_CLI_CONFIG`, ou a pasta de configuração do usuário no sistema.
pub fn config_path(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = env("TABA_CLI_CONFIG").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let missing = |var: &str| {
        format!("não achei a pasta de configuração do usuário (a variável {var} não existe); use TABA_CLI_CONFIG")
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

/// Lê o arquivo; se não existe, é um arquivo vazio. Ilegível ou inválido: o erro diz o caminho.
pub fn load(path: &Path) -> Result<ConfigFile, String> {
    match fs::read_to_string(path) {
        Ok(text) => ConfigFile::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(format!("não consegui ler {}: {e}", path.display())),
    }
}

/// Reescreve o arquivo inteiro de forma atômica: grava um arquivo temporário ao lado e o renomeia
/// sobre o original. Em Unix o arquivo nasce com permissão `0600`.
pub fn save(path: &Path, file: &ConfigFile) -> Result<(), String> {
    let fail =
        |what: &str, e: std::io::Error| format!("não consegui {what} {}: {e}", path.display());
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| fail("criar a pasta de", e))?;
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
        fail("gravar", e)
    })
}

#[cfg(test)]
mod tests;
