//! The state of the whole interface (doc 34, section 4): everything the screens show, with no input or output.
//! `update` changes it and `view` only reads it, so both are tested without a terminal or a network.

use taba_client::json::Fields;
use taba_client::Error;

use crate::profile::ConfigFile;
use crate::tui::api::{
    ApiCall, Backup, ClusterStatus, Drain, FlushReport, Me, Nodes, Severity, StopReport, Token,
};
use crate::tui::editor::Editor;
use crate::tui::history::History;
use crate::tui::i18n::{self, Lang};
use crate::tui::prom::Metrics;
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Cluster,
    Topics,
    Tokens,
    Messages,
    Profiles,
}

pub const TABS: [Tab; 5] = [
    Tab::Cluster,
    Tab::Topics,
    Tab::Messages,
    Tab::Tokens,
    Tab::Profiles,
];

impl Tab {
    pub fn index(self) -> usize {
        TABS.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn name_key(self) -> &'static str {
        match self {
            Tab::Cluster => "tab.cluster",
            Tab::Topics => "tab.topics",
            Tab::Tokens => "tab.tokens",
            Tab::Messages => "tab.messages",
            Tab::Profiles => "tab.profiles",
        }
    }
}

/// What the person set at the start: language, look, profile and history.
#[derive(Debug, Clone)]
pub struct Options {
    pub lang: Lang,
    pub theme: Theme,
    pub profile: String,
    pub url: String,
    pub retention_secs: u64,
    pub interval_secs: u64,
    /// The profiles file as it was read: the Profiles tab and the choice of a profile start from it.
    pub profiles: ConfigFile,
    /// Ask which profile to use before connecting (none was given and there is more than one).
    pub choose_profile: bool,
}

/// A node id for tables and messages: its start and its end, since the ids of nodes made together share the
/// start (they are GUID v7, ordered by time).
pub fn short_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 12 {
        return id.to_owned();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 6..].iter().collect();
    format!("{head}…{tail}")
}

/// The part of a node id that is safe and distinct in a file name: its last eight letters and digits.
pub fn file_tag(id: &str) -> String {
    let kept: Vec<char> = id.chars().filter(char::is_ascii_alphanumeric).collect();
    kept[kept.len().saturating_sub(8)..].iter().collect()
}

/// A failed call, reduced to what the screens show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fail {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
    pub retry_after: Option<u64>,
    /// `root` or the permission that was missing (`403 auth.forbidden`).
    pub required: Option<String>,
}

impl Fail {
    /// The client could not reach the server (no HTTP status).
    pub fn is_offline(&self) -> bool {
        self.status == 0
    }

    pub fn is_denied(&self) -> bool {
        self.status == 401 || self.status == 403
    }
}

impl From<&Error> for Fail {
    fn from(e: &Error) -> Fail {
        let body = String::from_utf8_lossy(&e.body);
        let required = Fields::parse(&body).ok().and_then(|f| f.string("required"));
        Fail {
            status: e.status,
            code: e.code.clone(),
            message: e.message.clone(),
            request_id: e.request_id.clone(),
            retry_after: e.retry_after,
            required,
        }
    }
}

/// The longest wait between two tries of a read that keeps failing.
const MAX_BACKOFF_MS: u64 = 30_000;

/// A thing read on a schedule: the last good value, the error of the last try, and when to try again.
/// A failure keeps the value (it becomes *stale*) and backs off (×2 up to 30 s; `Retry-After` is obeyed).
#[derive(Debug, Clone, PartialEq)]
pub struct Feed<T> {
    pub data: Option<T>,
    /// When the last good value arrived (ms since the app opened).
    pub fetched_at: Option<u64>,
    pub error: Option<Fail>,
    pub in_flight: Option<u64>,
    pub next_at: u64,
    pub failures: u32,
}

impl<T> Default for Feed<T> {
    fn default() -> Feed<T> {
        Feed {
            data: None,
            fetched_at: None,
            error: None,
            in_flight: None,
            next_at: 0,
            failures: 0,
        }
    }
}

impl<T> Feed<T> {
    pub fn due(&self, now: u64) -> bool {
        self.in_flight.is_none() && now >= self.next_at
    }

    pub fn ok(&mut self, now: u64, data: T, every_ms: u64) {
        self.data = Some(data);
        self.fetched_at = Some(now);
        self.error = None;
        self.in_flight = None;
        self.failures = 0;
        self.next_at = now + every_ms;
    }

    pub fn fail(&mut self, now: u64, fail: Fail, every_ms: u64) {
        self.failures = self.failures.saturating_add(1);
        let backoff = every_ms
            .saturating_mul(1 << self.failures.min(5))
            .min(MAX_BACKOFF_MS);
        self.next_at = now
            + match fail.retry_after {
                Some(secs) => secs.saturating_mul(1000),
                None => backoff,
            };
        self.error = Some(fail);
        self.in_flight = None;
    }

    /// Old data kept after a failure.
    pub fn is_stale(&self) -> bool {
        self.data.is_some() && self.error.is_some()
    }

    /// Seconds until the next try after a failure.
    pub fn retry_in(&self, now: u64) -> Option<u64> {
        self.error.as_ref()?;
        Some(self.next_at.saturating_sub(now).div_ceil(1000))
    }

    /// Asks for a read at the next tick (`r`).
    pub fn refresh_now(&mut self) {
        self.next_at = 0;
    }

    pub fn reset(&mut self) {
        *self = Feed::default();
    }
}

/// An action of a node or the cluster that asks for confirmation first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Drain(String),
    Return(String),
    FlushNode(String),
    FlushAll,
    Stop,
    /// The id and the name of the token.
    RevokeToken(String, String),
    RemoveProfile(String),
}

impl Pending {
    /// The API call the action makes; removing a profile only touches the file, so it has none.
    pub fn call(&self) -> Option<ApiCall> {
        Some(match self {
            Pending::Drain(id) => ApiCall::DrainStart(id.clone()),
            Pending::Return(id) => ApiCall::DrainCancel(id.clone()),
            Pending::FlushNode(id) => ApiCall::FlushNode(id.clone()),
            Pending::FlushAll => ApiCall::FlushCluster,
            Pending::Stop => ApiCall::StopCluster,
            Pending::RevokeToken(id, _) => ApiCall::TokenRevoke(id.clone()),
            Pending::RemoveProfile(_) => return None,
        })
    }
}

/// A popup that asks to be typed to proceed (stopping the cluster).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    pub expected: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub pending: Pending,
    pub typed: Option<Typed>,
    /// The default is always **Cancel** (TUI-FRM-07).
    pub ok_focused: bool,
}

/// The result of an action, shown in a popup.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    Flush(FlushReport),
    Stop(StopReport),
    Failure { call: ApiCall, fail: Fail },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPurpose {
    ExportCsv {
        node: String,
        series: usize,
    },
    /// The part of a token name the Tokens tab filters by.
    TokenFilter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub purpose: InputPurpose,
    pub value: String,
}

/// One popup at a time (TUI-FRM-07); `Esc` closes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Modal {
    Help {
        scroll: u16,
    },
    Palette {
        query: String,
        selected: usize,
    },
    Language {
        selected: usize,
    },
    Confirm(Confirm),
    Report(Report),
    Input(Input),
    /// What a condition means and what to do (`Enter` on a condition).
    Condition(String),
    /// Which profile to connect with.
    Profiles {
        selected: usize,
    },
    /// A form: a token or a profile.
    Editor(Editor),
    /// The secret of a token that was just created.
    Secret(Secret),
}

/// The secret of a new token, shown once (TUI-TOK-03). It lives only in this popup: never in a notice, a log or an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret {
    pub id: String,
    pub name: String,
    pub token: String,
    /// When it was shown (ms since the app opened); after 30 s it is masked.
    pub shown_at: u64,
    pub copied: bool,
    pub saved: bool,
    /// The person tried to close it without copying or saving it.
    pub warned: bool,
    /// The token this one replaces (a clone), and whether it is to be revoked on closing (default: no).
    pub replaces: Option<(String, String)>,
    pub revoke_old: bool,
}

/// A short message in the footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub severity: Severity,
    pub text: String,
    pub until: u64,
}

/// What the drain watch is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Drained,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainWatch {
    pub node: String,
    pub goal: Goal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterView {
    List,
    Detail,
    Monitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Conditions,
    Nodes,
}

/// The windows of a chart, in minutes (TUI-MON-03).
pub const WINDOWS_MIN: [u64; 5] = [1, 5, 15, 30, 60];

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// The chart being shown, an index of `screens::cluster::MON_CHARTS`.
    pub chart: usize,
    pub window: usize,
    /// The instant the chart was frozen at (`Space`).
    pub frozen: Option<u64>,
    /// `true`: the scale starts at zero and ends at the largest value ever seen; `false`: it follows the window.
    pub fixed_scale: bool,
}

impl Default for Monitor {
    fn default() -> Monitor {
        Monitor {
            chart: 0,
            window: 1,
            frozen: None,
            fixed_scale: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClusterScreen {
    pub view: ClusterView,
    pub pane: Pane,
    pub condition: usize,
    /// The selected node; kept by id so a reordering does not move the selection.
    pub node: Option<String>,
    pub scroll: u16,
    pub monitor: Monitor,
    pub nodes: Feed<Nodes>,
    pub metrics: Feed<Metrics>,
    pub backup: Feed<Option<Backup>>,
    pub drain: Feed<Drain>,
    pub watch: Option<DrainWatch>,
}

impl Default for ClusterScreen {
    fn default() -> ClusterScreen {
        ClusterScreen {
            view: ClusterView::List,
            pane: Pane::Nodes,
            condition: 0,
            node: None,
            scroll: 0,
            monitor: Monitor::default(),
            nodes: Feed::default(),
            metrics: Feed::default(),
            backup: Feed::default(),
            drain: Feed::default(),
            watch: None,
        }
    }
}

/// How the tokens are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSort {
    NewestFirst,
    OldestFirst,
    Name,
    Id,
}

impl TokenSort {
    pub fn next(self) -> TokenSort {
        match self {
            TokenSort::NewestFirst => TokenSort::OldestFirst,
            TokenSort::OldestFirst => TokenSort::Name,
            TokenSort::Name => TokenSort::Id,
            TokenSort::Id => TokenSort::NewestFirst,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TokenSort::NewestFirst => "sort.newest",
            TokenSort::OldestFirst => "sort.oldest",
            TokenSort::Name => "sort.name",
            TokenSort::Id => "sort.id",
        }
    }
}

/// The Tokens tab (doc 34, section 6.5).
#[derive(Debug, Clone, PartialEq)]
pub struct TokensScreen {
    /// Every token the server has, read page by page.
    pub list: Feed<Vec<Token>>,
    /// The tokens of the pages read so far while a read still goes on.
    pub reading: Vec<Token>,
    pub pages: u32,
    /// The selected token, by id.
    pub selected: Option<String>,
    pub filter: String,
    pub sort: TokenSort,
}

impl Default for TokensScreen {
    fn default() -> TokensScreen {
        TokensScreen {
            list: Feed::default(),
            reading: Vec::new(),
            pages: 0,
            selected: None,
            filter: String::new(),
            sort: TokenSort::NewestFirst,
        }
    }
}

/// The Profiles tab (doc 34, section 6.7): the profiles file and the selected profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfilesScreen {
    pub file: ConfigFile,
    pub selected: Option<String>,
}

pub struct Model {
    pub lang: Lang,
    pub theme: Theme,
    pub profile: String,
    pub url: String,
    pub tab: Tab,
    pub size: (u16, u16),
    /// Milliseconds since the app opened, from the last tick.
    pub now: u64,
    /// The wall clock at the last tick (ms since the epoch, UTC).
    pub wall_ms: i64,
    pub paused: bool,
    /// `false` while the terminal does not have the focus.
    pub focused: bool,
    pub quit: bool,
    pub modal: Option<Modal>,
    pub notice: Option<Notice>,
    /// An action that is running (its reply has not come).
    pub busy: Option<ApiCall>,
    pub me: Feed<Me>,
    pub status: Feed<ClusterStatus>,
    /// `GET /cluster/status` answered `401`/`403` for this token: the screen falls back to `GET /nodes`.
    pub status_denied: Option<Fail>,
    pub cluster: ClusterScreen,
    pub tokens: TokensScreen,
    pub profiles: ProfilesScreen,
    /// Waiting for the person to choose a profile: nothing is read until then.
    pub awaiting_profile: bool,
    pub history: History,
    pub next_id: u64,
    /// Bumped when the screens are reset; a reply of an older generation is dropped.
    pub generation: u64,
}

impl Model {
    pub fn new(options: Options) -> Model {
        // With one profile (or none) there is nothing to choose.
        let choose = options.choose_profile && options.profiles.profiles.len() > 1;
        let first = choose.then(|| {
            let file = &options.profiles;
            let selected = file
                .default
                .as_ref()
                .and_then(|name| file.profiles.iter().position(|(n, _)| n == name))
                .unwrap_or(0);
            Modal::Profiles { selected }
        });
        Model {
            lang: options.lang,
            theme: options.theme,
            profile: options.profile,
            url: options.url,
            tab: Tab::Cluster,
            size: (80, 24),
            now: 0,
            wall_ms: 0,
            paused: false,
            focused: true,
            quit: false,
            modal: first,
            notice: None,
            busy: None,
            me: Feed::default(),
            status: Feed::default(),
            status_denied: None,
            cluster: ClusterScreen::default(),
            tokens: TokensScreen::default(),
            profiles: ProfilesScreen {
                selected: options.profiles.default.clone(),
                file: options.profiles,
            },
            awaiting_profile: choose,
            history: History::new(options.retention_secs, options.interval_secs),
            next_id: 1,
            generation: 0,
        }
    }

    /// Starts over with another profile: what was read from the old server is dropped, and so is any reply still
    /// on its way (its generation is old).
    pub fn reconnect(&mut self, profile: String, url: String) {
        self.profile = profile;
        self.url = url;
        self.generation += 1;
        self.awaiting_profile = false;
        self.busy = None;
        self.me = Feed::default();
        self.status = Feed::default();
        self.status_denied = None;
        self.cluster = ClusterScreen::default();
        self.tokens = TokensScreen::default();
        let retention = self.history.retention_ms() / 1000;
        let interval = self.history.interval_ms() / 1000;
        self.history = History::new(retention, interval);
    }

    pub fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn tx(&self) -> Tx {
        Tx(self.lang)
    }

    /// `true` when this token may read the cluster's detail (root, or no authentication).
    pub fn is_root(&self) -> bool {
        self.me.data.as_ref().is_some_and(Me::is_root)
    }

    /// The cluster-wide status is read with this token.
    pub fn uses_status(&self) -> bool {
        self.is_root() && self.status_denied.is_none()
    }

    /// Any feed that ended in a connection failure: the server cannot be reached.
    pub fn is_offline(&self) -> bool {
        let feeds = [
            self.me.error.as_ref(),
            self.status.error.as_ref(),
            self.cluster.nodes.error.as_ref(),
        ];
        feeds.into_iter().flatten().any(Fail::is_offline)
    }

    /// The worst condition for the status line: its severity and the word (a condition name, or a state key).
    pub fn health(&self) -> (Severity, String) {
        if self.is_offline() {
            return (Severity::Critical, "offline".to_owned());
        }
        if let Some(status) = &self.status.data {
            let severity = status
                .conditions
                .iter()
                .map(|c| c.severity)
                .max()
                .unwrap_or(Severity::Ok);
            let word = if status.worst.is_empty() {
                "ok"
            } else {
                &status.worst
            };
            return (severity, word.to_owned());
        }
        if let Some(nodes) = &self.cluster.nodes.data {
            let bad = nodes
                .items
                .iter()
                .any(|n| !n.reachable || !matches!(n.state.as_str(), "active" | ""));
            if bad || nodes.partial {
                return (Severity::Warning, "degraded".to_owned());
            }
            return (Severity::Ok, "ok".to_owned());
        }
        (Severity::Ok, "unknown".to_owned())
    }

    pub fn notify(&mut self, severity: Severity, text: String) {
        self.notice = Some(Notice {
            severity,
            text,
            until: self.now + 5000,
        });
    }
}

/// The catalog in a language: the views and the update ask it for their texts.
#[derive(Debug, Clone, Copy)]
pub struct Tx(pub Lang);

impl Tx {
    pub fn t(self, key: &str) -> String {
        i18n::tr(self.0, key)
    }

    pub fn tw(self, key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
        i18n::tr_with(self.0, key, args)
    }

    pub fn tn(self, key: &str, n: u64) -> String {
        i18n::tr_n(self.0, key, n)
    }
}

#[cfg(test)]
mod tests;
