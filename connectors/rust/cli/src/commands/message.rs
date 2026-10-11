//! Mensagens (doc 19b, seção 18.11.5).

use std::cell::Cell;

use taba_client::{ConsumeOptions, Message, PublishOptions};

use super::{call, emit, number, pairs};
use crate::app::{CliError, Command, Ctx, EXIT_EMPTY};
use crate::args::{flag, val, Parsed};
use crate::output::{self, Format};

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            path: &["publish"],
            args: "<tópico> [corpo]",
            summary: "Publica uma mensagem; o corpo vem do argumento, de --file ou de --stdin.",
            min_pos: 1,
            max_pos: 2,
            opts: vec![
                val("file"),
                flag("stdin"),
                val("content-type"),
                flag("json"),
                flag("text"),
                val("group-id"),
                val("deduplication-id"),
                flag("id-only"),
                val("count"),
            ],
            run: publish,
        },
        Command {
            path: &["consume"],
            args: "<tópico>",
            summary: "Consome mensagens (GET); sem --ack imprime a mensagem inteira, com o receiptToken.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![
                val("max"),
                val("visibility"),
                val("wait"),
                flag("auto-create"),
                flag("persistent"),
                val("idle-timeout"),
                flag("fifo"),
                val("tag"),
                flag("ack"),
                flag("exit-empty"),
            ],
            run: consume,
        },
        Command {
            path: &["subscribe"],
            args: "<tópico>",
            summary: "Consome em laço com long polling; --exec roda um comando por mensagem e confirma se sai com 0.",
            min_pos: 1,
            max_pos: 1,
            opts: vec![val("max"), val("visibility"), val("wait"), val("exec"), val("limit")],
            run: subscribe,
        },
        Command {
            path: &["ack"],
            args: "<tópico> <id>",
            summary: "Confirma uma mensagem (DELETE); --receipt é o receiptToken da última entrega.",
            min_pos: 2,
            max_pos: 2,
            opts: vec![val("receipt")],
            run: ack,
        },
        Command {
            path: &["visibility"],
            args: "<tópico> <id>",
            summary: "Define a invisibilidade de uma mensagem entregue (PUT); --seconds 0 a devolve na hora.",
            min_pos: 2,
            max_pos: 2,
            opts: vec![val("receipt"), val("seconds")],
            run: visibility,
        },
    ]
}

/// De onde vem o corpo: o argumento, `--file` ou `--stdin`, e só um deles.
fn read_body(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<Vec<u8>, CliError> {
    let sources = usize::from(p.positional.len() > 1)
        + usize::from(p.has_values("file"))
        + usize::from(p.flag("stdin"));
    if sources != 1 {
        return Err(CliError::usage(if sources == 0 {
            "dê o corpo: como argumento, com --file <arquivo> ou com --stdin"
        } else {
            "o corpo vem de um só lugar: o argumento, --file ou --stdin"
        }));
    }
    if let Some(path) = p.value("file") {
        return std::fs::read(path)
            .map_err(|e| CliError::File(format!("não consegui ler {path}: {e}")));
    }
    if p.flag("stdin") {
        let mut body = Vec::new();
        ctx.input
            .read_to_end(&mut body)
            .map_err(|e| CliError::File(format!("não consegui ler a entrada padrão: {e}")))?;
        return Ok(body);
    }
    Ok(p.positional[1].clone().into_bytes())
}

fn content_type(p: &Parsed) -> Result<Option<String>, CliError> {
    let chosen = usize::from(p.has_values("content-type"))
        + usize::from(p.flag("json"))
        + usize::from(p.flag("text"));
    if chosen > 1 {
        return Err(CliError::usage(
            "use só um de --content-type, --json e --text",
        ));
    }
    Ok(if p.flag("json") {
        Some("application/json".to_owned())
    } else if p.flag("text") {
        Some("text/plain; charset=utf-8".to_owned())
    } else {
        p.value("content-type").map(str::to_owned)
    })
}

fn publish(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let topic = p.positional[0].clone();
    let body = read_body(ctx, p)?;
    let count = number(p, "count")?.unwrap_or(1);
    if count == 0 {
        return Err(CliError::usage("--count precisa ser pelo menos 1"));
    }
    let mut options = PublishOptions {
        content_type: content_type(p)?,
        group_id: p.value("group-id").map(str::to_owned),
        deduplication_id: p.value("deduplication-id").map(str::to_owned),
        request_id: ctx.globals.request_id.clone(),
        fifo: false,
    };
    let client = ctx.api_client()?;
    for _ in 0..count {
        let response = match client.publish(&topic, &body, &options) {
            // Fora da FIFO uma publicação que falhou no repasse não se repete (duplicaria); na FIFO o
            // primário deduplica, então a repetição é segura. Só aqui o CLI descobre qual é o caso.
            Err(e) if e.status == 502 || e.status == 504 => {
                let is_fifo = client.get_topic(&topic).ok().and_then(|t| t.fifo) == Some(true);
                if !is_fifo {
                    return Err(e.into());
                }
                options.fifo = true;
                client.publish(&topic, &body, &options)?
            }
            other => other?,
        };
        if p.flag("id-only") {
            if let Some(id) = &response.id {
                if !ctx.globals.quiet {
                    ctx.say(id)?;
                }
                continue;
            }
        }
        emit(ctx, &response.raw)?;
    }
    Ok(())
}

fn consume(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let auto_create = p.flag("auto-create");
    if !auto_create
        && (p.flag("persistent")
            || p.flag("fifo")
            || p.has_values("idle-timeout")
            || p.has_values("tag"))
    {
        return Err(CliError::usage(
            "--persistent, --fifo, --idle-timeout e --tag só valem com --auto-create",
        ));
    }
    let options = ConsumeOptions {
        max_messages: number(p, "max")?.map(|n| n as u32),
        visibility_timeout: number(p, "visibility")?.map(|n| n as u32),
        wait_time: number(p, "wait")?.map(|n| n as u32),
        auto_create,
        volatile: p.flag("persistent").then_some(false),
        idle_timeout: number(p, "idle-timeout")?.map(|n| n as u32),
        fifo: p.flag("fifo").then_some(true),
        tags: pairs(p, "tag")?,
        request_id: ctx.globals.request_id.clone(),
    };
    let client = ctx.api_client()?;
    let messages = client.consume(&p.positional[0], &options)?;
    let format = ctx.format()?;
    if !ctx.globals.quiet {
        match format {
            Format::Raw => {
                for m in &messages {
                    ctx.say(String::from_utf8_lossy(&m.body).trim_end_matches(['\r', '\n']))?;
                }
            }
            _ => {
                let raws: Vec<&str> = messages.iter().map(|m| m.raw.as_str()).collect();
                emit(ctx, &format!("{{\"messages\":[{}]}}", raws.join(",")))?;
            }
        }
    }
    if p.flag("ack") {
        for m in &messages {
            send_ack(ctx, &client, &p.positional[0], &m.id, &m.receipt_token)?;
        }
    }
    if p.flag("exit-empty") && messages.is_empty() {
        return Err(CliError::Exit(EXIT_EMPTY));
    }
    Ok(())
}

fn print_message(ctx: &mut Ctx<'_>, m: &Message) -> Result<(), CliError> {
    if ctx.globals.quiet {
        return Ok(());
    }
    match ctx.format()? {
        Format::Raw => ctx.say(String::from_utf8_lossy(&m.body).trim_end_matches(['\r', '\n'])),
        format => {
            let text = output::render(&m.raw, format);
            ctx.say(&text)
        }
    }
}

fn subscribe(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let options = ConsumeOptions {
        max_messages: Some(number(p, "max")?.unwrap_or(1) as u32),
        visibility_timeout: number(p, "visibility")?.map(|n| n as u32),
        wait_time: Some(number(p, "wait")?.unwrap_or(20) as u32),
        request_id: ctx.globals.request_id.clone(),
        ..ConsumeOptions::default()
    };
    let limit = number(p, "limit")?;
    let exec = p.value("exec").map(str::to_owned);
    let client = ctx.api_client()?;
    let handled = Cell::new(0u64);
    let stop = || limit.is_some_and(|n| handled.get() >= n);
    let mut fatal: Option<CliError> = None;
    let result = client.subscribe(&p.positional[0], &options, stop, |m| {
        handled.set(handled.get() + 1);
        match &exec {
            Some(command) => match ctx.platform.exec(command, &m.body) {
                Ok(0) => Ok(()),
                Ok(code) => Err(format!(
                    "o comando saiu com {code}; a mensagem {} não foi confirmada",
                    m.id
                )
                .into()),
                Err(e) => Err(format!("não consegui rodar `{command}`: {e}").into()),
            },
            None => print_message(ctx, m).map_err(|e| {
                fatal = Some(e);
                "falha ao imprimir".into()
            }),
        }
    });
    if let Some(error) = fatal {
        return Err(error);
    }
    result?;
    Ok(())
}

fn receipt(p: &Parsed) -> Result<&str, CliError> {
    p.value("receipt")
        .ok_or_else(|| CliError::usage("--receipt é obrigatório: o receiptToken da última entrega"))
}

fn message_path(topic: &str, id: &str) -> String {
    format!(
        "/topics/{}/messages/{}",
        taba_client::http::percent_encode(topic),
        taba_client::http::percent_encode(id)
    )
}

/// `DELETE /topics/{topic}/messages/{id}?receiptToken=`, com o `--request-id`.
fn send_ack(
    ctx: &Ctx<'_>,
    client: &taba_client::Client,
    topic: &str,
    id: &str,
    token: &str,
) -> Result<(), CliError> {
    client.call(&call(ctx, "DELETE", message_path(topic, id)).query("receiptToken", token))?;
    Ok(())
}

fn ack(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let token = receipt(p)?.to_owned();
    let client = ctx.api_client()?;
    send_ack(ctx, &client, &p.positional[0], &p.positional[1], &token)
}

fn visibility(ctx: &mut Ctx<'_>, p: &Parsed) -> Result<(), CliError> {
    let token = receipt(p)?.to_owned();
    let seconds = number(p, "seconds")?
        .ok_or_else(|| CliError::usage("--seconds é obrigatório (0 devolve a mensagem na hora)"))?;
    let client = ctx.api_client()?;
    let c = call(
        ctx,
        "PUT",
        format!(
            "{}/visibility",
            message_path(&p.positional[0], &p.positional[1])
        ),
    )
    .query("receiptToken", &token)
    .json_body(format!("{{\"visibilityTimeout\":{seconds}}}"));
    emit(ctx, &client.call(&c)?.text())
}
