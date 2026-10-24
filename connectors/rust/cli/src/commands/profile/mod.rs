//! The local commands: `version`, `config show` and `profile ...` (doc 19b, section 18.11.2). None
//! of them calls the server.

use taba_client::json;

use super::{emit, help_text};
use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Opt, Parsed};
use crate::profile::{self, ConfigFile, Profile, FIELDS};
use crate::settings;

/// The options that are profile fields.
const FIELD_OPTS: [Opt; 8] = [
    val("url"),
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
            summary: "Shows the program version and that of the API contract.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| ctx.say(&super::version_text()),
        },
        Command {
            path: &["config", "show"],
            args: "",
            summary: "The effective configuration and where each value came from (without the token).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: config_show,
        },
        Command {
            path: &["profile"],
            args: "",
            summary: "Without a subcommand, opens the interactive profiles screen (only on a terminal).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: interactive,
        },
        Command {
            path: &["profile", "list"],
            args: "",
            summary: "Lists the profiles, marking the default and the one in use in the session.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: list,
        },
        Command {
            path: &["profile", "show"],
            args: "[name]",
            summary: "Shows a profile (the one in use, if no name), without the token.",
            min_pos: 0,
            max_pos: 1,
            opts: vec![],
            run: show,
        },
        Command {
            path: &["profile", "add"],
            args: "<name>",
            summary: "Creates a profile; the first becomes the default, and --default makes another the default.",
            min_pos: 1,
            max_pos: 1,
            opts: ADD_OPTS.to_vec(),
            run: add,
        },
        Command {
            path: &["profile", "set"],
            args: "<name>",
            summary: "Changes the given fields and deletes those in --unset <field> (repeatable).",
            min_pos: 1,
            max_pos: 1,
            opts: SET_OPTS.to_vec(),
            run: set,
        },
        Command {
            path: &["profile", "remove"],
            args: "<name>",
            summary: "Removes a profile (asks for confirmation; --yes skips it).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![flag("yes")],
            run: remove,
        },
        Command {
            path: &["profile", "default"],
            args: "[name]",
            summary: "Without a name shows the default profile; with a name sets it; --none removes it.",
            min_pos: 0,
            max_pos: 1,
            opts: vec![flag("none")],
            run: default,
        },
        Command {
            path: &["profile", "path"],
            args: "",
            summary: "Prints the path of the configuration file.",
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
            args: "[name]",
            summary: "Prints the lines that make the terminal use a profile, or loose fields, for the session only.",
            min_pos: 0,
            max_pos: 1,
            opts: SESSION_OPTS.to_vec(),
            run: session,
        },
    ]
}

const ADD_OPTS: [Opt; 9] = with_extra(flag("default"));
const SET_OPTS: [Opt; 9] = with_extra(val("unset"));
const SESSION_OPTS: [Opt; 10] = {
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
        val("shell"),
    ]
};

const fn with_extra(extra: Opt) -> [Opt; 9] {
    [
        FIELD_OPTS[0],
        FIELD_OPTS[1],
        FIELD_OPTS[2],
        FIELD_OPTS[3],
        FIELD_OPTS[4],
        FIELD_OPTS[5],
        FIELD_OPTS[6],
        FIELD_OPTS[7],
        extra,
    ]
}

/// The profile built from the given field options.
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
        "(set)".to_owned()
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
                CliError::usage("no profile in use; give the name or create one with `profile add`")
            })?,
    };
    let profile = file
        .get(&name)
        .ok_or_else(|| CliError::usage(format!("the profile `{name}` does not exist")))?;
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
    say_done(ctx, &format!("Profile `{name}` created."))
}

fn set(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = &p.positional[0];
    let changes = profile_from(p)?;
    let unset = p.values("unset");
    for field in &unset {
        if !FIELDS.contains(field) {
            return Err(CliError::usage(format!(
                "--unset: `{field}` is not a profile field ({})",
                FIELDS.join(", ")
            )));
        }
    }
    if changes.entries().is_empty() && unset.is_empty() {
        return Err(CliError::usage(
            "nothing to change: give fields (--url, ...) or --unset <field>",
        ));
    }
    let mut file = ctx.load_config()?;
    let profile = file.get_mut(name).ok_or_else(|| {
        CliError::usage(format!(
            "the profile `{name}` does not exist (use `profile add`)"
        ))
    })?;
    for field in unset {
        profile.unset(field);
    }
    for (field, value) in changes.entries() {
        profile.set(field, value).map_err(CliError::Usage)?;
    }
    ctx.save_config(&file)?;
    say_done(ctx, &format!("Profile `{name}` changed."))
}

fn remove(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = &p.positional[0];
    let mut file = ctx.load_config()?;
    if file.get(name).is_none() {
        return Err(CliError::usage(format!(
            "the profile `{name}` does not exist"
        )));
    }
    ctx.require_confirmation(p.flag("yes"), &format!("Remove the profile `{name}`?"))?;
    file.remove(name).map_err(CliError::Usage)?;
    ctx.save_config(&file)?;
    say_done(ctx, &format!("Profile `{name}` removed."))
}

fn default(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut file = ctx.load_config()?;
    if p.flag("none") {
        if !p.positional.is_empty() {
            return Err(CliError::usage("--none does not combine with a name"));
        }
        file.default = None;
        ctx.save_config(&file)?;
        return say_done(ctx, "The file is left with no default profile.");
    }
    match p.positional.first() {
        None => match &file.default {
            Some(name) => {
                let name = name.clone();
                ctx.say(&name)
            }
            None => say_done(ctx, "(none)"),
        },
        Some(name) => {
            file.set_default(name).map_err(CliError::Usage)?;
            ctx.save_config(&file)?;
            say_done(ctx, &format!("`{name}` is the default profile."))
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
// Session profile
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
                "--shell: `{other}` is not valid (use sh, powershell or cmd)"
            )),
        }
    }

    pub fn set_line(self, var: &str, value: &str) -> Result<String, String> {
        Ok(match self {
            Shell::Sh => format!("export {var}='{}'", value.replace('\'', "'\\''")),
            Shell::Ps => format!("$env:{var} = '{}'", value.replace('\'', "''")),
            Shell::Cmd => {
                if value.contains(['"', '\n', '\r']) {
                    return Err(format!("the value of {var} has quotes or a line break, which cmd cannot store safely"));
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

/// The lines of the session profile, without touching the disk or this process's environment.
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
            "--clear does not combine with a name or with fields",
        ));
    }
    if !clear && name.is_none() && fields.entries().is_empty() {
        return Err(CliError::usage(
            "give a profile name, fields (--url, ...) or --clear",
        ));
    }
    if let Some(name) = name {
        let file: ConfigFile = ctx.load_config()?;
        if file.get(name).is_none() {
            return Err(CliError::usage(format!(
                "the profile `{name}` does not exist"
            )));
        }
    }
    let lines = session_lines(shell, name, &fields, clear).map_err(CliError::Usage)?;
    for line in lines {
        ctx.say(&line)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Interactive screen
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
                "the interactive screen needs a terminal; use the subcommands.\n{text}"
            )))
        }
    }
}

#[cfg(test)]
mod tests;
