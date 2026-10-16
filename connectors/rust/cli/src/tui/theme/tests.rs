use super::*;

fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

/// TUI: TUI-FRM-09
#[test]
fn no_color_takes_the_colors_away_and_keeps_the_marks() {
    let theme = Theme::from_env(&env(&[("NO_COLOR", "1")]));
    assert!(!theme.color);
    assert_eq!(theme.critical().fg, None);
    assert_ne!(theme.selected(), Style::default());
    assert!(Theme::from_env(&env(&[])).color);
    assert!(Theme::from_env(&env(&[("NO_COLOR", "")])).color);
}

/// TUI: TUI-FRM-09
#[test]
fn ascii_comes_from_the_variable_or_a_locale_that_is_not_utf8() {
    assert!(Theme::from_env(&env(&[("TABA_CLI_ASCII", "1")])).ascii);
    assert!(Theme::from_env(&env(&[("LANG", "pt_BR.ISO-8859-1")])).ascii);
    assert!(!Theme::from_env(&env(&[("LANG", "pt_BR.UTF-8")])).ascii);
    assert!(!Theme::from_env(&env(&[])).ascii);
}

/// TUI: TUI-FRM-01
#[test]
fn every_severity_has_a_symbol_in_both_alphabets() {
    for severity in [Severity::Ok, Severity::Warning, Severity::Critical] {
        let fancy = Theme::default().icon(severity);
        let plain = Theme {
            ascii: true,
            ..Theme::default()
        }
        .icon(severity);
        assert!(!fancy.is_empty() && plain.is_ascii());
    }
}

#[test]
fn a_bar_fills_in_proportion_and_never_overflows() {
    let t = Theme {
        ascii: true,
        ..Theme::default()
    };
    assert_eq!(t.bar(0.5, 10), "#####-----");
    assert_eq!(t.bar(2.0, 4), "####");
    assert_eq!(t.bar(-1.0, 4), "----");
}

/// TUI: TUI-MON-02
#[test]
fn chart_markers_differ_per_line() {
    let t = Theme::default();
    assert_ne!(t.marker(0), t.marker(1));
}
