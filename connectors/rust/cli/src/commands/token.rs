//! Tokens de acesso (doc 19b, seção 18.11.6).

use taba_client::json::{self, Fields};

use super::{call, emit, number, paged};
use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Parsed};

const PERMISSIONS: [&str; 4] = ["publish", "consume", "create", "admin"];

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["token", "create"],
            args: "",
            summary: "Cria um token (só o token raiz). O segredo aparece só nesta resposta.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![
                val("name"),
                val("permission"),
                val("prefix"),
                val("expires-at"),
                flag("token-only"),
            ],
            run: create,
        },
        Command {
            path: &["token", "list"],
            args: "",
            summary: "Lista os tokens (nunca mostra o segredo).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![
                val("name"),
                val("sort"),
                val("limit"),
                val("cursor"),
                flag("all"),
            ],
            run: list,
        },
        Command {
            path: &["token", "get"],
            args: "<id>",
            summary: "Mostra um token.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![],
            run: get,
        },
        Command {
            path: &["token", "update"],
            args: "<id>",
            summary:
                "Altera o nome ou o vencimento de um token (--expires-at none remove o prazo).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![val("name"), val("expires-at")],
            run: update,
        },
        Command {
            path: &["token", "revoke"],
            args: "<id>",
            summary: "Revoga um token (pede confirmação; --yes dispensa).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![flag("yes")],
            run: revoke,
        },
    ]
}

fn token_path(id: &str) -> String {
    format!("/tokens/{}", taba_client::http::percent_encode(id))
}

fn strings(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| json::quote(s)).collect();
    format!("[{}]", quoted.join(","))
}

fn create(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = p
        .value("name")
        .ok_or_else(|| CliError::usage("--name é obrigatório"))?;
    let permissions: Vec<String> = p
        .values("permission")
        .into_iter()
        .flat_map(|v| v.split(','))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    if permissions.is_empty() {
        return Err(CliError::usage(
            "--permission é obrigatório (publish, consume, create, admin)",
        ));
    }
    if let Some(bad) = permissions
        .iter()
        .find(|s| !PERMISSIONS.contains(&s.as_str()))
    {
        return Err(CliError::usage(format!(
            "--permission: `{bad}` não vale (use {})",
            PERMISSIONS.join(", ")
        )));
    }
    let mut fields = vec![
        format!("\"name\":{}", json::quote(name)),
        format!("\"permissions\":{}", strings(&permissions)),
    ];
    let prefixes: Vec<String> = p.values("prefix").into_iter().map(str::to_owned).collect();
    if !prefixes.is_empty() {
        fields.push(format!("\"topicPrefixes\":{}", strings(&prefixes)));
    }
    if let Some(at) = p.value("expires-at") {
        fields.push(format!("\"expiresAt\":{}", json::quote(at)));
    }
    let client = ctx.api_client()?;
    let c = call(ctx, "POST", "/tokens").json_body(format!("{{{}}}", fields.join(",")));
    let text = client.call(&c)?.text();
    if p.flag("token-only") {
        let secret = Fields::parse(&text).ok().and_then(|f| f.string("token"));
        return match secret {
            Some(secret) if !ctx.globals.quiet => ctx.say(&secret),
            Some(_) => Ok(()),
            None => Err(CliError::File(
                "a resposta não trouxe o campo token".to_owned(),
            )),
        };
    }
    emit(ctx, &text)
}

fn list(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut query = Vec::new();
    for key in ["name", "sort", "limit"] {
        if let Some(v) = p.value(key) {
            if key == "limit" {
                number(p, "limit")?;
            }
            query.push((key.to_owned(), v.to_owned()));
        }
    }
    let client = ctx.api_client()?;
    let text = paged(
        ctx,
        &client,
        "/tokens",
        query,
        p.flag("all"),
        p.value("cursor").map(str::to_owned),
    )?;
    emit(ctx, &text)
}

fn get(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let client = ctx.api_client()?;
    let c = call(ctx, "GET", token_path(&p.positional[0]));
    emit(ctx, &client.call(&c)?.text())
}

fn update(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut fields = Vec::new();
    if let Some(name) = p.value("name") {
        fields.push(format!("\"name\":{}", json::quote(name)));
    }
    match p.value("expires-at") {
        None => {}
        Some("none") => fields.push("\"expiresAt\":null".to_owned()),
        Some(at) => fields.push(format!("\"expiresAt\":{}", json::quote(at))),
    }
    if fields.is_empty() {
        return Err(CliError::usage(
            "nada para alterar: dê --name ou --expires-at",
        ));
    }
    let client = ctx.api_client()?;
    let c = call(ctx, "PATCH", token_path(&p.positional[0]))
        .json_body(format!("{{{}}}", fields.join(",")));
    emit(ctx, &client.call(&c)?.text())
}

fn revoke(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    ctx.require_confirmation(
        p.flag("yes"),
        &format!("Revogar o token `{}`?", p.positional[0]),
    )?;
    let client = ctx.api_client()?;
    client.call(&call(ctx, "DELETE", token_path(&p.positional[0])))?;
    Ok(())
}
