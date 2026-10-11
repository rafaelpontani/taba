//! Tópicos (doc 19b, seção 18.11.4).

use taba_client::{Nullable, TopicRequest, TopicUpdate};

use super::{call, emit, number, one_of, paged, pairs};
use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Parsed};

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["topic", "create"],
            args: "<nome>",
            summary: "Cria um tópico (POST /topics). --type é obrigatório.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![
                val("type"),
                val("tag"),
                val("prefix"),
                flag("volatile"),
                val("idle-timeout"),
                flag("fifo"),
                val("max-message-bytes"),
                val("ttl"),
                val("max-deliveries"),
            ],
            run: create,
        },
        Command {
            path: &["topic", "list"],
            args: "",
            summary: "Lista os tópicos (GET /topics); --all percorre todas as páginas.",
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
            path: &["topic", "get"],
            args: "<nome>",
            summary: "Mostra um tópico (GET /topics/{topic}).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![],
            run: get,
        },
        Command {
            path: &["topic", "update"],
            args: "<nome>",
            summary: "Altera um tópico (PATCH); só o que foi dado é enviado.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![
                val("tag"),
                flag("clear-tags"),
                val("prefix"),
                flag("clear-prefixes"),
                val("idle-timeout"),
                val("max-message-bytes"),
                val("ttl"),
                val("max-deliveries"),
            ],
            run: update,
        },
        Command {
            path: &["topic", "delete"],
            args: "<nome>",
            summary:
                "Remove um tópico e todas as suas mensagens (pede confirmação; --yes dispensa).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![flag("yes")],
            run: delete,
        },
    ]
}

fn create(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let topic_type = one_of(p, "type", &["queue", "distributor"])?
        .ok_or_else(|| CliError::usage("--type é obrigatório (queue ou distributor)"))?;
    let request = TopicRequest {
        name: p.positional[0].clone(),
        topic_type,
        tags: pairs(p, "tag")?,
        prefixes: p.values("prefix").into_iter().map(str::to_owned).collect(),
        volatile: p.flag("volatile").then_some(true),
        idle_timeout: number(p, "idle-timeout")?,
        fifo: p.flag("fifo").then_some(true),
        max_message_bytes: number(p, "max-message-bytes")?,
        ttl: number(p, "ttl")?,
        max_deliveries: number(p, "max-deliveries")?,
    };
    let client = ctx.api_client()?;
    let mut c = call(ctx, "POST", "/topics").json_body(request.to_json());
    c.recover = vec![(409, "topic.already-exists")];
    let mut reply = client.call(&c)?;
    if reply.recovered {
        reply = client.call(&call(
            ctx,
            "GET",
            format!(
                "/topics/{}",
                taba_client::http::percent_encode(&request.name)
            ),
        ))?;
    }
    emit(ctx, &reply.text())
}

fn list(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut query = Vec::new();
    for (opt, key) in [("name", "name"), ("sort", "sort"), ("limit", "limit")] {
        if let Some(v) = p.value(opt) {
            if opt == "limit" {
                number(p, "limit")?;
            }
            query.push((key.to_owned(), v.to_owned()));
        }
    }
    let client = ctx.api_client()?;
    let text = paged(
        ctx,
        &client,
        "/topics",
        query,
        p.flag("all"),
        p.value("cursor").map(str::to_owned),
    )?;
    emit(ctx, &text)
}

fn get(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let client = ctx.api_client()?;
    let c = call(
        ctx,
        "GET",
        format!(
            "/topics/{}",
            taba_client::http::percent_encode(&p.positional[0])
        ),
    );
    emit(ctx, &client.call(&c)?.text())
}

/// `<n>` ou `default` (que envia `null`).
fn nullable(p: &Parsed, name: &str) -> Result<Nullable<u64>, CliError> {
    match p.value(name) {
        None => Ok(Nullable::Keep),
        Some("default") => Ok(Nullable::Default),
        Some(_) => Ok(number(p, name)?.map_or(Nullable::Keep, Nullable::Set)),
    }
}

fn update(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    if p.flag("clear-tags") && p.has_values("tag") {
        return Err(CliError::usage("--clear-tags e --tag não combinam"));
    }
    if p.flag("clear-prefixes") && p.has_values("prefix") {
        return Err(CliError::usage("--clear-prefixes e --prefix não combinam"));
    }
    let update = TopicUpdate {
        tags: if p.flag("clear-tags") {
            Some(Vec::new())
        } else if p.has_values("tag") {
            Some(pairs(p, "tag")?)
        } else {
            None
        },
        prefixes: if p.flag("clear-prefixes") {
            Some(Vec::new())
        } else if p.has_values("prefix") {
            Some(p.values("prefix").into_iter().map(str::to_owned).collect())
        } else {
            None
        },
        idle_timeout: number(p, "idle-timeout")?,
        max_message_bytes: nullable(p, "max-message-bytes")?,
        ttl: nullable(p, "ttl")?,
        max_deliveries: nullable(p, "max-deliveries")?,
    };
    let body = update.to_json();
    if body == "{}" {
        return Err(CliError::usage(
            "nada para alterar: dê ao menos uma opção (--tag, --ttl, ...)",
        ));
    }
    let client = ctx.api_client()?;
    let c = call(
        ctx,
        "PATCH",
        format!(
            "/topics/{}",
            taba_client::http::percent_encode(&p.positional[0])
        ),
    )
    .json_body(body);
    emit(ctx, &client.call(&c)?.text())
}

fn delete(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let name = &p.positional[0];
    ctx.require_confirmation(
        p.flag("yes"),
        &format!("Remover o tópico `{name}` e todas as suas mensagens?"),
    )?;
    let client = ctx.api_client()?;
    let mut c = call(
        ctx,
        "DELETE",
        format!("/topics/{}", taba_client::http::percent_encode(name)),
    );
    c.recover = vec![(404, "topic.not-found")];
    client.call(&c)?;
    Ok(())
}
