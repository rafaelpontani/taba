//! Leitura própria dos argumentos (sem biblioteca de linhas de comando), guiada pela tabela de
//! opções de cada comando.

#[derive(Debug, Clone, Copy)]
pub struct Opt {
    pub name: &'static str,
    /// `true` se a opção recebe um valor; `false` é uma chave liga/desliga.
    pub takes_value: bool,
    pub short: Option<char>,
}

pub const fn val(name: &'static str) -> Opt {
    Opt {
        name,
        takes_value: true,
        short: None,
    }
}

pub const fn flag(name: &'static str) -> Opt {
    Opt {
        name,
        takes_value: false,
        short: None,
    }
}

/// As opções que todo comando aceita.
pub const GLOBAL: [Opt; 6] = [
    val("profile"),
    val("request-id"),
    Opt {
        name: "output",
        takes_value: true,
        short: Some('o'),
    },
    Opt {
        name: "quiet",
        takes_value: false,
        short: Some('q'),
    },
    flag("help"),
    flag("version"),
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub positional: Vec<String>,
    values: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Parsed {
    /// O último valor dado à opção.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Todos os valores da opção, na ordem em que vieram.
    pub fn values(&self, name: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn has_values(&self, name: &str) -> bool {
        self.values.iter().any(|(n, _)| n == name)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    /// O valor da opção como inteiro sem sinal.
    pub fn number(&self, name: &str) -> Result<Option<u64>, String> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => v
                .parse::<u64>()
                .map(Some)
                .map_err(|_| format!("--{name}: `{v}` não é um número inteiro")),
        }
    }

    pub fn has_any(&self) -> bool {
        !self.values.is_empty() || !self.flags.is_empty()
    }
}

/// Lê `args` (sem o nome do programa nem as palavras do comando) segundo as opções do comando mais
/// as globais. `--opção=valor`, `--opção valor`, `-o valor`, `-ovalor` e `--` (o resto é posicional).
pub fn parse(args: &[String], opts: &[Opt]) -> Result<Parsed, String> {
    let find_long = |name: &str| opts.iter().chain(GLOBAL.iter()).find(|o| o.name == name);
    let find_short = |c: char| {
        opts.iter()
            .chain(GLOBAL.iter())
            .find(|o| o.short == Some(c))
    };
    let mut parsed = Parsed::default();
    let mut i = 0;
    let mut only_positional = false;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if only_positional || arg == "-" || !arg.starts_with('-') {
            parsed.positional.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_positional = true;
            continue;
        }
        let (opt, inline) = if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_owned())),
                None => (long, None),
            };
            (
                find_long(name).ok_or_else(|| format!("opção desconhecida: --{name}"))?,
                inline,
            )
        } else {
            let mut chars = arg[1..].chars();
            let c = chars.next().unwrap_or('-');
            let rest: String = chars.collect();
            let opt = find_short(c).ok_or_else(|| format!("opção desconhecida: -{c}"))?;
            (opt, if rest.is_empty() { None } else { Some(rest) })
        };
        if opt.takes_value {
            let value = match inline {
                Some(v) => v,
                None => {
                    let v = args
                        .get(i)
                        .ok_or_else(|| format!("--{} precisa de um valor", opt.name))?;
                    i += 1;
                    v.clone()
                }
            };
            parsed.values.push((opt.name.to_owned(), value));
        } else {
            if inline
                .as_ref()
                .is_some_and(|v| arg.starts_with("--") || !v.is_empty())
            {
                return Err(format!("--{} não recebe valor", opt.name));
            }
            parsed.flags.push(opt.name.to_owned());
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests;
