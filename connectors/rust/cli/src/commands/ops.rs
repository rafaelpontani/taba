//! The operations API: health, metrics, statistics and drain (doc 19b, section 18.11.6). These routes
//! carry no token.

use super::{call, emit, number, one_of, paged, pairs};
use crate::app::{CliError, Command, Ctx, EXIT_API};
use crate::args::{flag, val, Parsed};
use crate::output::{self, Format};

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["health", "live"],
            args: "",
            summary: "Is the process up? Exits 0 if the node answered 200, and 1 if not.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| health(ctx, "/health/live", Vec::new()),
        },
        Command {
            path: &["health", "ready"],
            args: "",
            summary: "Is the node ready? With 503 it shows the reason.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| health(ctx, "/health/ready", Vec::new()),
        },
        Command {
            path: &["health", "cluster"],
            args: "",
            summary: "Cluster state and conditions; --scope cluster|node.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![val("scope")],
            run: |ctx, p| {
                let query = one_of(p, "scope", &["cluster", "node"])?
                    .map(|s| vec![("scope".to_owned(), s)])
                    .unwrap_or_default();
                health(ctx, "/health/cluster", query)
            },
        },
        Command {
            path: &["metrics"],
            args: "",
            summary:
                "This node's Prometheus metrics; --openmetrics asks for the OpenMetrics format.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![flag("openmetrics")],
            run: metrics,
        },
        Command {
            path: &["stats", "topics"],
            args: "",
            summary:
                "Topic metrics: --scope, --tag k=v, --condition, --fields, --group-by, pagination.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![
                val("scope"),
                val("tag"),
                val("condition"),
                val("fields"),
                val("group-by"),
                val("limit"),
                val("cursor"),
                flag("all"),
            ],
            run: stats_topics,
        },
        Command {
            path: &["stats", "topic"],
            args: "<name>",
            summary: "Metrics and condition of a topic; --scope cluster|node.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![val("scope")],
            run: stats_topic,
        },
        Command {
            path: &["stats", "groups"],
            args: "<topic>",
            summary: "Metrics per group of a topic.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![val("limit"), val("cursor"), flag("all")],
            run: stats_groups,
        },
        Command {
            path: &["drain", "start"],
            args: "",
            summary: "Puts the profile's ops-url node into drain.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| drain(ctx, "POST"),
        },
        Command {
            path: &["drain", "status"],
            args: "",
            summary: "The node's drain progress.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| drain(ctx, "GET"),
        },
        Command {
            path: &["drain", "cancel"],
            args: "",
            summary:
                "Cancels the drain, or makes a node that already left come back as a new node.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| drain(ctx, "DELETE"),
        },
    ]
}

/// Health: the response body belongs to the endpoint itself, not an `ErrorResponse`, so a `503` is also
/// printed (on standard output) and the exit code is 1. Without retrying: the orchestrator asks
/// again by itself.
fn health(ctx: &mut Ctx<'_>, path: &str, query: Vec<(String, String)>) -> Result<(), CliError> {
    let client = ctx.ops_client(Some(1))?;
    let mut c = call(ctx, "GET", path);
    c.query = query;
    match client.call(&c) {
        Ok(reply) => {
            let text = reply.text();
            if path == "/health/cluster" && !ctx.globals.quiet && ctx.format()? == Format::Table {
                if let Some(table) = output::cluster_table(&text) {
                    return ctx.say(&table);
                }
            }
            emit(ctx, &text)
        }
        Err(e) if e.status > 0 => {
            let body = String::from_utf8_lossy(&e.body).into_owned();
            if !body.trim().is_empty() && !ctx.globals.quiet {
                let format = ctx.format()?;
                let text = output::render(&body, format);
                ctx.say(&text)?;
            }
            Err(CliError::Exit(EXIT_API))
        }
        Err(e) => Err(e.into()),
    }
}

fn metrics(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let client = ctx.ops_client(None)?;
    let mut c = call(ctx, "GET", "/metrics");
    let accept = if p.flag("openmetrics") {
        "application/openmetrics-text"
    } else {
        "text/plain"
    };
    c.headers.push(("Accept".to_owned(), accept.to_owned()));
    let text = client.call(&c)?.text();
    // Metrics are not JSON: they come out as they came, whatever the format.
    if !ctx.globals.quiet {
        ctx.say(text.trim_end_matches('\n'))?;
    }
    Ok(())
}

fn stats_topics(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut query = Vec::new();
    if let Some(scope) = one_of(p, "scope", &["cluster", "node"])? {
        query.push(("scope".to_owned(), scope));
    }
    for (k, v) in pairs(p, "tag")? {
        query.push((format!("tag.{k}"), v));
    }
    for (opt, key) in [
        ("condition", "condition"),
        ("fields", "fields"),
        ("group-by", "groupBy"),
        ("limit", "limit"),
    ] {
        if let Some(v) = p.value(opt) {
            if opt == "limit" {
                number(p, "limit")?;
            }
            query.push((key.to_owned(), v.to_owned()));
        }
    }
    let client = ctx.ops_client(None)?;
    let text = paged(
        ctx,
        &client,
        "/metrics/topics",
        query,
        p.flag("all"),
        p.value("cursor").map(str::to_owned),
    )?;
    emit(ctx, &text)
}

fn stats_topic(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut c = call(
        ctx,
        "GET",
        format!(
            "/metrics/topics/{}",
            taba_client::http::percent_encode(&p.positional[0])
        ),
    );
    if let Some(scope) = one_of(p, "scope", &["cluster", "node"])? {
        c.query.push(("scope".to_owned(), scope));
    }
    let client = ctx.ops_client(None)?;
    emit(ctx, &client.call(&c)?.text())
}

fn stats_groups(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut query = Vec::new();
    if let Some(v) = p.value("limit") {
        number(p, "limit")?;
        query.push(("limit".to_owned(), v.to_owned()));
    }
    let client = ctx.ops_client(None)?;
    let path = format!(
        "/metrics/topics/{}/groups",
        taba_client::http::percent_encode(&p.positional[0])
    );
    let text = paged(
        ctx,
        &client,
        &path,
        query,
        p.flag("all"),
        p.value("cursor").map(str::to_owned),
    )?;
    emit(ctx, &text)
}

fn drain(ctx: &mut Ctx<'_>, method: &'static str) -> Result<(), CliError> {
    let client = ctx.ops_client(None)?;
    let reply = client.call(&call(ctx, method, "/node/drain"))?;
    emit(ctx, &reply.text())
}
