//! A tabela de comandos: o que cada um aceita, a ajuda e a escolha do comando pelas palavras dos
//! argumentos.

use crate::app::Command;
use crate::args::GLOBAL;

mod message;
mod ops;
mod profile;
mod token;
mod topic;

/// A versão do programa e a do contrato da API que ele implementa.
pub fn version_text() -> String {
    format!(
        "taba-cli {} (contrato da API {})",
        env!("CARGO_PKG_VERSION"),
        taba_client::CONTRACT_VERSION
    )
}

pub fn table() -> Vec<Command> {
    let mut all = Vec::new();
    all.extend(topic::commands());
    all.extend(message::commands());
    all.extend(token::commands());
    all.extend(ops::commands());
    all.extend(profile::commands());
    all
}

pub enum Invocation<'a> {
    /// `help [comando]` ou `--help` sem comando: o texto de ajuda do assunto (vazio = geral).
    Help(String),
    Version,
    /// O comando e os argumentos que sobraram (sem as palavras do comando).
    Run(&'a Command, Vec<String>),
}

/// Acha o comando pelas primeiras palavras. As opções globais com valor (`--profile x`, `-o x`)
/// podem vir antes: os valores delas não contam como palavras.
pub fn resolve<'a>(table: &'a [Command], args: &[String]) -> Result<Invocation<'a>, String> {
    let mut words: Vec<(usize, &str)> = Vec::new();
    let mut i = 0;
    while i < args.len() && words.len() < 2 {
        let arg = args[i].as_str();
        if arg == "--" {
            break;
        }
        if let Some(rest) = arg.strip_prefix('-').filter(|r| !r.is_empty()) {
            let long = rest.strip_prefix('-');
            let takes = GLOBAL.iter().any(|o| {
                o.takes_value
                    && match long {
                        Some(name) => o.name == name,
                        None => rest.len() == 1 && o.short.is_some_and(|s| rest.starts_with(s)),
                    }
            });
            i += if takes { 2 } else { 1 };
            continue;
        }
        words.push((i, arg));
        i += 1;
    }
    let wants_help = args.iter().any(|a| a == "--help");
    if words.is_empty() {
        if args.iter().any(|a| a == "--version") {
            return Ok(Invocation::Version);
        }
        return if args.is_empty() || wants_help {
            Ok(Invocation::Help(String::new()))
        } else {
            Err("faltou o comando".to_owned())
        };
    }
    if words[0].1 == "help" {
        let topic: Vec<&str> = args
            .iter()
            .map(String::as_str)
            .filter(|a| !a.starts_with('-'))
            .skip(1)
            .collect();
        return Ok(Invocation::Help(topic.join(" ")));
    }
    let find = |path: &[&str]| table.iter().find(|c| c.path == path);
    let known_group = table
        .iter()
        .any(|c| c.path.len() == 2 && c.path[0] == words[0].1);
    let (command, used): (&Command, Vec<usize>) =
        if let Some(second) = words.get(1).filter(|_| known_group) {
            match find(&[words[0].1, second.1]) {
                Some(c) => (c, vec![words[0].0, second.0]),
                None => {
                    let subs: Vec<&str> = table
                        .iter()
                        .filter(|c| c.path.len() == 2 && c.path[0] == words[0].1)
                        .map(|c| c.path[1])
                        .collect();
                    return Err(format!(
                        "`{} {}` não é um comando ({} {})",
                        words[0].1,
                        second.1,
                        words[0].1,
                        subs.join(" | ")
                    ));
                }
            }
        } else if let Some(c) = find(&[words[0].1]) {
            (c, vec![words[0].0])
        } else if known_group {
            if wants_help {
                return Ok(Invocation::Help(words[0].1.to_owned()));
            }
            let subs: Vec<&str> = table
                .iter()
                .filter(|c| c.path.len() == 2 && c.path[0] == words[0].1)
                .map(|c| c.path[1])
                .collect();
            return Err(format!(
                "faltou o subcomando de `{}` ({})",
                words[0].1,
                subs.join(" | ")
            ));
        } else {
            return Err(format!("`{}` não é um comando", words[0].1));
        };
    let rest: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(n, _)| !used.contains(n))
        .map(|(_, a)| a.clone())
        .collect();
    Ok(Invocation::Run(command, rest))
}

pub fn usage_line(c: &Command) -> String {
    let mut line = format!("uso: taba-cli {}", c.path.join(" "));
    if !c.args.is_empty() {
        line.push(' ');
        line.push_str(c.args);
    }
    if !c.opts.is_empty() {
        line.push_str(" [opções]");
    }
    line
}

pub fn help_text(table: &[Command], topic: &str) -> String {
    let words: Vec<&str> = topic.split_whitespace().collect();
    let matching: Vec<&Command> = table
        .iter()
        .filter(|c| {
            !words.is_empty() && c.path.len() >= words.len() && c.path[..words.len()] == words[..]
        })
        .collect();
    let mut out = String::new();
    if words.is_empty() {
        out.push_str(&format!(
            "{}\n\nuso: taba-cli <comando> [argumentos] [opções]\n\nComandos:\n",
            version_text()
        ));
        for c in table {
            out.push_str(&format!(
                "  {:<28} {}\n",
                format!("{} {}", c.path.join(" "), c.args).trim_end(),
                c.summary
            ));
        }
        out.push_str("  help [comando]               Mostra a ajuda de um comando.\n  version                      Mostra a versão do programa e a do contrato.\n");
        out.push_str("\nOpções de todos os comandos:\n  --profile <nome>    Perfil de conexão (padrão: o perfil padrão do arquivo)\n  -o, --output <fmt>  json, table ou raw\n  -q, --quiet         Não imprime nada em caso de sucesso\n  --request-id <id>   Vai em X-Request-Id\n  --help, --version\n");
        return out;
    }
    if matching.is_empty() {
        return format!("Não há ajuda para `{topic}`. Use `taba-cli help`.\n");
    }
    for c in matching {
        out.push_str(&format!("{}\n  {}\n", usage_line(c), c.summary));
        let mut opts: Vec<String> = c
            .opts
            .iter()
            .map(|o| {
                if o.takes_value {
                    format!("--{} <valor>", o.name)
                } else {
                    format!("--{}", o.name)
                }
            })
            .collect();
        opts.extend([
            "--profile <nome>".into(),
            "-o <formato>".into(),
            "-q".into(),
            "--request-id <id>".into(),
        ]);
        out.push_str(&format!("  opções: {}\n\n", opts.join(", ")));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Ajudantes dos comandos
// ---------------------------------------------------------------------------------------------

use crate::app::{CliError, Ctx};
use crate::args::Parsed;
use crate::output;
use taba_client::json::{self, Fields};
use taba_client::{Call, Client, Error as ApiError};

/// Uma chamada nova, já com o `--request-id`.
pub(crate) fn call(ctx: &Ctx<'_>, method: &'static str, path: impl Into<String>) -> Call {
    let mut call = Call::new(method, path);
    call.request_id = ctx.globals.request_id.clone();
    call
}

/// Imprime o texto de uma resposta no formato escolhido (nada com `--quiet`).
pub(crate) fn emit(ctx: &mut Ctx<'_>, text: &str) -> Result<(), CliError> {
    if ctx.globals.quiet {
        return Ok(());
    }
    let format = ctx.format()?;
    let rendered = output::render(text, format);
    ctx.say(&rendered)
}

pub(crate) fn number(p: &Parsed, name: &str) -> Result<Option<u64>, CliError> {
    p.number(name).map_err(CliError::Usage)
}

/// Os pares `chave=valor` de uma opção repetível.
pub(crate) fn pairs(p: &Parsed, name: &str) -> Result<Vec<(String, String)>, CliError> {
    p.values(name)
        .into_iter()
        .map(|raw| match raw.split_once('=') {
            Some((k, v)) if !k.is_empty() => Ok((k.to_owned(), v.to_owned())),
            _ => Err(CliError::Usage(format!(
                "--{name}: `{raw}` precisa ter a forma chave=valor"
            ))),
        })
        .collect()
}

pub(crate) fn one_of(p: &Parsed, name: &str, allowed: &[&str]) -> Result<Option<String>, CliError> {
    match p.value(name) {
        None => Ok(None),
        Some(v) if allowed.contains(&v) => Ok(Some(v.to_owned())),
        Some(v) => Err(CliError::Usage(format!(
            "--{name}: `{v}` não vale (use {})",
            allowed.join(" ou ")
        ))),
    }
}

/// Uma lista paginada: uma página como o servidor devolveu, ou, com `all`, todas as páginas juntas
/// num só `{"items":[...]}`. Devolve o texto JSON.
pub(crate) fn paged(
    ctx: &Ctx<'_>,
    client: &Client,
    path: &str,
    query: Vec<(String, String)>,
    all: bool,
    first_cursor: Option<String>,
) -> Result<String, CliError> {
    let mut cursor = first_cursor;
    let mut collected: Vec<String> = Vec::new();
    loop {
        let mut c = call(ctx, "GET", path);
        c.query = query.clone();
        if let Some(cursor) = &cursor {
            c.query.retain(|(k, _)| k != "cursor");
            c.query.push(("cursor".to_owned(), cursor.clone()));
        }
        let text = client.call(&c)?.text();
        if !all {
            return Ok(text);
        }
        let invalid =
            |m: String| CliError::Api(ApiError::from_message("client.invalid-response", m));
        let fields = Fields::parse(&text).map_err(|e| invalid(format!("página inválida: {e}")))?;
        let items = json::array_items(fields.raw("items").unwrap_or("[]"))
            .map_err(|e| invalid(format!("itens inválidos: {e}")))?;
        collected.extend(items.into_iter().map(str::to_owned));
        match fields.string("nextCursor") {
            Some(next) => cursor = Some(next),
            None => return Ok(format!("{{\"items\":[{}]}}", collected.join(","))),
        }
    }
}
