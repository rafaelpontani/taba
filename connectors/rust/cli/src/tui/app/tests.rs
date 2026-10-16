use std::sync::mpsc;
use std::sync::Mutex;

use taba_client::{Call, Error, Reply};

use super::*;
use crate::profile::{ConfigFile, Profile};
use crate::tui::api::ApiCall;
use crate::tui::i18n::Lang;
use crate::tui::testing::*;

#[derive(Default)]
struct FakeHost {
    languages: Vec<Lang>,
    files: Vec<(String, String)>,
    fail_writes: bool,
    secrets: Vec<(String, String)>,
    saved: Vec<crate::profile::ConfigFile>,
    connected: Vec<String>,
    fail_connect: bool,
}

impl Host for FakeHost {
    fn save_language(&mut self, lang: Lang) -> Result<(), String> {
        self.languages.push(lang);
        Ok(())
    }

    fn write_file(&mut self, path: &str, contents: &str) -> Result<(), String> {
        if self.fail_writes {
            return Err("exists".to_owned());
        }
        self.files.push((path.to_owned(), contents.to_owned()));
        Ok(())
    }

    fn write_secret(&mut self, path: &str, contents: &str) -> Result<(), String> {
        self.secrets.push((path.to_owned(), contents.to_owned()));
        Ok(())
    }

    fn save_profiles(&mut self, file: &ConfigFile) -> Result<(), String> {
        if self.fail_writes {
            return Err("disk full".to_owned());
        }
        self.saved.push(file.clone());
        Ok(())
    }

    fn check_profile(&mut self, _: &Profile) -> crate::tui::msg::CheckResult {
        crate::tui::msg::CheckResult::Reachable {
            url: "http://x".to_owned(),
            millis: 7,
        }
    }

    fn connect(&mut self, name: &str) -> Result<Connection, String> {
        if self.fail_connect {
            return Err("no token file".to_owned());
        }
        self.connected.push(name.to_owned());
        Ok(Connection {
            backend: Arc::new(Seen(Mutex::new(Vec::new()))),
            url: format!("http://{name}.example"),
        })
    }
}

fn switch() -> SwitchBackend {
    SwitchBackend::new(Arc::new(Seen(Mutex::new(Vec::new()))))
}

struct Seen(Mutex<Vec<String>>);

impl Backend for Seen {
    fn call(&self, call: &Call) -> Result<Reply, Error> {
        self.0.lock().unwrap().push(call.path.clone());
        Err(Error::from_message("client.network", "offline"))
    }
}

fn workers() -> (Workers, mpsc::Receiver<Msg>, Arc<Seen>) {
    let backend = Arc::new(Seen(Mutex::new(Vec::new())));
    let (tx, rx) = mpsc::channel();
    (Workers::start(backend.clone(), tx, 1), rx, backend)
}

/// TUI: TUI-FRM-08
#[test]
fn a_call_effect_goes_to_a_worker_and_its_reply_comes_back() {
    let (workers, rx, backend) = workers();
    let mut sim = Sim::new(Lang::EnUs);
    let mut host = FakeHost::default();
    let keep = carry_out(
        vec![Effect::Call {
            id: 5,
            generation: 0,
            call: ApiCall::Nodes,
        }],
        &mut sim.m,
        &workers,
        &switch(),
        &mut host,
    );
    assert!(keep);
    let reply = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(reply, Msg::Reply { id: 5, .. }));
    assert_eq!(*backend.0.lock().unwrap(), vec!["/nodes".to_owned()]);
}

/// TUI: TUI-MON-07
#[test]
fn a_file_effect_writes_through_the_host_and_reports_the_result() {
    let (workers, _rx, _) = workers();
    let mut sim = Sim::new(Lang::EnUs);
    let mut host = FakeHost::default();
    let effects = vec![Effect::WriteFile {
        path: "a.csv".into(),
        contents: "time\n".into(),
    }];
    assert!(carry_out(
        effects,
        &mut sim.m,
        &workers,
        &switch(),
        &mut host
    ));
    assert_eq!(host.files, vec![("a.csv".to_owned(), "time\n".to_owned())]);
    assert_eq!(sim.m.notice.as_ref().unwrap().text, "Wrote a.csv");

    host.fail_writes = true;
    let effects = vec![Effect::WriteFile {
        path: "b.csv".into(),
        contents: String::new(),
    }];
    carry_out(effects, &mut sim.m, &workers, &switch(), &mut host);
    assert!(sim.m.notice.as_ref().unwrap().text.contains("exists"));
}

/// TUI: TUI-HLP-03
#[test]
fn the_language_effect_saves_it_through_the_host() {
    let (workers, _rx, _) = workers();
    let mut sim = Sim::new(Lang::EnUs);
    let mut host = FakeHost::default();
    carry_out(
        vec![Effect::SaveLanguage(Lang::PtBr)],
        &mut sim.m,
        &workers,
        &switch(),
        &mut host,
    );
    assert_eq!(host.languages, vec![Lang::PtBr]);
}

#[test]
fn the_quit_effect_stops_the_loop_after_the_other_effects_ran() {
    let (workers, _rx, _) = workers();
    let mut sim = Sim::new(Lang::EnUs);
    let mut host = FakeHost::default();
    let effects = vec![
        Effect::SaveLanguage(Lang::EnUs),
        Effect::Quit,
        Effect::SaveLanguage(Lang::PtBr),
    ];
    assert!(!carry_out(
        effects,
        &mut sim.m,
        &workers,
        &switch(),
        &mut host
    ));
    assert_eq!(host.languages.len(), 2);
}

/// TUI: TUI-HLP-04
#[test]
fn the_profile_effects_go_through_the_host_and_come_back_as_results() {
    let (workers, _rx, _) = workers();
    let mut sim = Sim::new(Lang::EnUs);
    let mut host = FakeHost::default();
    let mut file = ConfigFile::default();
    file.add("prod", Profile::default()).unwrap();
    let effects = vec![
        Effect::SaveProfiles {
            file: file.clone(),
            select: Some("prod".into()),
        },
        Effect::CheckProfile(Profile::default()),
    ];
    assert!(carry_out(
        effects,
        &mut sim.m,
        &workers,
        &switch(),
        &mut host
    ));
    assert_eq!(host.saved, vec![file.clone()]);
    assert_eq!(sim.m.profiles.file, file);
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "http://x answered in 7 ms"
    );
    host.fail_writes = true;
    let effects = vec![Effect::SaveProfiles {
        file: ConfigFile::default(),
        select: None,
    }];
    carry_out(effects, &mut sim.m, &workers, &switch(), &mut host);
    assert!(sim.m.notice.as_ref().unwrap().text.contains("disk full"));
    assert_eq!(sim.m.profiles.file, file, "a failed save changes nothing");
}

/// TUI: TUI-HLP-05
#[test]
fn connecting_points_the_calls_at_the_new_server_and_starts_over() {
    let first = Arc::new(Seen(Mutex::new(Vec::new())));
    let switch = SwitchBackend::new(first.clone());
    let mut sim = Sim::new(Lang::EnUs);
    sim.m.generation = 4;
    let mut host = FakeHost::default();
    let (workers, _rx, _) = self::workers();
    carry_out(
        vec![Effect::Connect("staging".into())],
        &mut sim.m,
        &workers,
        &switch,
        &mut host,
    );
    assert_eq!(host.connected, vec!["staging".to_owned()]);
    assert_eq!(sim.m.profile, "staging");
    assert_eq!(sim.m.url, "http://staging.example");
    assert_eq!(
        sim.m.generation, 5,
        "the replies of the old profile are dropped"
    );
    // The switch now sends to the new backend, not to the first.
    let _ = switch.call(&Call::new("GET", "/topics"));
    assert!(first.0.lock().unwrap().is_empty());

    host.fail_connect = true;
    carry_out(
        vec![Effect::Connect("other".into())],
        &mut sim.m,
        &workers,
        &switch,
        &mut host,
    );
    assert_eq!(
        sim.m.profile, "staging",
        "a failed connection keeps the profile"
    );
    assert!(sim
        .m
        .notice
        .as_ref()
        .unwrap()
        .text
        .contains("no token file"));
}
