use super::*;

#[derive(Default)]
struct Fake {
    saved: Vec<ConfigFile>,
    fail_save: Option<String>,
    checks: Vec<Profile>,
    check_result: Option<Result<String, String>>,
}

impl Services for Fake {
    fn save(&mut self, file: &ConfigFile) -> Result<(), String> {
        if let Some(e) = &self.fail_save {
            return Err(e.clone());
        }
        self.saved.push(file.clone());
        Ok(())
    }

    fn check(&mut self, profile: &Profile) -> Result<String, String> {
        self.checks.push(profile.clone());
        self.check_result
            .clone()
            .unwrap_or_else(|| Ok("ok".to_owned()))
    }
}

fn profile(pairs: &[(&str, &str)]) -> Profile {
    let mut p = Profile::default();
    for (k, v) in pairs {
        p.set(k, v).unwrap();
    }
    p
}

fn file() -> ConfigFile {
    let mut f = ConfigFile::default();
    f.add("local", profile(&[("url", "http://127.0.0.1:8080")]))
        .unwrap();
    f.add(
        "homologacao",
        profile(&[("url", "https://taba-hml:8080"), ("token-file", "/seg/tok")]),
    )
    .unwrap();
    f.add("producao", Profile::default()).unwrap();
    f
}

fn app() -> App {
    App::new(file(), "C:\\cfg\\taba\\config.yml".into(), None)
}

fn press(app: &mut App, fake: &mut Fake, keys: &[Key]) -> Flow {
    let mut flow = Flow::Continue;
    for k in keys {
        flow = app.handle(*k, fake);
    }
    flow
}

fn typed(text: &str) -> Vec<Key> {
    text.chars().map(Key::Char).collect()
}

fn shown(app: &App) -> String {
    app.render(Style { color: false }, 100, 30).lines.join("\n")
}

/// UC: UC-CLI-029
#[test]
fn the_list_shows_profiles_with_the_default_and_the_session_profile_and_never_the_token() {
    let mut f = file();
    f.get_mut("homologacao")
        .unwrap()
        .set("token", "tb_secreto")
        .unwrap();
    let app = App::new(f, "C:\\cfg\\config.yml".into(), Some("homologacao".into()));
    let text = shown(&app);
    assert!(
        text.contains("Perfis do taba-cli") && text.contains("C:\\cfg\\config.yml"),
        "{text}"
    );
    assert!(text.contains("▶ local") && text.contains("●"), "{text}");
    assert!(
        text.contains("homologacao (sessão)") && text.contains("○"),
        "{text}"
    );
    assert!(
        text.contains("/seg/tok") && !text.contains("tb_secreto"),
        "{text}"
    );
    assert!(text.contains("↑↓ escolher") && text.contains("Del remover"));
}

/// UC: UC-CLI-029
#[test]
fn up_and_down_move_the_selection_and_stay_inside_the_list() {
    let mut a = app();
    let mut fake = Fake::default();
    press(
        &mut a,
        &mut fake,
        &[Key::Down, Key::Down, Key::Down, Key::Down],
    );
    assert_eq!(a.selected, 2);
    press(&mut a, &mut fake, &[Key::Up, Key::Up, Key::Up]);
    assert_eq!(a.selected, 0);
    assert!(shown(&a).contains("▶ local"));
}

/// UC: UC-CLI-029
#[test]
fn an_empty_list_says_so_and_offers_n() {
    let a = App::new(ConfigFile::default(), "p".into(), None);
    let text = shown(&a);
    assert!(
        text.contains("Não há perfis") && text.contains("n para criar"),
        "{text}"
    );
}

/// UC: UC-CLI-029
#[test]
fn q_escape_and_ctrl_c_leave_the_screen() {
    for key in [Key::Char('q'), Key::Esc, Key::Ctrl('c'), Key::Ctrl('d')] {
        let mut a = app();
        assert_eq!(a.handle(key, &mut Fake::default()), Flow::Quit, "{key:?}");
    }
}

/// UC: UC-CLI-029
#[test]
fn d_makes_the_selected_profile_the_default_and_saves() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Down, Key::Char('d')]);
    assert_eq!(fake.saved.len(), 1);
    assert_eq!(fake.saved[0].default.as_deref(), Some("homologacao"));
    assert!(shown(&a).contains("`homologacao` é o perfil padrão."));
}

/// UC: UC-CLI-033
#[test]
fn removing_asks_first_and_only_s_removes() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Down, Key::Delete]);
    assert!(shown(&a).contains("Remover \"homologacao\"? [s/N]"));
    press(&mut a, &mut fake, &[Key::Enter]);
    assert!(fake.saved.is_empty());
    assert_eq!(a.file().names(), ["local", "homologacao", "producao"]);
    assert!(shown(&a).contains("Nada foi removido."));
    press(&mut a, &mut fake, &[Key::Char('r'), Key::Char('s')]);
    assert_eq!(fake.saved.last().unwrap().names(), ["local", "producao"]);
    assert_eq!(a.file().names(), ["local", "producao"]);
}

/// UC: UC-CLI-033
#[test]
fn removing_the_default_leaves_the_file_without_one() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Char('r'), Key::Char('s')]);
    assert_eq!(fake.saved[0].default, None);
}

/// UC: UC-CLI-030
#[test]
fn a_new_profile_is_created_through_the_form_and_the_first_one_becomes_the_default() {
    let mut a = App::new(ConfigFile::default(), "p".into(), None);
    let mut fake = Fake::default();
    let mut keys = vec![Key::Char('n')];
    keys.extend(typed("local"));
    keys.push(Key::Down);
    keys.extend(typed("http://10.0.0.1:8080"));
    keys.push(Key::Ctrl('s'));
    press(&mut a, &mut fake, &keys);
    let saved = fake.saved.last().unwrap();
    assert_eq!(saved.default.as_deref(), Some("local"));
    assert_eq!(
        saved.get("local").unwrap().get("url"),
        Some("http://10.0.0.1:8080")
    );
    assert!(shown(&a).contains("Perfil `local` salvo."));
    assert_eq!(a.file(), saved);
}

/// UC: UC-CLI-030
#[test]
fn a_second_profile_does_not_take_the_default_unless_the_box_is_ticked() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Char('n')]);
    press(&mut a, &mut fake, &typed("novo"));
    press(&mut a, &mut fake, &[Key::Ctrl('s')]);
    assert_eq!(fake.saved.last().unwrap().default.as_deref(), Some("local"));

    press(&mut a, &mut fake, &[Key::Char('n')]);
    press(&mut a, &mut fake, &typed("outro"));
    // Do nome até a caixa "Perfil padrão": 10 passos para baixo.
    press(&mut a, &mut fake, &[Key::Down; 10]);
    press(&mut a, &mut fake, &[Key::Char(' '), Key::Ctrl('s')]);
    assert_eq!(fake.saved.last().unwrap().default.as_deref(), Some("outro"));
}

/// UC: UC-CLI-031
#[test]
fn save_is_refused_while_a_field_is_invalid() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Char('n')]);
    press(&mut a, &mut fake, &typed("x"));
    press(&mut a, &mut fake, &[Key::Down]);
    press(&mut a, &mut fake, &typed("sem-esquema"));
    press(&mut a, &mut fake, &[Key::Ctrl('s')]);
    assert!(fake.saved.is_empty());
    let text = shown(&a);
    assert!(
        text.contains("Corrija os campos em vermelho antes de salvar."),
        "{text}"
    );
    assert!(text.contains("http://"), "{text}");
}

/// UC: UC-CLI-030
#[test]
fn editing_an_existing_profile_changes_it_in_place_and_a_rename_keeps_the_default() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Enter]);
    assert!(shown(&a).contains("Perfil  local"));
    // Renomeia: apaga o nome e digita outro.
    press(&mut a, &mut fake, &[Key::Ctrl('u')]);
    press(&mut a, &mut fake, &typed("principal"));
    press(&mut a, &mut fake, &[Key::Ctrl('s')]);
    let saved = fake.saved.last().unwrap();
    assert_eq!(saved.names(), ["principal", "homologacao", "producao"]);
    assert_eq!(saved.default.as_deref(), Some("principal"));
    assert_eq!(
        saved.get("principal").unwrap().get("url"),
        Some("http://127.0.0.1:8080")
    );
}

/// UC: UC-CLI-030
#[test]
fn unticking_the_box_of_the_default_profile_removes_the_default() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Enter]);
    press(&mut a, &mut fake, &[Key::Down; 10]);
    press(&mut a, &mut fake, &[Key::Char(' '), Key::Ctrl('s')]);
    assert_eq!(fake.saved.last().unwrap().default, None);
}

/// UC: UC-CLI-032
#[test]
fn test_connection_uses_the_unsaved_values_and_only_reports() {
    let mut a = app();
    let mut fake = Fake {
        check_result: Some(Ok("ok: http://x:1 respondeu em 12 ms".to_owned())),
        ..Fake::default()
    };
    press(&mut a, &mut fake, &[Key::Char('n')]);
    press(&mut a, &mut fake, &typed("t"));
    press(&mut a, &mut fake, &[Key::Down]);
    press(&mut a, &mut fake, &typed("http://x:1"));
    // Até o botão Testar: do campo url (1) ao botão (12).
    press(&mut a, &mut fake, &[Key::Down; 11]);
    press(&mut a, &mut fake, &[Key::Enter]);
    assert_eq!(fake.checks.len(), 1);
    assert_eq!(fake.checks[0].get("url"), Some("http://x:1"));
    assert!(fake.saved.is_empty());
    assert!(shown(&a).contains("ok: http://x:1 respondeu em 12 ms"));

    fake.check_result = Some(Err("não conectou".to_owned()));
    press(&mut a, &mut fake, &[Key::Enter]);
    assert!(shown(&a).contains("não conectou"));
    // O teste falhar não impede de salvar.
    press(&mut a, &mut fake, &[Key::Ctrl('s')]);
    assert_eq!(fake.saved.len(), 1);
}

/// UC: UC-CLI-032
#[test]
fn testing_is_refused_while_the_form_has_errors() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Char('n'), Key::Down, Key::Down]);
    press(&mut a, &mut fake, &typed("nope"));
    press(&mut a, &mut fake, &[Key::Down; 10]);
    press(&mut a, &mut fake, &[Key::Enter]);
    assert!(fake.checks.is_empty());
    assert!(shown(&a).contains("Corrija os campos em vermelho antes de testar."));
}

/// UC: UC-CLI-033
#[test]
fn an_unsaved_form_is_discarded_on_a_second_escape() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Char('n')]);
    press(&mut a, &mut fake, &typed("x"));
    press(&mut a, &mut fake, &[Key::Esc]);
    assert!(shown(&a).contains("Há alterações não salvas"));
    press(&mut a, &mut fake, &[Key::Esc]);
    assert!(shown(&a).contains("Perfis do taba-cli"));
    assert!(fake.saved.is_empty());
    assert_eq!(a.file().names().len(), 3);
}

/// UC: UC-CLI-033
#[test]
fn ctrl_c_quits_from_the_form_too() {
    let mut a = app();
    let mut fake = Fake::default();
    press(&mut a, &mut fake, &[Key::Char('n')]);
    assert_eq!(a.handle(Key::Ctrl('c'), &mut fake), Flow::Quit);
}

/// UC: UC-CLI-033
#[test]
fn a_failed_write_keeps_the_previous_state_and_shows_the_error() {
    let mut a = app();
    let mut fake = Fake {
        fail_save: Some("disco cheio".to_owned()),
        ..Fake::default()
    };
    press(&mut a, &mut fake, &[Key::Char('r'), Key::Char('s')]);
    assert_eq!(a.file().names().len(), 3);
    assert!(shown(&a).contains("disco cheio"));

    press(&mut a, &mut fake, &[Key::Char('n')]);
    press(&mut a, &mut fake, &typed("x"));
    press(&mut a, &mut fake, &[Key::Ctrl('s')]);
    assert!(shown(&a).contains("disco cheio") && shown(&a).contains("Novo perfil"));
}

/// UC: UC-CLI-035
#[test]
fn a_terminal_smaller_than_60_by_16_shows_a_warning_instead_of_a_broken_screen() {
    let a = app();
    for (w, h) in [(59, 30), (80, 15), (10, 5)] {
        let text = a.render(Style { color: false }, w, h).lines.join("\n");
        assert!(
            text.contains("Terminal pequeno demais") && text.contains("60 colunas por 16 linhas"),
            "{w}x{h}: {text}"
        );
        assert!(!text.contains("Perfis do taba-cli"));
    }
    assert!(a
        .render(Style { color: false }, 60, 16)
        .lines
        .join("\n")
        .contains("Perfis do taba-cli"));
}

/// UC: UC-CLI-035
#[test]
fn colors_are_only_emitted_when_asked_for() {
    let a = app();
    let plain = a.render(Style { color: false }, 100, 30).lines.join("\n");
    let colored = a.render(Style { color: true }, 100, 30).lines.join("\n");
    assert!(!plain.contains('\x1b'));
    assert!(colored.contains("\x1b[1m") && colored.contains("\x1b[0m"));
}
