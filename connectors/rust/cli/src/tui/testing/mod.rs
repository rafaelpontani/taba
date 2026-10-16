//! Helpers for the tests of the interface: a model with the defaults, scripted replies, and a way to feed it
//! keys and clock ticks and look at the effects it asks for.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taba_client::{Error, Reply};

use crate::tui::api::ApiCall;
use crate::tui::effect::Effect;
use crate::tui::i18n::Lang;
use crate::tui::model::{Model, Options};
use crate::tui::msg::Msg;
use crate::tui::theme::Theme;
use crate::tui::update::update;

pub const NODE_A: &str = "01a13640-0000-7000-8000-000000000001";
pub const NODE_B: &str = "01a13640-0000-7000-8000-000000000002";
pub const NODE_C: &str = "01a13640-0000-7000-8000-000000000003";

pub fn options(lang: Lang) -> Options {
    Options {
        lang,
        theme: Theme::default(),
        profile: "prod".to_owned(),
        url: "https://taba.example:8080".to_owned(),
        retention_secs: 3600,
        interval_secs: 5,
        profiles: crate::profile::ConfigFile::default(),
        choose_profile: false,
    }
}

pub fn me_root() -> &'static str {
    r#"{"authEnabled":true,"root":true,"id":null,"name":"root","permissions":["publish","consume","create","admin"],"topicPrefixes":[]}"#
}

pub fn me_plain() -> &'static str {
    r#"{"authEnabled":true,"root":false,"id":"x","name":"app","permissions":["publish"],"topicPrefixes":["app."]}"#
}

fn node_status(id: &str, used: u64, drain: &str, rejected: u32) -> String {
    format!(
        r#"{{"nodeId":"{id}","reachable":true,"status":{{"nodeId":"{id}","version":"1.4.0","clusterName":"prod-eu","mode":"production","state":"active","drainState":"{drain}","startedAt":"2026-10-15T08:00:00.000Z","collectedAt":"2026-10-15T12:00:00.000Z","storage":{{"memoryUsedBytes":{used},"memoryLimitBytes":1073741824,"memoryMessages":1200,"diskMessages":340000,"diskFreeBytes":53687091200}},"signals":{{"membersVersion":7,"quorumAvailable":true,"channels":2,"syncingBlocks":0,"blocksBelowReplication":0,"blocksBelowQuorum":0,"frozenNodes":0}},"runtime":{{"activeRequests":3.0,"rateLimitRejected":{rejected}.0,"channelRttP95Seconds":0.0123}}}}}}"#
    )
}

/// `GET /cluster/status` of three nodes; with `partial` the third did not answer.
pub fn status_json(partial: bool) -> String {
    let third = if partial {
        format!(r#"{{"nodeId":"{NODE_C}","reachable":false,"status":null}}"#)
    } else {
        node_status(NODE_C, 100_000_000, "active", 0)
    };
    let (conditions, worst, missing) = if partial {
        (
            r#"[{"condition":"member-unreachable","severity":"warning"}]"#,
            "member-unreachable",
            format!(r#"[{{"nodeId":"{NODE_C}","reason":"timeout"}}]"#),
        )
    } else {
        ("[]", "ok", "[]".to_owned())
    };
    format!(
        r#"{{"scope":"cluster","collectedAt":"2026-10-15T12:00:00.000Z","partial":{partial},"missingNodes":{missing},"worstCondition":"{worst}","conditions":{conditions},"summary":{{"members":{{"active":3}},"membersVersion":7,"quorumAvailable":true,"syncingBlocks":0}},"nodes":[{a},{b},{third}]}}"#,
        a = node_status(NODE_A, 805_306_368, "active", 5),
        b = node_status(NODE_B, 214_748_364, "active", 0),
    )
}

pub fn nodes_json() -> String {
    format!(
        r#"{{"collectedAt":"2026-10-15T12:00:00.000Z","membersVersion":7,"partial":false,"missingNodes":[],"items":[{{"nodeId":"{NODE_A}","self":true,"reachable":true,"state":"active","drainState":"active","version":"1.4.0","memberState":"active"}},{{"nodeId":"{NODE_B}","self":false,"reachable":true,"state":"active","drainState":"active","version":"1.4.0","memberState":"active"}}]}}"#
    )
}

/// `GET /tokens`: one that never expires, one that expires in three days and one that already expired.
pub fn tokens_json() -> &'static str {
    r#"{"items":[{"id":"0191f3a2-7c1e-7000-8000-00000000000a","name":"orders-app","permissions":["publish","consume"],"topicPrefixes":["orders."],"createdAt":"2026-10-01T10:00:00Z","expiresAt":null},{"id":"0191f3a2-7c1e-7000-8000-00000000000b","name":"billing","permissions":["admin"],"topicPrefixes":[],"createdAt":"2026-10-10T10:00:00Z","expiresAt":"2026-10-18T00:00:00Z"},{"id":"0191f3a2-7c1e-7000-8000-00000000000c","name":"old-batch","permissions":["consume"],"topicPrefixes":["batch.","etl."],"createdAt":"2026-09-01T10:00:00Z","expiresAt":"2026-10-01T00:00:00Z"}]}"#
}

pub fn topics_json() -> &'static str {
    r#"{"items":[{"id":"01a1-1","name":"orders","type":"queue","fifo":true,"volatile":false,"tags":{"team":"shop"},"createdAt":"2026-10-01T10:00:00Z","maxDeliveries":null,"maxMessageBytes":null,"ttl":3600,"effective":{"maxDeliveries":10,"maxMessageBytes":5242880,"ttl":3600}},{"id":"01a1-2","name":"orders-dead-letter","type":"queue","deadLetter":true,"source":"orders","fifo":false,"volatile":false,"tags":{},"createdAt":"2026-10-01T10:00:00Z"},{"id":"01a1-3","name":"fanout","type":"distributor","prefixes":["orders","billing"],"tags":{},"createdAt":"2026-10-12T10:00:00Z"},{"id":"01a1-4","name":"replies","type":"queue","volatile":true,"fifo":false,"idleTimeout":600,"tags":{},"createdAt":"2026-10-14T10:00:00Z","effective":{"maxDeliveries":10,"maxMessageBytes":5242880,"ttl":1209600}}]}"#
}

pub fn topic_stats_json() -> &'static str {
    r#"{"items":[{"name":"fanout"},{"name":"orders","depth":1500,"publishRate":12.5,"consumeRate":3.0,"oldestAge":125.0,"worstCondition":"backlog","conditions":[{"condition":"backlog","severity":"warning"}]},{"name":"orders-dead-letter","depth":0,"publishRate":0.0,"consumeRate":0.0,"worstCondition":"ok","conditions":[]},{"name":"replies","depth":7,"publishRate":0.2,"consumeRate":0.2,"oldestAge":3.0,"worstCondition":"ok","conditions":[]}],"partial":false}"#
}

/// One reading of `orders` for its charts; `step` makes the numbers grow so the lines are not flat.
pub fn topic_sample_json(step: u64) -> String {
    format!(
        r#"{{"name":"orders","visibleMessages":{},"invisibleMessages":{},"depth":{},"publishRate":{:.1},"consumeRate":{:.1},"ackRate":{:.1},"oldestAge":{},"topGroups":[{{"groupId":"customer-17","messages":900,"oldestAge":{}}},{{"groupId":"customer-4","messages":300,"oldestAge":{}}}],"partial":false}}"#,
        1000 + step * 200,
        20 + step * 5,
        1020 + step * 205,
        10.0 + step as f64,
        6.0 + step as f64 * 0.5,
        5.5 + step as f64 * 0.5,
        120 + step * 30,
        100 + step * 25,
        30 + step * 4
    )
}

/// `GET /topics/orders/messages` answering `count` messages, `m1`, `m2`, ...
pub fn messages_json(count: u32) -> String {
    let items: Vec<String> = (1..=count)
        .map(|n| {
            format!(
                r#"{{"id":"01a1-m{n}","body":{{"order":{n},"total":19.9}},"contentType":"application/json","receiptToken":"rt-{n}","deliveryCount":1,"publishedAt":"2026-10-16T10:00:00.000Z","invisibleUntil":"2026-10-16T10:00:30.000Z","groupId":"customer-{n}"}}"#
            )
        })
        .collect();
    format!(r#"{{"messages":[{}]}}"#, items.join(","))
}

pub fn ok(body: &str) -> Result<Reply, Error> {
    Ok(Reply {
        status: 200,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
        request_id: None,
        recovered: false,
    })
}

pub fn fail(status: u16, code: &str, body: &str) -> Result<Reply, Error> {
    Err(Error {
        status,
        code: code.to_owned(),
        message: format!("{code} happened"),
        request_id: Some("req-1".to_owned()),
        retry_after: None,
        details: None,
        body: body.as_bytes().to_vec(),
    })
}

pub fn offline() -> Result<Reply, Error> {
    Err(Error::from_message("client.network", "connection refused"))
}

pub fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

pub fn ch(c: char) -> Msg {
    key(KeyCode::Char(c))
}

/// A model and the log of the effects `update` has asked for.
pub struct Sim {
    pub m: Model,
    pub log: Vec<Effect>,
    wall: i64,
}

impl Sim {
    pub fn new(lang: Lang) -> Sim {
        Sim::with_options(options(lang))
    }

    pub fn with_options(options: Options) -> Sim {
        Sim {
            m: Model::new(options),
            log: Vec::new(),
            wall: 1_792_065_600_000,
        }
    }

    pub fn send(&mut self, msg: Msg) -> Vec<Effect> {
        let effects = update(&mut self.m, msg);
        self.log.extend(effects.clone());
        effects
    }

    pub fn press(&mut self, c: char) -> Vec<Effect> {
        self.send(ch(c))
    }

    pub fn code(&mut self, code: KeyCode) -> Vec<Effect> {
        self.send(key(code))
    }

    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        let wall_ms = self.wall + i64::try_from(now).unwrap_or(0);
        self.send(Msg::Tick { now, wall_ms })
    }

    /// Answers a call effect.
    pub fn answer(&mut self, effect: &Effect, result: Result<Reply, Error>) -> Vec<Effect> {
        let Effect::Call {
            id,
            generation,
            call,
        } = effect
        else {
            panic!("not a call: {effect:?}");
        };
        self.send(Msg::Reply {
            id: *id,
            generation: *generation,
            call: call.clone(),
            result,
        })
    }

    /// Ticks at `now` and returns the one call it asked for (or none).
    pub fn poll(&mut self, now: u64) -> Vec<ApiCall> {
        self.tick(now)
            .into_iter()
            .filter_map(|e| match e {
                Effect::Call { call, .. } => Some(call),
                _ => None,
            })
            .collect()
    }

    /// Opens the session as root with a healthy cluster, at time `1000`.
    pub fn root_with_status(lang: Lang) -> Sim {
        let mut sim = Sim::new(lang);
        let effects = sim.tick(0);
        sim.answer(&effects[0], ok(me_root()));
        let effects = sim.tick(100);
        sim.answer(&effects[0], ok(&status_json(false)));
        sim
    }

    pub fn with_status_json(lang: Lang, status: &str) -> Sim {
        let mut sim = Sim::new(lang);
        let effects = sim.tick(0);
        sim.answer(&effects[0], ok(me_root()));
        let effects = sim.tick(100);
        sim.answer(&effects[0], ok(status));
        sim
    }

    /// A root session on the Tokens tab with three tokens read (the clock is in October 2026).
    pub fn on_tokens(lang: Lang) -> Sim {
        let mut sim = Sim::root_with_status(lang);
        sim.press('3');
        let effects = sim.tick(1_000);
        let call = effects
            .iter()
            .find(|e| {
                matches!(
                    e,
                    Effect::Call {
                        call: ApiCall::Tokens(None),
                        ..
                    }
                )
            })
            .expect("the tab asks for the tokens")
            .clone();
        sim.answer(&call, ok(tokens_json()));
        sim
    }

    /// A root session on the Topics tab with its topics and their numbers read.
    pub fn on_topics(lang: Lang) -> Sim {
        let mut sim = Sim::root_with_status(lang);
        sim.press('2');
        let effects = sim.tick(1_000);
        for effect in &effects {
            let Effect::Call { call, .. } = effect else {
                continue;
            };
            match call {
                ApiCall::Topics(None) => {
                    sim.answer(effect, ok(topics_json()));
                }
                ApiCall::TopicStats(None) => {
                    sim.answer(effect, ok(topic_stats_json()));
                }
                _ => {}
            }
        }
        sim
    }

    /// The charts of the queue `orders` after five readings, 5 s apart, the last at 22 s.
    pub fn on_topic_charts(lang: Lang) -> Sim {
        let mut sim = Sim::on_topics(lang);
        sim.press('j');
        sim.press('g');
        for (step, now) in [2_000u64, 7_000, 12_000, 17_000, 22_000]
            .into_iter()
            .enumerate()
        {
            let effects = sim.tick(now);
            let asked = effects.iter().find(|e| {
                matches!(
                    e,
                    Effect::Call {
                        call: ApiCall::TopicDetail(_),
                        ..
                    }
                )
            });
            let effect = asked.expect("the charts ask for a reading").clone();
            sim.answer(&effect, ok(&topic_sample_json(step as u64)));
        }
        sim
    }

    /// The Messages view of the queue `orders`, opened with `x` from the list; nothing was read yet.
    pub fn on_topic_messages(lang: Lang) -> Sim {
        let mut sim = Sim::on_topics(lang);
        sim.press('j');
        sim.press('x');
        sim
    }

    pub fn plain_with_nodes(lang: Lang) -> Sim {
        let mut sim = Sim::new(lang);
        let effects = sim.tick(0);
        sim.answer(&effects[0], ok(me_plain()));
        let effects = sim.tick(100);
        assert_eq!(effects.len(), 1, "{effects:?}");
        sim.answer(&effects[0], ok(&nodes_json()));
        sim
    }
}
