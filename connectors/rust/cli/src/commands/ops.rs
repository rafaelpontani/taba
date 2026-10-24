//! The cluster, the nodes and the statistics (doc 19b, section 18.11.6). All of them go through the client API,
//! with the profile's token: the CLI lives outside the infrastructure and never calls the operations API, which is
//! for the collectors inside it.

use super::{call, emit, number, one_of, paged, pairs};
use crate::app::{CliError, Command, Ctx};
use crate::args::{flag, val, Parsed};
use crate::output::{self, Format};

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["cluster", "status"],
            args: "",
            summary: "Cluster state and conditions (admin); --scope cluster|node.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![val("scope")],
            run: cluster_status,
        },
        Command {
            path: &["node", "list"],
            args: "",
            summary: "The nodes of the cluster: id, state, drain state and version.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![],
            run: |ctx, _| {
                let client = ctx.api_client()?;
                emit(ctx, &client.call(&call(ctx, "GET", "/nodes"))?.text())
            },
        },
        Command {
            path: &["metrics"],
            args: "",
            summary: "Prometheus metrics of the node that answers, or of --node <id> (admin); --openmetrics asks for the OpenMetrics format.",
            min_pos: 0,
            max_pos: 0,
            opts: vec![val("node"), flag("openmetrics")],
            run: metrics,
        },
        Command {
            path: &["stats", "topics"],
            args: "",
            summary:
                "Topic statistics of the cluster: --scope, --tag k=v, --condition, --fields, --group-by, pagination.",
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
            summary: "Statistics and condition of a topic; --scope cluster|node.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![val("scope")],
            run: stats_topic,
        },
        Command {
            path: &["stats", "groups"],
            args: "<topic>",
            summary: "Statistics per group of a topic.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![val("limit"), val("cursor"), flag("all")],
            run: stats_groups,
        },
        Command {
            path: &["drain", "start"],
            args: "",
            summary: "Puts the node --node <id> into drain (admin).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![val("node")],
            run: |ctx, p| drain(ctx, p, "POST"),
        },
        Command {
            path: &["drain", "status"],
            args: "",
            summary: "The drain progress of the node --node <id> (admin).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![val("node")],
            run: |ctx, p| drain(ctx, p, "GET"),
        },
        Command {
            path: &["drain", "cancel"],
            args: "",
            summary:
                "Cancels the drain of --node <id>, or makes a node that already left come back as a new node (admin).",
            min_pos: 0,
            max_pos: 0,
            opts: vec![val("node")],
            run: |ctx, p| drain(ctx, p, "DELETE"),
        },
    ]
}

/// The `--node` option, which names a node of the cluster (`node list` shows the ids).
fn node_id(p: &Parsed) -> Result<String, CliError> {
    p.value("node")
        .map(taba_client::http::percent_encode)
        .ok_or_else(|| {
            CliError::Usage("--node <id> is required (`taba-cli node list` shows the ids)".into())
        })
}

fn cluster_status(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut c = call(ctx, "GET", "/cluster/status");
    if let Some(scope) = one_of(p, "scope", &["cluster", "node"])? {
        c.query.push(("scope".to_owned(), scope));
    }
    let client = ctx.api_client()?;
    let text = client.call(&c)?.text();
    if !ctx.globals.quiet && ctx.format()? == Format::Table {
        if let Some(table) = output::cluster_table(&text) {
            return ctx.say(&table);
        }
    }
    emit(ctx, &text)
}

fn metrics(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let path = match p.value("node") {
        Some(_) => format!("/nodes/{}/metrics", node_id(p)?),
        None => "/metrics".to_owned(),
    };
    let mut c = call(ctx, "GET", path);
    let accept = if p.flag("openmetrics") {
        "application/openmetrics-text"
    } else {
        "text/plain"
    };
    c.headers.push(("Accept".to_owned(), accept.to_owned()));
    let client = ctx.api_client()?;
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
    let client = ctx.api_client()?;
    let text = paged(
        ctx,
        &client,
        "/stats/topics",
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
            "/stats/topics/{}",
            taba_client::http::percent_encode(&p.positional[0])
        ),
    );
    if let Some(scope) = one_of(p, "scope", &["cluster", "node"])? {
        c.query.push(("scope".to_owned(), scope));
    }
    let client = ctx.api_client()?;
    emit(ctx, &client.call(&c)?.text())
}

fn stats_groups(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let mut query = Vec::new();
    if let Some(v) = p.value("limit") {
        number(p, "limit")?;
        query.push(("limit".to_owned(), v.to_owned()));
    }
    let client = ctx.api_client()?;
    let path = format!(
        "/stats/topics/{}/groups",
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

fn drain(ctx: &mut Ctx<'_>, p: &Parsed, method: &'static str) -> Result<(), CliError> {
    let path = format!("/nodes/{}/drain", node_id(p)?);
    let client = ctx.api_client()?;
    let reply = client.call(&call(ctx, method, path))?;
    emit(ctx, &reply.text())
}
