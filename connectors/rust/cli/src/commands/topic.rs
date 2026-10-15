//! Topics (doc 19b, section 18.11.4).

use taba_client::{Nullable, TopicRequest, TopicUpdate};

use super::{call, emit, number, one_of, paged, pairs};
use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Parsed};

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["topic", "create"],
            args: "<name>",
            summary: "Creates a topic (POST /topics). --type is required.",
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
            summary: "Lists the topics (GET /topics); --all walks through all the pages.",
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
            args: "<name>",
            summary: "Shows a topic (GET /topics/{topic}).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![],
            run: get,
        },
        Command {
            path: &["topic", "update"],
            args: "<name>",
            summary: "Changes a topic (PATCH); only what was given is sent.",
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
            args: "<name>",
            summary:
                "Removes a topic and all its messages (asks for confirmation; --yes skips it).",
            min_pos: 1,
            max_pos: 1,
            opts: vec![flag("yes")],
            run: delete,
        },
    ]
}

fn create(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let topic_type = one_of(p, "type", &["queue", "distributor"])?
        .ok_or_else(|| CliError::usage("--type is required (queue or distributor)"))?;
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

/// `<n>` or `default` (which sends `null`).
fn nullable(p: &Parsed, name: &str) -> Result<Nullable<u64>, CliError> {
    match p.value(name) {
        None => Ok(Nullable::Keep),
        Some("default") => Ok(Nullable::Default),
        Some(_) => Ok(number(p, name)?.map_or(Nullable::Keep, Nullable::Set)),
    }
}

fn update(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    if p.flag("clear-tags") && p.has_values("tag") {
        return Err(CliError::usage("--clear-tags and --tag do not combine"));
    }
    if p.flag("clear-prefixes") && p.has_values("prefix") {
        return Err(CliError::usage(
            "--clear-prefixes and --prefix do not combine",
        ));
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
            "nothing to change: give at least one option (--tag, --ttl, ...)",
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
        &format!("Remove the topic `{name}` and all its messages?"),
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
