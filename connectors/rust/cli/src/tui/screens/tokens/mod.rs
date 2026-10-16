//! The Tokens tab (doc 34, section 6.5): the tokens of the server, creating, editing, cloning and revoking them.
//! This file is the state and the keys; `view.rs` draws it. The secret of a new token never leaves the popup that
//! shows it (TUI-TOK-03).

use taba_client::json;

use crate::tui::api::{ApiCall, Severity, Token};
use crate::tui::editor::{Editor, Field, Outcome, Purpose};
use crate::tui::effect::Effect;
use crate::tui::i18n;
use crate::tui::keymap::Action;
use crate::tui::model::{
    Confirm, Fail, Input, InputPurpose, Modal, Model, Pending, Secret, TokenSort,
};
use crate::tui::screens::cluster::moved;
use crate::tui::update::run;

pub mod view;

/// The permissions a token can have, in the order the form lists them.
pub const PERMISSIONS: [&str; 5] = ["publish", "consume", "create", "manage", "admin"];

/// After this long the secret on the screen is masked (TUI-TOK-03), in milliseconds.
pub const MASK_AFTER_MS: u64 = 30_000;

/// A token that expires within this many days is shown in yellow (TUI-TOK-01).
pub const SOON_DAYS: i64 = 7;

/// How many pages of tokens one read follows before it gives up (200 tokens each).
pub const MAX_PAGES: u32 = 25;

/// What the tab can do with this token and this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// `GET /auth/me` has not answered yet.
    Unknown,
    /// The server runs without authentication: there are no tokens (`404`).
    NoAuth,
    /// Only the root token and the `admin` tokens manage tokens.
    NotAdmin,
    Ready,
}

pub fn availability(m: &Model) -> Availability {
    match &m.me.data {
        None => Availability::Unknown,
        Some(me) if !me.auth_enabled => Availability::NoAuth,
        Some(me) if !me.is_admin() => Availability::NotAdmin,
        Some(_) => Availability::Ready,
    }
}

/// The tokens shown: those whose name has the filter in it, in the chosen order.
pub fn visible(m: &Model) -> Vec<Token> {
    let wanted = m.tokens.filter.to_lowercase();
    let mut tokens: Vec<Token> = m
        .tokens
        .list
        .data
        .iter()
        .flatten()
        .filter(|t| wanted.is_empty() || t.name.to_lowercase().contains(&wanted))
        .cloned()
        .collect();
    match m.tokens.sort {
        TokenSort::NewestFirst => tokens.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        TokenSort::OldestFirst => tokens.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
        TokenSort::Name => tokens.sort_by_key(|t| t.name.to_lowercase()),
        TokenSort::Id => tokens.sort_by(|a, b| a.id.cmp(&b.id)),
    }
    tokens
}

/// The index of the selected token in `tokens`; the first one if the selection is gone.
pub fn selected_index(m: &Model, tokens: &[Token]) -> Option<usize> {
    if tokens.is_empty() {
        return None;
    }
    let found = m
        .tokens
        .selected
        .as_ref()
        .and_then(|id| tokens.iter().position(|t| &t.id == id));
    Some(found.unwrap_or(0))
}

pub fn selected(m: &Model) -> Option<Token> {
    let tokens = visible(m);
    selected_index(m, &tokens).map(|i| tokens[i].clone())
}

/// Why an action cannot run now, as a catalog key; `None` means it can (TUI-FRM-10).
pub fn gate(m: &Model, action: Action) -> Option<&'static str> {
    match availability(m) {
        Availability::Unknown => return Some("gate.tokens-unknown"),
        Availability::NoAuth => return Some("gate.tokens-no-auth"),
        Availability::NotAdmin => return Some("gate.not-admin"),
        Availability::Ready => {}
    }
    if m.busy.is_some() {
        return Some("gate.busy");
    }
    match action {
        Action::TokenEdit | Action::TokenClone | Action::TokenRevoke if selected(m).is_none() => {
            Some("gate.no-token")
        }
        // An `admin` token is only changed and revoked by the root token (the server answers `403` otherwise).
        Action::TokenEdit | Action::TokenRevoke
            if !m.is_root()
                && selected(m).is_some_and(|t| t.permissions.iter().any(|p| p == "admin")) =>
        {
            Some("gate.root-for-admin")
        }
        _ => None,
    }
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
        Action::Sort => {
            m.tokens.sort = m.tokens.sort.next();
            Vec::new()
        }
        Action::Filter => {
            m.modal = Some(Modal::Input(Input {
                purpose: InputPurpose::TokenFilter,
                value: m.tokens.filter.clone(),
            }));
            Vec::new()
        }
        Action::TokenNew | Action::TokenEdit | Action::TokenClone | Action::TokenRevoke => {
            if let Some(key) = gate(m, action) {
                return refuse(m, key);
            }
            match action {
                Action::TokenNew => open_create(m, None),
                Action::TokenEdit => open_edit(m),
                Action::TokenClone => open_create(m, selected(m)),
                _ => open_revoke(m),
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn step(m: &mut Model, delta: i32) -> Vec<Effect> {
    let tokens = visible(m);
    if let Some(current) = selected_index(m, &tokens) {
        let next = moved(current, delta, tokens.len());
        m.tokens.selected = Some(tokens[next].id.clone());
    }
    Vec::new()
}

fn open_create(m: &mut Model, from: Option<Token>) {
    let (name, prefixes, perms, clone_of) = match &from {
        Some(t) => (
            format!("{}-copy", t.name),
            t.prefixes.join(" "),
            t.permissions.clone(),
            Some((t.id.clone(), t.name.clone())),
        ),
        None => (String::new(), String::new(), Vec::new(), None),
    };
    let mut fields = vec![Field::text("name", "editor.field.name", &name)];
    for permission in PERMISSIONS {
        let mut field = Field::check(
            permission,
            permission_label(permission),
            perms.iter().any(|p| p == permission) && (permission != "admin" || m.is_root()),
        );
        // Only the root token creates an `admin` token.
        if permission == "admin" && !m.is_root() {
            field = field.locked("editor.locked.root");
        }
        fields.push(field);
    }
    fields.push(Field::text("prefixes", "editor.field.prefixes", &prefixes));
    fields.push(Field::text("expires", "editor.field.expires", ""));
    m.modal = Some(Modal::Editor(Editor::new(
        Purpose::TokenCreate { clone_of },
        fields,
    )));
}

fn permission_label(permission: &str) -> &'static str {
    match permission {
        "publish" => "editor.perm.publish",
        "consume" => "editor.perm.consume",
        "create" => "editor.perm.create",
        "manage" => "editor.perm.manage",
        _ => "editor.perm.admin",
    }
}

fn open_edit(m: &mut Model) {
    let Some(token) = selected(m) else { return };
    let expires = token.expires_at.clone().unwrap_or_default();
    let mut fields = vec![Field::text("name", "editor.field.name", &token.name)];
    for permission in PERMISSIONS {
        fields.push(
            Field::check(
                permission,
                permission_label(permission),
                token.permissions.iter().any(|p| p == permission),
            )
            .locked("editor.locked.clone"),
        );
    }
    fields.push(
        Field::text(
            "prefixes",
            "editor.field.prefixes",
            &token.prefixes.join(" "),
        )
        .locked("editor.locked.clone"),
    );
    fields.push(Field::text("expires", "editor.field.expires", &expires));
    m.modal = Some(Modal::Editor(Editor::new(
        Purpose::TokenEdit {
            id: token.id,
            name: token.name,
            expires,
        },
        fields,
    )));
}

fn open_revoke(m: &mut Model) {
    if let Some(token) = selected(m) {
        m.modal = Some(Modal::Confirm(Confirm {
            pending: Pending::RevokeToken(token.id, token.name),
            typed: None,
            ok_focused: false,
        }));
    }
}

/// `2026-10-15T12:00:00Z` for a time in milliseconds since the epoch.
pub fn rfc3339(ms: i64) -> String {
    let c = i18n::civil_from_ms(ms);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

/// The text typed as an expiry is not one of the forms `parse_expiry` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadExpiry;

/// What a person types as an expiry: nothing, `none` or `never` (no deadline), a time from now (`90m`, `12h`,
/// `30d`), a date (`2027-01-31`, the start of that day in UTC) or a full RFC 3339 instant. `Ok(None)` is no
/// deadline.
pub fn parse_expiry(text: &str, now_ms: i64) -> Result<Option<String>, BadExpiry> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") || text.eq_ignore_ascii_case("never") {
        return Ok(None);
    }
    let relative = text.char_indices().last().and_then(|(at, unit)| {
        let number = text[..at].parse::<i64>().ok().filter(|n| *n > 0)?;
        let step = match unit {
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => return None,
        };
        number.checked_mul(step)
    });
    if let Some(span) = relative {
        return Ok(Some(rfc3339(now_ms.saturating_add(span))));
    }
    let instant = if text.len() == 10 {
        format!("{text}T00:00:00Z")
    } else {
        text.to_owned()
    };
    i18n::parse_instant(&instant).ok_or(BadExpiry)?;
    Ok(Some(instant))
}

fn quoted_list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| json::quote(s)).collect();
    format!("[{}]", quoted.join(","))
}

/// The form was filled in and the person asked to save it.
pub fn submit(m: &mut Model, mut editor: Editor) -> Vec<Effect> {
    let tx = m.tx();
    let fail = |editor: &mut Editor, field: &str, key: &str, m: &mut Model| {
        editor.error = Some(m.tx().t(key));
        editor.focus_on(field);
        m.modal = Some(Modal::Editor(editor.clone()));
    };
    let name = editor.text("name").to_owned();
    if name.is_empty() {
        fail(&mut editor, "name", "editor.error.name", m);
        return Vec::new();
    }
    let call = match editor.purpose.clone() {
        Purpose::TokenCreate { .. } => {
            let permissions: Vec<String> = PERMISSIONS
                .iter()
                .filter(|p| editor.checked(p))
                .map(|p| (*p).to_owned())
                .collect();
            if permissions.is_empty() {
                fail(&mut editor, "publish", "editor.error.permissions", m);
                return Vec::new();
            }
            let expires = match parse_expiry(editor.text("expires"), m.wall_ms) {
                Ok(expires) => expires,
                Err(BadExpiry) => {
                    fail(&mut editor, "expires", "editor.error.expiry", m);
                    return Vec::new();
                }
            };
            let prefixes: Vec<String> = editor
                .text("prefixes")
                .split([',', ' '])
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
            let mut body = vec![
                format!("\"name\":{}", json::quote(&name)),
                format!("\"permissions\":{}", quoted_list(&permissions)),
            ];
            if !prefixes.is_empty() {
                body.push(format!("\"topicPrefixes\":{}", quoted_list(&prefixes)));
            }
            if let Some(at) = expires {
                body.push(format!("\"expiresAt\":{}", json::quote(&at)));
            }
            ApiCall::TokenCreate(format!("{{{}}}", body.join(",")))
        }
        Purpose::TokenEdit {
            id,
            name: was_name,
            expires: was_expires,
        } => {
            let mut body = Vec::new();
            if name != was_name {
                body.push(format!("\"name\":{}", json::quote(&name)));
            }
            if editor.text("expires") != was_expires.trim() {
                match parse_expiry(editor.text("expires"), m.wall_ms) {
                    Ok(Some(at)) => body.push(format!("\"expiresAt\":{}", json::quote(&at))),
                    Ok(None) => body.push("\"expiresAt\":null".to_owned()),
                    Err(BadExpiry) => {
                        fail(&mut editor, "expires", "editor.error.expiry", m);
                        return Vec::new();
                    }
                }
            }
            if body.is_empty() {
                editor.error = Some(tx.t("editor.error.nothing"));
                m.modal = Some(Modal::Editor(editor));
                return Vec::new();
            }
            ApiCall::TokenUpdate(id, format!("{{{}}}", body.join(",")))
        }
        _ => return Vec::new(),
    };
    editor.busy = true;
    editor.error = None;
    m.modal = Some(Modal::Editor(editor));
    run(m, call)
}

/// The keys of the form of a token.
pub fn editor_key(m: &mut Model, mut editor: Editor, outcome: Outcome) -> Vec<Effect> {
    match outcome {
        Outcome::Cancel => {
            m.modal = None;
            Vec::new()
        }
        Outcome::Submit => submit(m, editor),
        Outcome::Test | Outcome::None => {
            editor.note = None;
            m.modal = Some(Modal::Editor(editor));
            Vec::new()
        }
    }
}

/// The server answered the form: the secret of a new token, or the change of a token.
pub fn saved(m: &mut Model, call: &ApiCall, text: &str) -> Vec<Effect> {
    let tx = m.tx();
    let editor = match m.modal.take() {
        Some(Modal::Editor(editor)) => Some(editor),
        other => {
            m.modal = other;
            None
        }
    };
    m.tokens.list.refresh_now();
    match call {
        ApiCall::TokenCreate(_) => match crate::tui::api::parse_token_created(text) {
            Ok((token, secret)) => {
                let replaces = match editor.map(|e| e.purpose) {
                    Some(Purpose::TokenCreate { clone_of }) => clone_of,
                    _ => None,
                };
                m.tokens.selected = Some(token.id.clone());
                m.modal = Some(Modal::Secret(Secret {
                    id: token.id,
                    name: token.name,
                    token: secret,
                    shown_at: m.now,
                    copied: false,
                    saved: false,
                    warned: false,
                    replaces,
                    revoke_old: false,
                }));
            }
            Err(e) => {
                m.notify(
                    Severity::Warning,
                    tx.tw("notice.bad-reply", &[("error", &e)]),
                );
            }
        },
        _ => {
            let text = tx.t("notice.token-updated");
            m.notify(Severity::Ok, text);
        }
    }
    Vec::new()
}

/// The server refused the form: the reason goes in the form, which stays open (TUI-TOK-02).
pub fn refused(m: &mut Model, fail: &Fail) -> bool {
    if !matches!(m.modal, Some(Modal::Editor(_))) {
        return false;
    }
    let text = if fail.is_denied() {
        crate::tui::view::widgets::denied_text(m, fail)
    } else {
        format!("{}: {}", fail.code, fail.message)
    };
    if let Some(Modal::Editor(editor)) = &mut m.modal {
        editor.busy = false;
        editor.error = Some(text);
    }
    true
}

/// A key in the popup of the secret (TUI-TOK-03).
pub fn secret_key(
    m: &mut Model,
    mut secret: Secret,
    code: ratatui::crossterm::event::KeyCode,
) -> Vec<Effect> {
    use ratatui::crossterm::event::KeyCode;
    let tx = m.tx();
    match code {
        KeyCode::Char('c') => {
            secret.copied = true;
            let text = tx.t("notice.secret-copied");
            let effect = Effect::Copy(secret.token.clone());
            m.modal = Some(Modal::Secret(secret));
            m.notify(Severity::Ok, text);
            vec![effect]
        }
        KeyCode::Char('s') => {
            let effect = Effect::WriteSecret {
                path: secret_file(&secret.id),
                contents: format!("{}\n", secret.token),
            };
            m.modal = Some(Modal::Secret(secret));
            vec![effect]
        }
        KeyCode::Char('r') if secret.replaces.is_some() => {
            secret.revoke_old = !secret.revoke_old;
            m.modal = Some(Modal::Secret(secret));
            Vec::new()
        }
        KeyCode::Esc | KeyCode::Enter => {
            if !(secret.copied || secret.saved || secret.warned) {
                secret.warned = true;
                m.modal = Some(Modal::Secret(secret));
                return Vec::new();
            }
            m.modal = None;
            match (&secret.replaces, secret.revoke_old) {
                (Some((old, _)), true) => run(m, ApiCall::TokenRevoke(old.clone())),
                _ => Vec::new(),
            }
        }
        _ => {
            m.modal = Some(Modal::Secret(secret));
            Vec::new()
        }
    }
}

/// The file `s` writes the secret to: in the folder the interface was opened from.
pub fn secret_file(id: &str) -> String {
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    format!("taba-token-{safe}.txt")
}

/// The secret was written to a file: it counts as kept.
pub fn secret_written(m: &mut Model, path: &str) {
    if let Some(Modal::Secret(secret)) = &mut m.modal {
        if secret_file(&secret.id) == path {
            secret.saved = true;
        }
    }
}

/// A token was revoked.
pub fn revoked(m: &mut Model) {
    let text = m.tx().t("notice.token-revoked");
    m.notify(Severity::Ok, text);
    m.tokens.list.refresh_now();
}

#[cfg(test)]
mod tests;
