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
        sim.press('4');
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
