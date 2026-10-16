//! The Profiles tab (doc 34, section 6.7): list, create, edit, remove, choose the default, test a connection and
//! connect with another profile. It replaces the old profiles screen (D-TUI-8) and works on the same file
//! (`config.yml`), through the effects `SaveProfiles`, `CheckProfile` and `Connect`.

use crate::profile::{self, ConfigFile, Profile};
use crate::tui::api::Severity;
use crate::tui::editor::{Editor, Field, Outcome, Purpose};
use crate::tui::effect::Effect;
use crate::tui::keymap::Action;
use crate::tui::model::{Confirm, Modal, Model, Pending};
use crate::tui::msg::CheckResult;
use crate::tui::screens::cluster::moved;

pub mod view;

/// The fields of a profile the form edits; the others (`output`, the timeouts) are kept as they are.
pub const FORM_FIELDS: [(&str, &str); 5] = [
    ("url", "editor.field.url"),
    ("ops-url", "editor.field.ops-url"),
    ("token-file", "editor.field.token-file"),
    ("token", "editor.field.token"),
    ("ca-file", "editor.field.ca-file"),
];

/// The index of the selected profile; the first one if the selection is gone.
pub fn selected_index(m: &Model) -> Option<usize> {
    let profiles = &m.profiles.file.profiles;
    if profiles.is_empty() {
        return None;
    }
    let found = m
        .profiles
        .selected
        .as_ref()
        .and_then(|name| profiles.iter().position(|(n, _)| n == name));
    Some(found.unwrap_or(0))
}

pub fn selected(m: &Model) -> Option<(String, Profile)> {
    selected_index(m).map(|i| m.profiles.file.profiles[i].clone())
}

/// The index of the profile a person most likely wants: the default one, or the first.
pub fn preferred_index(file: &ConfigFile) -> usize {
    file.default
        .as_ref()
        .and_then(|name| file.profiles.iter().position(|(n, _)| n == name))
        .unwrap_or(0)
}

fn refuse(m: &mut Model, key: &str) -> Vec<Effect> {
    let text = m.tx().t(key);
    m.notify(Severity::Warning, text);
    Vec::new()
}

pub fn on_action(m: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Up => step(m, -1),
        Action::Down => step(m, 1),
        Action::PageUp => step(m, -10),
        Action::PageDown => step(m, 10),
        Action::First => step(m, i32::MIN / 2),
        Action::Last => step(m, i32::MAX / 2),
        Action::ProfileNew => {
            open_form(m, None);
            Vec::new()
        }
        Action::ProfileEdit => match selected(m) {
            Some(chosen) => {
                open_form(m, Some(chosen));
                Vec::new()
            }
            None => refuse(m, "gate.no-profile"),
        },
        Action::ProfileUse => match selected(m) {
            Some((name, _)) => vec![Effect::Connect(name)],
            None => refuse(m, "gate.no-profile"),
        },
        Action::ProfileDefault => match selected(m) {
            Some((name, _)) => {
                if m.profiles.file.default.as_deref() == Some(name.as_str()) {
                    let text = m
                        .tx()
                        .tw("notice.profile-already-default", &[("name", &name)]);
                    m.notify(Severity::Ok, text);
                    return Vec::new();
                }
                let mut next = m.profiles.file.clone();
                match next.set_default(&name) {
                    Ok(()) => vec![Effect::SaveProfiles {
                        file: next,
                        select: Some(name),
                    }],
                    Err(e) => {
                        m.notify(Severity::Critical, e);
                        Vec::new()
                    }
                }
            }
            None => refuse(m, "gate.no-profile"),
        },
        Action::ProfileRemove => match selected(m) {
            Some((name, _)) => {
                m.modal = Some(Modal::Confirm(Confirm {
                    pending: Pending::RemoveProfile(name),
                    typed: None,
                    ok_focused: false,
                }));
                Vec::new()
            }
            None => refuse(m, "gate.no-profile"),
        },
        Action::ProfileTest => match selected(m) {
            Some((_, profile)) => vec![Effect::CheckProfile(profile)],
            None => refuse(m, "gate.no-profile"),
        },
        _ => Vec::new(),
    }
}

fn step(m: &mut Model, delta: i32) -> Vec<Effect> {
    if let Some(current) = selected_index(m) {
        let next = moved(current, delta, m.profiles.file.profiles.len());
        m.profiles.selected = Some(m.profiles.file.profiles[next].0.clone());
    }
    Vec::new()
}

fn open_form(m: &mut Model, existing: Option<(String, Profile)>) {
    let (name, profile, is_default, purpose) = match existing {
        Some((name, profile)) => {
            let is_default = m.profiles.file.default.as_deref() == Some(name.as_str());
            let original = name.clone();
            (name, profile, is_default, Purpose::ProfileEdit { original })
        }
        None => (
            String::new(),
            Profile::default(),
            m.profiles.file.profiles.is_empty(),
            Purpose::ProfileNew,
        ),
    };
    let mut fields = vec![Field::text("name", "editor.field.name", &name)];
    for (id, label) in FORM_FIELDS {
        let value = profile.get(id).unwrap_or("");
        fields.push(if id == "token" {
            Field::secret(id, label, value)
        } else {
            Field::text(id, label, value)
        });
    }
    fields.push(Field::check("default", "editor.field.default", is_default));
    m.modal = Some(Modal::Editor(Editor::new(purpose, fields)));
}

/// What the form holds, as a profile: the form's fields over the profile it started from.
fn profile_of(m: &Model, editor: &Editor) -> Result<Profile, (String, String)> {
    let mut profile = match &editor.purpose {
        Purpose::ProfileEdit { original } => {
            m.profiles.file.get(original).cloned().unwrap_or_default()
        }
        _ => Profile::default(),
    };
    for (id, _) in FORM_FIELDS {
        let value = editor.text(id);
        if value.is_empty() {
            profile.unset(id);
        } else if let Err(e) = profile.set(id, value) {
            return Err((id.to_owned(), e));
        }
    }
    Ok(profile)
}

/// The keys of the form of a profile.
pub fn editor_key(m: &mut Model, mut editor: Editor, outcome: Outcome) -> Vec<Effect> {
    match outcome {
        Outcome::Cancel => {
            m.modal = None;
            Vec::new()
        }
        Outcome::Submit => submit(m, editor),
        Outcome::Test => match profile_of(m, &editor) {
            Ok(profile) => {
                editor.note = None;
                m.modal = Some(Modal::Editor(editor));
                vec![Effect::CheckProfile(profile)]
            }
            Err((field, e)) => {
                editor.focus_on(&field);
                editor.error = Some(e);
                m.modal = Some(Modal::Editor(editor));
                Vec::new()
            }
        },
        Outcome::None => {
            editor.note = None;
            m.modal = Some(Modal::Editor(editor));
            Vec::new()
        }
    }
}

fn submit(m: &mut Model, mut editor: Editor) -> Vec<Effect> {
    let name = editor.text("name").to_owned();
    let original = match &editor.purpose {
        Purpose::ProfileEdit { original } => Some(original.clone()),
        _ => None,
    };
    let result = profile::validate_name(&name)
        .map_err(|e| ("name".to_owned(), e))
        .and_then(|()| {
            if original.as_deref() != Some(name.as_str()) && m.profiles.file.get(&name).is_some() {
                Err((
                    "name".to_owned(),
                    m.tx().tw("editor.error.profile-exists", &[("name", &name)]),
                ))
            } else {
                Ok(())
            }
        })
        .and_then(|()| profile_of(m, &editor));
    let profile = match result {
        Ok(profile) => profile,
        Err((field, e)) => {
            editor.focus_on(&field);
            editor.error = Some(e);
            m.modal = Some(Modal::Editor(editor));
            return Vec::new();
        }
    };
    let mut next = m.profiles.file.clone();
    match &original {
        None => {
            if let Err(e) = next.add(&name, profile) {
                editor.error = Some(e);
                m.modal = Some(Modal::Editor(editor));
                return Vec::new();
            }
        }
        Some(old) => {
            let Some(at) = next.profiles.iter().position(|(n, _)| n == old) else {
                editor.error = Some(m.tx().tw("editor.error.profile-gone", &[("name", old)]));
                m.modal = Some(Modal::Editor(editor));
                return Vec::new();
            };
            next.profiles[at] = (name.clone(), profile);
            if next.default.as_deref() == Some(old.as_str()) {
                next.default = Some(name.clone());
            }
        }
    }
    if editor.checked("default") {
        next.default = Some(name.clone());
    } else if next.default.as_deref() == Some(name.as_str()) {
        next.default = None;
    }
    editor.busy = true;
    editor.error = None;
    m.modal = Some(Modal::Editor(editor));
    vec![Effect::SaveProfiles {
        file: next,
        select: Some(name),
    }]
}

/// Removing a profile was confirmed.
pub fn remove(m: &mut Model, name: &str) -> Vec<Effect> {
    let mut next = m.profiles.file.clone();
    match next.remove(name) {
        Ok(()) => vec![Effect::SaveProfiles {
            file: next,
            select: None,
        }],
        Err(e) => {
            m.notify(Severity::Critical, e);
            Vec::new()
        }
    }
}

/// The profiles file was written (or not).
pub fn saved(m: &mut Model, file: ConfigFile, select: Option<String>, result: Result<(), String>) {
    let tx = m.tx();
    match result {
        Ok(()) => {
            m.profiles.file = file;
            if select.is_some() {
                m.profiles.selected = select;
            }
            if matches!(m.modal, Some(Modal::Editor(_))) {
                m.modal = None;
            }
            let text = tx.t("notice.profiles-saved");
            m.notify(Severity::Ok, text);
        }
        Err(e) => {
            if let Some(Modal::Editor(editor)) = &mut m.modal {
                editor.busy = false;
                editor.error = Some(e);
            } else {
                let text = tx.tw("notice.profiles-not-saved", &[("error", &e)]);
                m.notify(Severity::Critical, text);
            }
        }
    }
}

/// The text for the result of testing a connection, and whether it is a failure.
pub fn check_text(m: &Model, result: &CheckResult) -> (bool, String) {
    let tx = m.tx();
    match result {
        CheckResult::Reachable { url, millis } => {
            (false, tx.tw("check.ok", &[("url", url), ("ms", millis)]))
        }
        CheckResult::Refused { url, status } => (
            true,
            tx.tw("check.refused", &[("url", url), ("status", status)]),
        ),
        CheckResult::Answered { url, status, code } => (
            true,
            tx.tw(
                "check.answered",
                &[("url", url), ("status", status), ("code", code)],
            ),
        ),
        CheckResult::Failed(e) => (true, tx.tw("check.failed", &[("error", e)])),
    }
}

/// The result of a connection test: in the form that asked for it, or in the footer.
pub fn checked(m: &mut Model, result: &CheckResult) {
    let (failed, text) = check_text(m, result);
    if let Some(Modal::Editor(editor)) = &mut m.modal {
        editor.note = Some((failed, text));
        return;
    }
    m.notify(
        if failed {
            Severity::Warning
        } else {
            Severity::Ok
        },
        text,
    );
}

#[cfg(test)]
mod tests;
