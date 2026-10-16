use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::profile::ConfigFile;
use crate::tui::i18n::Lang;
use crate::tui::model::{ProfilesScreen, Tab};
use crate::tui::msg::Msg;
use crate::tui::testing::*;

pub fn file() -> ConfigFile {
    let mut file = ConfigFile::default();
    for (name, url) in [
        ("dev", "http://localhost:8080"),
        ("prod", "https://taba.example:8080"),
        ("lab", "https://lab.example"),
    ] {
        let mut profile = Profile::default();
        profile.set("url", url).unwrap();
        file.add(name, profile).unwrap();
    }
    file.default = Some("prod".to_owned());
    file
}

/// A session on the Profiles tab with the three profiles of `file()`.
fn on_profiles(lang: Lang) -> Sim {
    let mut sim = Sim::new(lang);
    sim.m.profiles = ProfilesScreen {
        selected: file().default.clone(),
        file: file(),
    };
    sim.press('5');
    assert_eq!(sim.m.tab, Tab::Profiles);
    sim
}

fn ctrl(c: char) -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn type_text(sim: &mut Sim, text: &str) {
    for c in text.chars() {
        sim.press(c);
    }
}

fn saved_file(effects: &[Effect]) -> ConfigFile {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::SaveProfiles { file, .. } => Some(file.clone()),
            _ => None,
        })
        .expect("the profiles are saved")
}

/// TUI: TUI-HLP-04
#[test]
fn the_list_starts_on_the_default_profile_and_moves_inside_the_file() {
    let mut sim = on_profiles(Lang::EnUs);
    assert_eq!(selected(&sim.m).unwrap().0, "prod");
    sim.press('j');
    assert_eq!(selected(&sim.m).unwrap().0, "lab");
    sim.press('j');
    assert_eq!(selected(&sim.m).unwrap().0, "lab");
    sim.press('g');
    assert_eq!(selected(&sim.m).unwrap().0, "dev");
}

/// TUI: TUI-HLP-04
#[test]
fn d_makes_the_selected_profile_the_default_and_saves_the_file() {
    let mut sim = on_profiles(Lang::EnUs);
    sim.press('g');
    let effects = sim.press('d');
    let next = saved_file(&effects);
    assert_eq!(next.default.as_deref(), Some("dev"));
    // Nothing changes until the file was written.
    assert_eq!(sim.m.profiles.file.default.as_deref(), Some("prod"));
    sim.send(Msg::ProfilesSaved {
        file: next,
        select: Some("dev".into()),
        result: Ok(()),
    });
    assert_eq!(sim.m.profiles.file.default.as_deref(), Some("dev"));
    // Already the default: it only says so.
    assert!(sim.press('d').is_empty());
    assert!(sim
        .m
        .notice
        .as_ref()
        .unwrap()
        .text
        .contains("already the default"));
}

/// TUI: TUI-HLP-04
#[test]
fn a_new_profile_is_checked_and_saved_with_the_default_if_ticked() {
    let mut sim = on_profiles(Lang::EnUs);
    sim.press('n');
    // A name that is not valid stays in the form.
    type_text(&mut sim, "bad name");
    assert!(sim.send(ctrl('s')).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form closed");
    };
    assert!(editor.error.is_some());
    for _ in 0..8 {
        sim.code(KeyCode::Backspace);
    }
    // An existing name is refused.
    type_text(&mut sim, "dev");
    assert!(sim.send(ctrl('s')).is_empty());
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form closed");
    };
    assert_eq!(
        editor.error.as_deref(),
        Some("There is already a profile called dev.")
    );
    for _ in 0..3 {
        sim.code(KeyCode::Backspace);
    }
    type_text(&mut sim, "stage");
    sim.code(KeyCode::Tab);
    type_text(&mut sim, "https://stage.example");
    // url, ops-url, token-file, token: four fields further the token is typed.
    for _ in 0..3 {
        sim.code(KeyCode::Tab);
    }
    type_text(&mut sim, "hunter2");
    sim.code(KeyCode::Tab);
    sim.code(KeyCode::Tab);
    sim.press(' ');
    let effects = sim.send(ctrl('s'));
    let next = saved_file(&effects);
    let stage = next.get("stage").unwrap();
    assert_eq!(stage.get("url"), Some("https://stage.example"));
    assert_eq!(stage.get("token"), Some("hunter2"));
    assert_eq!(next.default.as_deref(), Some("stage"));
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("closed before the file was written");
    };
    assert!(editor.busy);
    sim.send(Msg::ProfilesSaved {
        file: next,
        select: Some("stage".into()),
        result: Ok(()),
    });
    assert!(sim.m.modal.is_none());
    assert_eq!(selected(&sim.m).unwrap().0, "stage");
}

/// TUI: TUI-HLP-04
#[test]
fn a_profile_that_could_not_be_written_keeps_the_form_open_with_the_reason() {
    let mut sim = on_profiles(Lang::EnUs);
    sim.press('e');
    type_text(&mut sim, "x");
    let effects = sim.send(ctrl('s'));
    let next = saved_file(&effects);
    sim.send(Msg::ProfilesSaved {
        file: next,
        select: None,
        result: Err("disk full".into()),
    });
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form closed");
    };
    assert!(!editor.busy);
    assert_eq!(editor.error.as_deref(), Some("disk full"));
    assert_eq!(
        sim.m.profiles.file,
        file(),
        "the file in memory did not change"
    );
}

/// TUI: TUI-HLP-04
#[test]
fn editing_keeps_the_fields_the_form_does_not_show_and_a_rename_keeps_the_default() {
    let mut sim = on_profiles(Lang::EnUs);
    sim.m
        .profiles
        .file
        .get_mut("prod")
        .unwrap()
        .set("timeout", "90")
        .unwrap();
    sim.press('e');
    for _ in 0..4 {
        sim.code(KeyCode::Backspace);
    }
    type_text(&mut sim, "live");
    let effects = sim.send(ctrl('s'));
    let next = saved_file(&effects);
    assert!(next.get("prod").is_none());
    assert_eq!(next.get("live").unwrap().get("timeout"), Some("90"));
    assert_eq!(next.default.as_deref(), Some("live"));
    assert_eq!(
        next.names(),
        vec!["dev", "live", "lab"],
        "same place in the list"
    );
}

/// TUI: TUI-HLP-04
#[test]
fn removing_asks_first_and_the_default_answer_is_cancel() {
    let mut sim = on_profiles(Lang::EnUs);
    sim.press('x');
    assert!(sim.code(KeyCode::Enter).is_empty());
    assert!(sim.m.modal.is_none());
    sim.press('x');
    sim.code(KeyCode::Tab);
    let effects = sim.code(KeyCode::Enter);
    let next = saved_file(&effects);
    assert!(next.get("prod").is_none());
    assert_eq!(next.default, None, "the default left with it");
}

/// TUI: TUI-HLP-04
#[test]
fn the_connection_can_be_tested_with_values_that_are_not_saved() {
    let mut sim = on_profiles(Lang::PtBr);
    sim.press('n');
    type_text(&mut sim, "novo");
    sim.code(KeyCode::Tab);
    type_text(&mut sim, "https://novo.example");
    let effects = sim.send(ctrl('t'));
    let [Effect::CheckProfile(profile)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(profile.get("url"), Some("https://novo.example"));
    sim.send(Msg::ProfileChecked(CheckResult::Refused {
        url: "https://novo.example".into(),
        status: 401,
    }));
    let Some(Modal::Editor(editor)) = &sim.m.modal else {
        panic!("the form closed");
    };
    let (failed, text) = editor.note.clone().unwrap();
    assert!(failed);
    assert_eq!(text, "https://novo.example respondeu 401: confira o token");
    // The list's `t` tests the saved profile and answers in the footer.
    sim.code(KeyCode::Esc);
    let effects = sim.press('t');
    assert!(matches!(effects.as_slice(), [Effect::CheckProfile(_)]));
    sim.send(Msg::ProfileChecked(CheckResult::Reachable {
        url: "http://x".into(),
        millis: 12,
    }));
    assert_eq!(
        sim.m.notice.as_ref().unwrap().text,
        "http://x respondeu em 12 ms"
    );
}

/// TUI: TUI-HLP-05
#[test]
fn enter_connects_with_the_profile_and_the_session_starts_over_with_it() {
    let mut sim = on_profiles(Lang::EnUs);
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(effects, vec![Effect::Connect("prod".to_owned())]);
    // What was read from the old server is dropped, and so is a reply still on its way.
    sim.m.me.data = Some(Default::default());
    let before = sim.m.generation;
    sim.send(Msg::Connected {
        profile: "lab".into(),
        url: "https://lab.example".into(),
        result: Ok(()),
    });
    assert_eq!(
        (sim.m.profile.as_str(), sim.m.url.as_str()),
        ("lab", "https://lab.example")
    );
    assert_eq!(sim.m.generation, before + 1);
    assert!(sim.m.me.data.is_none());
    assert!(sim
        .m
        .notice
        .as_ref()
        .unwrap()
        .text
        .contains("Connected with lab"));
    sim.send(Msg::Connected {
        profile: "broken".into(),
        url: String::new(),
        result: Err("no token file".into()),
    });
    assert_eq!(sim.m.profile, "lab", "a failed connection changes nothing");
    assert!(sim
        .m
        .notice
        .as_ref()
        .unwrap()
        .text
        .contains("no token file"));
}

fn opening(profiles: ConfigFile, choose: bool) -> Sim {
    let mut options = options(Lang::EnUs);
    options.profiles = profiles;
    options.choose_profile = choose;
    Sim::with_options(options)
}

/// TUI: TUI-HLP-05
#[test]
fn with_several_profiles_and_none_given_the_session_opens_on_the_choice_with_the_default_selected()
{
    let mut sim = opening(file(), true);
    assert!(sim.m.awaiting_profile);
    assert_eq!(sim.m.modal, Some(Modal::Profiles { selected: 1 }));
    // Nothing is read from a server nobody chose yet.
    assert!(sim.tick(0).is_empty());
    assert!(sim.tick(10_000).is_empty());
    // The arrows move, Enter connects with the one under the cursor.
    sim.press('j');
    let effects = sim.code(KeyCode::Enter);
    assert_eq!(effects, vec![Effect::Connect("lab".to_owned())]);
    sim.send(Msg::Connected {
        profile: "lab".into(),
        url: "https://lab.example".into(),
        result: Ok(()),
    });
    assert!(sim.m.modal.is_none() && !sim.m.awaiting_profile);
    assert!(!sim.tick(11_000).is_empty(), "now the reading starts");
}

/// TUI: TUI-HLP-05
#[test]
fn escape_at_the_start_takes_the_default_profile_and_later_only_closes_the_choice() {
    let mut sim = opening(file(), true);
    sim.press('k');
    assert_eq!(
        sim.code(KeyCode::Esc),
        vec![Effect::Connect("prod".to_owned())]
    );
    sim.send(Msg::Connected {
        profile: "prod".into(),
        url: "https://taba.example:8080".into(),
        result: Ok(()),
    });
    // Later, `P` opens the same choice and Esc leaves it.
    sim.press('P');
    assert_eq!(sim.m.modal, Some(Modal::Profiles { selected: 1 }));
    assert!(sim.code(KeyCode::Esc).is_empty());
    assert!(sim.m.modal.is_none());
}

/// TUI: TUI-HLP-05
#[test]
fn with_one_profile_or_none_nothing_is_asked() {
    let mut one = ConfigFile::default();
    one.add("only", Profile::default()).unwrap();
    for profiles in [one, ConfigFile::default()] {
        let sim = opening(profiles, true);
        assert!(!sim.m.awaiting_profile);
        assert!(sim.m.modal.is_none());
    }
    // A profile that was given is not asked about either.
    let sim = opening(file(), false);
    assert!(sim.m.modal.is_none());
    let mut sim = opening(ConfigFile::default(), false);
    sim.press('P');
    assert!(sim.m.modal.is_none(), "there is nothing to switch to");
    assert!(sim.m.notice.is_some());
}
