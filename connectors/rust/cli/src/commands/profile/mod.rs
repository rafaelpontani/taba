//! Os comandos locais: `version`, `config show` e `profile ...` (doc 19b, seção 18.11.2). Nenhum
//! deles chama o servidor.

use taba_client::json;

use super::{emit, help_text};
use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Opt, Parsed};
use crate::profile::{self, ConfigFile, Profile, FIELDS};
use crate::settings;

/// As opções que são campos de perfil.
const FIELD_OPTS: [Opt; 9] = [
    val("url"),
    val("ops-url"),
    val("token-file"),
    val("token"),
    val("ca-file"),
    val("output"),
    val("connect-timeout"),
    val("timeout"),
    val("max-attempts"),
];

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["version"],
            args: "",
            summary: "Mostra a versão do programa e a do contrato da API.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| ctx.say(&super::version_text()),
        },
        Command {
            path: &["config", "show"],
            args: "",
            summary: "A configuração efetiva e de onde veio cada valor (sem o token).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: config_show,
        },
        Command {
            path: &["profile"],
            args: "",
            summary: "Sem subcomando, abre a tela interativa dos perfis (só num terminal).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: interactive,
        },
        Command {
            path: &["profile", "list"],
            args: "",
            summary: "Lista os perfis, marcando o padrão e o que está em uso na sessão.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: list,
        },
        Command {
            path: &["profile", "show"],
            args: "[nome]",
            summary: "Mostra um perfil (o em uso, se sem nome), sem o token.",
            min_pos: 0,
            max_pos: 1,
            opts: vec![],
            run: show,
        },
        Command {
            path: &["profile", "add"],
            args: "<nome>",
            summary: "Cria um perfil; o primeiro vira o padrão, e --default torna outro o padrão.",
            min_pos: 1,
            max_pos: 1,
            opts: ADD_OPTS.to_vec(),
            run: add,
        },
        Command {
            path: &["profile", "set"],
            args: "<nome>",
            summary: "Altera os campos dados e apaga os de --unset <campo> (repetível).",
            min_pos: 1,
            max_pos: 1,
            opts: SET_OPTS.to_vec(),
            run: set,
        },
        Command {
            path: &["profile", "remove"],
            args: "<nome>",
            summary: "Remove um perfil (pede confirmação; --yes dispensa).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![flag("yes")],
            run: remove,
        },
        Command {
            path: &["profile", "default"],
            args: "[nome]",
            summary: "Sem nome mostra o perfil padrão; com nome o define; --none o remove.",
            min_pos: 0,
            max_pos: 1,
            opts: vec![flag("none")],
            run: default,
        },
        Command {
            path: &["profile", "path"],
            args: "",
            summary: "Imprime o caminho do arquivo de configuração.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| {
                let path = ctx.config_path()?;
                ctx.say(&path.display().to_string())
            },
        },
        Command {
            path: &["profile", "session"],
            args: "[nome]",
            summary: "Imprime as linhas que fazem o terminal usar um perfil, ou campos soltos, só na sessão.",
            min_pos: 0,
            max_pos: 1,
            opts: SESSION_OPTS.to_vec(),
            run: session,
        },
    ]
}

const ADD_OPTS: [Opt; 10] = with_extra(flag("default"));
const SET_OPTS: [Opt; 10] = with_extra(val("unset"));
const SESSION_OPTS: [Opt; 11] = {
    let base = with_extra(flag("clear"));
    [
        base[0],
        base[1],
        base[2],
        base[3],
        base[4],
        base[5],
        base[6],
        base[7],
        base[8],
        base[9],
        val("shell"),
    ]
};

const fn with_extra(extra: Opt) -> [Opt; 10] {
    [
        FIELD_OPTS[0],
        FIELD_OPTS[1],
        FIELD_OPTS[2],
        FIELD_OPTS[3],
        FIELD_OPTS[4],
        FIELD_OPTS[5],
        FIELD_OPTS[6],
        FIELD_OPTS[7],
        FIELD_OPTS[8],
        extra,
    ]
}

/// O perfil montado com as opções de campo dadas.
fn profile_from(p: &Parsed) -> Result<Profile, CliError> {
    let mut profile = Profile::default();
    for field in FIELDS {
        if let Some(value) = p.value(field) {
            profile.set(field, value).map_err(CliError::Usage)?;
        }
    }
    Ok(profile)
}

fn mask(field: &str, value: &str) -> String {
    if field == "token" {
        "(definido)".to_owned()
    } else {
        value.to_owned()
    }
}

fn profile_json(name: &str, is_default: bool, profile: &Profile) -> String {
    let mut fields = vec![
        format!("\"name\":{}", json::quote(name)),
        format!("\"default\":{is_default}"),
    ];
    for (k, v) in profile.entries() {
        fields.push(format!("{}:{}", json::quote(k), json::quote(&mask(k, v))));
    }
    format!("{{{}}}", fields.join(","))
}

fn list(ctx: &mut Ctx<'_>, _: &Parsed) -> Result<(), CliError> {
    let file = ctx.load_config()?;
    let in_session = ctx.env("TABA_CLI_PROFILE").filter(|v| !v.is_empty());
    let items: Vec<String> = file
        .profiles
        .iter()
        .map(|(name, profile)| {
            let mut item = profile_json(name, file.default.as_deref() == Some(name), profile);
            if in_session.as_deref() == Some(name) {
                item.pop();
                item.push_str(",\"session\":true}");
            }
            item
        })
        .collect();
    emit(ctx, &format!("{{\"items\":[{}]}}", items.join(",")))
}

fn show(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let file = ctx.load_config()?;
    let name = match p.positional.first() {
        Some(name) => name.clone(),
        None => ctx
            .settings()?
            .profile
            .as_ref()
            .map(|(n, _)| n.clone())
            .ok_or_else(|| {
                CliError::usage("nenhum perfil em uso; dê o nome ou crie um com `profile add`")
            })?,
    };
    let profile = file
        .get(&name)
        .ok_or_else(|| CliError::usage(format!("o perfil `{name}` não existe")))?;
    let text = profile_json(
        &name,
        file.default.as_deref() == Some(name.as_str()),
        profile,
    );
    emit(ctx, &text)
}

fn say_done(ctx: &mut Ctx<'_>, message: &str) -> Result<(), CliError> {
    if ctx.globals.quiet {
        Ok(())
    } else {
        ctx.say(message)
    }
}

fn add(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = p.positional[0].clone();
    let profile = profile_from(p)?;
    let mut file = ctx.load_config()?;
    file.add(&name, profile).map_err(CliError::Usage)?;
    if p.flag("default") {
        file.set_default(&name).map_err(CliError::Usage)?;
    }
    ctx.save_config(&file)?;
    say_done(ctx, &format!("Perfil `{name}` criado."))
}

fn set(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = &p.positional[0];
    let changes = profile_from(p)?;
    let unset = p.values("unset");
    for field in &unset {
        if !FIELDS.contains(field) {
            return Err(CliError::usage(format!(
                "--unset: `{field}` não é um campo de perfil ({})",
                FIELDS.join(", ")
            )));
        }
    }
    if changes.entries().is_empty() && unset.is_empty() {
        return Err(CliError::usage(
            "nada para alterar: dê campos (--url, ...) ou --unset <campo>",
        ));
    }
    let mut file = ctx.load_config()?;
    let profile = file.get_mut(name).ok_or_else(|| {
        CliError::usage(format!("o perfil `{name}` não existe (use `profile add`)"))
    })?;
    for field in unset {
        profile.unset(field);
    }
    for (field, value) in changes.entries() {
        profile.set(field, value).map_err(CliError::Usage)?;
    }
    ctx.save_config(&file)?;
    say_done(ctx, &format!("Perfil `{name}` alterado."))
}

fn remove(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = &p.positional[0];
    let mut file = ctx.load_config()?;
    if file.get(name).is_none() {
        return Err(CliError::usage(format!("o perfil `{name}` não existe")));
    }
    ctx.require_confirmation(p.flag("yes"), &format!("Remover o perfil `{name}`?"))?;
    file.remove(name).map_err(CliError::Usage)?;
    ctx.save_config(&file)?;
    say_done(ctx, &format!("Perfil `{name}` removido."))
}

fn default(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut file = ctx.load_config()?;
    if p.flag("none") {
        if !p.positional.is_empty() {
            return Err(CliError::usage("--none não combina com um nome"));
        }
        file.default = None;
        ctx.save_config(&file)?;
        return say_done(ctx, "O arquivo ficou sem perfil padrão.");
    }
    match p.positional.first() {
        None => match &file.default {
            Some(name) => {
                let name = name.clone();
                ctx.say(&name)
            }
            None => say_done(ctx, "(nenhum)"),
        },
        Some(name) => {
            file.set_default(name).map_err(CliError::Usage)?;
            ctx.save_config(&file)?;
            say_done(ctx, &format!("`{name}` é o perfil padrão."))
        }
    }
}

fn config_show(ctx: &mut Ctx<'_>, _: &Parsed) -> Result<(), CliError> {
    let effective = ctx.settings()?;
    let mut items = Vec::new();
    for (field, setting) in effective.settings() {
        let value = match &setting.value {
            Some(v) => json::quote(&mask(field, v)),
            None => "null".to_owned(),
        };
        items.push(format!(
            "{{\"field\":{},\"value\":{value},\"source\":{}}}",
            json::quote(field),
            json::quote(&setting.source.label())
        ));
    }
    let name = effective
        .profile
        .as_ref()
        .map(|(n, s)| (n.clone(), s.label()));
    let head = match name {
        Some((n, why)) => format!(
            "\"profile\":{},\"profileSource\":{}",
            json::quote(&n),
            json::quote(&why)
        ),
        None => "\"profile\":null".to_owned(),
    };
    let text = format!("{{{head},\"items\":[{}]}}", items.join(","));
    emit(ctx, &text)
}

// ---------------------------------------------------------------------------------------------
// Perfil de sessão
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Sh,
    Ps,
    Cmd,
}

impl Shell {
    pub fn parse(name: &str) -> Result<Shell, String> {
        match name {
            "sh" => Ok(Shell::Sh),
            "powershell" => Ok(Shell::Ps),
            "cmd" => Ok(Shell::Cmd),
            other => Err(format!(
                "--shell: `{other}` não vale (use sh, powershell ou cmd)"
            )),
        }
    }

    pub fn set_line(self, var: &str, value: &str) -> Result<String, String> {
        Ok(match self {
            Shell::Sh => format!("export {var}='{}'", value.replace('\'', "'\\''")),
            Shell::Ps => format!("$env:{var} = '{}'", value.replace('\'', "''")),
            Shell::Cmd => {
                if value.contains(['"', '\n', '\r']) {
                    return Err(format!("o valor de {var} tem aspas ou quebra de linha, que o cmd não consegue guardar com segurança"));
                }
                format!("set \"{var}={value}\"")
            }
        })
    }

    pub fn unset_line(self, var: &str) -> String {
        match self {
            Shell::Sh => format!("unset {var}"),
            Shell::Ps => format!("Remove-Item Env:{var} -ErrorAction SilentlyContinue"),
            Shell::Cmd => format!("set \"{var}=\""),
        }
    }
}

/// As linhas do perfil de sessão, sem tocar em disco nem no ambiente deste processo.
pub fn session_lines(
    shell: Shell,
    name: Option<&str>,
    fields: &Profile,
    clear: bool,
) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    if clear {
        lines.push(shell.unset_line("TABA_CLI_PROFILE"));
        for field in FIELDS {
            lines.push(shell.unset_line(&settings::variable_name(field)));
        }
        return Ok(lines);
    }
    if let Some(name) = name {
        lines.push(shell.set_line("TABA_CLI_PROFILE", name)?);
    }
    for (field, value) in fields.entries() {
        lines.push(shell.set_line(&settings::variable_name(field), value)?);
    }
    Ok(lines)
}

fn session(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let shell = match p.value("shell") {
        Some(name) => Shell::parse(name).map_err(CliError::Usage)?,
        None if ctx.platform.os() == profile::Os::Windows => Shell::Ps,
        None => Shell::Sh,
    };
    let clear = p.flag("clear");
    let name = p.positional.first().map(String::as_str);
    let fields = profile_from(p)?;
    if clear && (name.is_some() || !fields.entries().is_empty()) {
        return Err(CliError::usage(
            "--clear não combina com um nome nem com campos",
        ));
    }
    if !clear && name.is_none() && fields.entries().is_empty() {
        return Err(CliError::usage(
            "dê um nome de perfil, campos (--url, ...) ou --clear",
        ));
    }
    if let Some(name) = name {
        let file: ConfigFile = ctx.load_config()?;
        if file.get(name).is_none() {
            return Err(CliError::usage(format!("o perfil `{name}` não existe")));
        }
    }
    let lines = session_lines(shell, name, &fields, clear).map_err(CliError::Usage)?;
    for line in lines {
        ctx.say(&line)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Tela interativa
// ---------------------------------------------------------------------------------------------

fn interactive(ctx: &mut Ctx<'_>, _: &Parsed) -> Result<(), CliError> {
    let terminal = if ctx.platform.stdin_is_tty() && ctx.platform.stdout_is_tty() {
        ctx.platform.terminal()
    } else {
        None
    };
    match terminal {
        Some(mut terminal) => crate::tui::run(ctx, terminal.as_mut()),
        None => {
            let all = super::table();
            let text = help_text(&all, "profile");
            Err(CliError::Usage(format!(
                "a tela interativa precisa de um terminal; use os subcomandos.\n{text}"
            )))
        }
    }
}

#[cfg(test)]
mod tests;
