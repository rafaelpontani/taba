//! The Cluster tab (doc 34, sections 6.2 and 6.3): the conditions, the nodes, the detail of a node, its
//! charts and the actions of node and cluster. This file is the state and the keys; `view.rs` draws it.

use crate::tui::api::{ApiCall, NodeStatus, Severity};
use crate::tui::effect::Effect;
use crate::tui::history::{Point, Series};
use crate::tui::i18n;
use crate::tui::keymap::Action;
use crate::tui::model::{
    file_tag, ClusterView, Confirm, DrainWatch, Goal, Input, InputPurpose, Modal, Model, Pane,
    Pending, Typed, WINDOWS_MIN,
};

pub mod view;

/// A node of the table: the same row from `GET /cluster/status` (root) or `GET /nodes` (any token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub state: String,
    pub drain_state: String,
    pub reachable: bool,
    pub is_self: bool,
}

/// What a chart draws: a title and the lines (one or two) with their legend keys.
pub struct ChartDef {
    pub key: &'static str,
    pub title: &'static str,
    pub lines: &'static [(Series, &'static str)],
    pub unit: Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Bytes,
    Count,
    PerSecond,
    Seconds,
}

/// The series of a node that can be charted (TUI-MON-01).
pub const MON_CHARTS: &[ChartDef] = &[
    ChartDef {
        key: "memory",
        title: "chart.memory",
        lines: &[
            (Series::MemoryUsed, "chart.line.used"),
            (Series::MemoryLimit, "chart.line.limit"),
        ],
        unit: Unit::Bytes,
    },
    ChartDef {
        key: "ready",
        title: "chart.ready",
        lines: &[(Series::MemoryMessages, "chart.line.in-memory")],
        unit: Unit::Count,
    },
    ChartDef {
        key: "disk",
        title: "chart.disk",
        lines: &[(Series::DiskMessages, "chart.line.on-disk")],
        unit: Unit::Count,
    },
    ChartDef {
        key: "requests",
        title: "chart.requests",
        lines: &[(Series::ActiveRequests, "chart.line.requests")],
        unit: Unit::Count,
    },
    ChartDef {
        key: "rejected",
        title: "chart.rejected",
        lines: &[(Series::RateLimited, "chart.line.rejected")],
        unit: Unit::PerSecond,
    },
    ChartDef {
        key: "rtt",
        title: "chart.rtt",
        lines: &[(Series::ChannelRtt, "chart.line.rtt")],
        unit: Unit::Seconds,
    },
];

/// The nodes of the table, in the order the server gave them.
pub fn rows(m: &Model) -> Vec<Row> {
    if m.uses_status() {
        if let Some(status) = &m.status.data {
            let mut rows: Vec<Row> = status
                .nodes
                .iter()
                .map(|n| Row {
                    id: n.node_id.clone(),
                    state: n.state.clone(),
                    drain_state: n.drain_state.clone(),
                    reachable: n.reachable,
                    is_self: false,
                })
                .collect();
            // A drained node leaves the member table, so the status no longer lists it; `GET /nodes` still does,
            // and the table keeps it so it can be returned.
            if let Some(nodes) = &m.cluster.nodes.data {
                let missing: Vec<_> = nodes
                    .items
                    .iter()
                    .filter(|n| !rows.iter().any(|r| r.id == n.node_id))
                    .collect();
                for n in missing {
                    rows.push(Row {
                        id: n.node_id.clone(),
                        state: n.state.clone(),
                        drain_state: n.drain_state.clone(),
                        reachable: n.reachable || n.state == "drained",
                        is_self: n.is_self,
                    });
                }
            }
            return rows;
        }
    }
    m.cluster
        .nodes
        .data
        .as_ref()
        .map(|nodes| {
            nodes
                .items
                .iter()
                .map(|n| Row {
                    id: n.node_id.clone(),
                    state: n.state.clone(),
                    drain_state: n.drain_state.clone(),
                    reachable: n.reachable,
                    is_self: n.is_self,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The index of the selected node in `rows`; the first one if the selection is gone.
pub fn selected_index(m: &Model, rows: &[Row]) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let found = m
        .cluster
        .node
        .as_ref()
        .and_then(|id| rows.iter().position(|r| &r.id == id));
    Some(found.unwrap_or(0))
}

pub fn selected_row(m: &Model) -> Option<Row> {
    let rows = rows(m);
    selected_index(m, &rows).map(|i| rows[i].clone())
}

pub fn status_of<'a>(m: &'a Model, id: &str) -> Option<&'a NodeStatus> {
    m.status
        .data
        .as_ref()?
        .nodes
        .iter()
        .find(|n| n.node_id == id && n.reachable)
}

/// Why an action cannot run now; `None` means it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Needs the `admin` permission (or the root token).
    NotAdmin,
    /// Needs the root token itself (stopping the cluster).
    NotRoot,
    NoNode,
    Unreachable,
    NotActive,
    NotDraining,
    Busy,
}

impl Gate {
    pub fn key(self) -> &'static str {
        match self {
            Gate::NotAdmin => "gate.not-admin",
            Gate::NotRoot => "gate.not-root",
            Gate::NoNode => "gate.no-node",
            Gate::Unreachable => "gate.unreachable",
            Gate::NotActive => "gate.not-active",
            Gate::NotDraining => "gate.not-draining",
            Gate::Busy => "gate.busy",
        }
    }
}

/// Whether an action of the Cluster tab can run now, and why not (TUI-CLU-05, TUI-FRM-10).
pub fn gate(m: &Model, action: Action) -> Option<Gate> {
    if m.busy.is_some() {
        return Some(Gate::Busy);
    }
    if !m.is_admin() {
        return Some(Gate::NotAdmin);
    }
    if action == Action::StopCluster && !m.is_root() {
        return Some(Gate::NotRoot);
    }
    let node_gate = |needs: fn(&Row) -> Option<Gate>| match selected_row(m) {
        None => Some(Gate::NoNode),
        Some(row) if !row.reachable => Some(Gate::Unreachable),
        Some(row) => needs(&row),
    };
    match action {
        Action::Drain => node_gate(|r| (r.drain_state != "active").then_some(Gate::NotActive)),
        Action::Return => node_gate(|r| {
            (!matches!(r.drain_state.as_str(), "draining" | "drained")).then_some(Gate::NotDraining)
        }),
        Action::FlushNode | Action::Monitor | Action::Detail => node_gate(|_| None),
        _ => None,
    }
}

/// The keys of the Cluster tab. `Enter` opens the detail; the rest are the actions and the movement.
pub fn on_action(m: &mut Model, action: Action) -> Vec<Effect> {
    match m.cluster.view {
        ClusterView::List => list_action(m, action),
        ClusterView::Detail => detail_action(m, action),
        ClusterView::Monitor => monitor_action(m, action),
    }
}

fn list_action(m: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Up => step(m, -1),
        Action::Down => step(m, 1),
        Action::PageUp => step(m, -10),
        Action::PageDown => step(m, 10),
        Action::First => step(m, i32::MIN / 2),
        Action::Last => step(m, i32::MAX / 2),
        Action::PaneNext | Action::PanePrev => {
            m.cluster.pane = match m.cluster.pane {
                Pane::Conditions => Pane::Nodes,
                Pane::Nodes => Pane::Conditions,
            };
            Vec::new()
        }
        Action::Enter => {
            if m.cluster.pane == Pane::Conditions {
                return open_condition(m);
            }
            crate::tui::menu::open(m)
        }
        Action::Detail => {
            if m.cluster.pane == Pane::Conditions {
                return open_condition(m);
            }
            open(m, ClusterView::Detail, Action::Detail)
        }
        Action::Monitor => open(m, ClusterView::Monitor, Action::Monitor),
        Action::Drain => confirm(m, action, |row| Pending::Drain(row.id.clone())),
        Action::Return => confirm(m, action, |row| Pending::Return(row.id.clone())),
        Action::FlushNode => confirm(m, action, |row| Pending::FlushNode(row.id.clone())),
        Action::FlushAll => ask(m, action, Pending::FlushAll, None),
        Action::StopCluster => {
            let name = m
                .status
                .data
                .as_ref()
                .and_then(|s| s.nodes.iter().find_map(|n| n.cluster_name.clone()))
                .unwrap_or_else(|| "stop".to_owned());
            let typed = Typed {
                expected: name,
                input: String::new(),
            };
            ask(m, action, Pending::Stop, Some(typed))
        }
        _ => Vec::new(),
    }
}

fn step(m: &mut Model, delta: i32) -> Vec<Effect> {
    match m.cluster.pane {
        Pane::Conditions => {
            let count = m.status.data.as_ref().map_or(0, |s| s.conditions.len());
            m.cluster.condition = moved(m.cluster.condition, delta, count);
        }
        Pane::Nodes => {
            let rows = rows(m);
            if let Some(current) = selected_index(m, &rows) {
                let next = moved(current, delta, rows.len());
                m.cluster.node = Some(rows[next].id.clone());
            }
        }
    }
    Vec::new()
}

/// `index` moved by `delta` inside `0..count`, without going around.
pub fn moved(index: usize, delta: i32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let target = i64::try_from(index).unwrap_or(0) + i64::from(delta);
    target.clamp(0, i64::try_from(count - 1).unwrap_or(0)) as usize
}

fn open_condition(m: &mut Model) -> Vec<Effect> {
    let name = m
        .status
        .data
        .as_ref()
        .and_then(|s| s.conditions.get(m.cluster.condition))
        .map(|c| c.name.clone());
    if let Some(name) = name {
        m.modal = Some(Modal::Condition(name));
    }
    Vec::new()
}

fn open(m: &mut Model, view: ClusterView, needs: Action) -> Vec<Effect> {
    if let Some(reason) = gate(m, needs) {
        return refuse(m, reason);
    }
    m.cluster.view = view;
    m.cluster.scroll = 0;
    m.cluster.metrics.reset();
    m.cluster.backup.reset();
    if view == ClusterView::Monitor {
        m.cluster.monitor.frozen = None;
    }
    Vec::new()
}

/// A key that cannot work now says why, in the footer (TUI-FRM-10).
pub fn refuse(m: &mut Model, reason: Gate) -> Vec<Effect> {
    let text = m.tx().t(reason.key());
    m.notify(Severity::Warning, text);
    Vec::new()
}

fn confirm(m: &mut Model, action: Action, pending: fn(&Row) -> Pending) -> Vec<Effect> {
    if let Some(reason) = gate(m, action) {
        return refuse(m, reason);
    }
    match selected_row(m) {
        Some(row) => ask(m, action, pending(&row), None),
        None => refuse(m, Gate::NoNode),
    }
}

fn ask(m: &mut Model, action: Action, pending: Pending, typed: Option<Typed>) -> Vec<Effect> {
    if let Some(reason) = gate(m, action) {
        return refuse(m, reason);
    }
    m.modal = Some(Modal::Confirm(Confirm {
        pending,
        typed,
        ok_focused: false,
    }));
    Vec::new()
}

fn detail_action(m: &mut Model, action: Action) -> Vec<Effect> {
    let effects = detail_keys(m, action);
    // `End` and the wheel may ask for more than there is: keep the offset where the text really ends.
    let visible = usize::from(m.size.1.saturating_sub(3));
    let max = view::detail_lines(m).len().saturating_sub(visible);
    m.cluster.scroll = m.cluster.scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
    effects
}

fn detail_keys(m: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Esc | Action::Left => m.cluster.view = ClusterView::List,
        Action::Up => m.cluster.scroll = m.cluster.scroll.saturating_sub(1),
        Action::Down => m.cluster.scroll = m.cluster.scroll.saturating_add(1),
        Action::PageUp => m.cluster.scroll = m.cluster.scroll.saturating_sub(10),
        Action::PageDown => m.cluster.scroll = m.cluster.scroll.saturating_add(10),
        Action::First => m.cluster.scroll = 0,
        Action::Last => m.cluster.scroll = u16::MAX,
        Action::Monitor => m.cluster.view = ClusterView::Monitor,
        Action::Drain
        | Action::Return
        | Action::FlushNode
        | Action::FlushAll
        | Action::StopCluster => return list_action(m, action),
        _ => {}
    }
    Vec::new()
}

fn monitor_action(m: &mut Model, action: Action) -> Vec<Effect> {
    let total = MON_CHARTS.len();
    let monitor = &mut m.cluster.monitor;
    match action {
        // Esc leaves the full screen first, then the charts.
        Action::Esc if monitor.full => monitor.full = false,
        Action::Esc => m.cluster.view = ClusterView::List,
        Action::Up => monitor.chart = moved(monitor.chart, -1, total),
        Action::Down => monitor.chart = moved(monitor.chart, 1, total),
        Action::First => monitor.chart = 0,
        Action::Last => monitor.chart = total - 1,
        Action::ChartFull => monitor.full = !monitor.full,
        Action::WindowWider => {
            let allowed = allowed_windows(m.history.retention_ms());
            m.cluster.monitor.window = (m.cluster.monitor.window + 1).min(allowed - 1);
        }
        Action::WindowNarrower => {
            m.cluster.monitor.window = m.cluster.monitor.window.saturating_sub(1);
        }
        Action::PauseChart => {
            m.cluster.monitor.frozen = match m.cluster.monitor.frozen {
                Some(_) => None,
                None => Some(m.now),
            };
        }
        Action::Scale => m.cluster.monitor.fixed_scale = !m.cluster.monitor.fixed_scale,
        Action::Export => {
            if let Some(node) = m
                .cluster
                .node
                .clone()
                .or_else(|| selected_row(m).map(|r| r.id))
            {
                let chart = m.cluster.monitor.chart;
                let key = MON_CHARTS[chart].key;
                let short = file_tag(&node);
                m.modal = Some(Modal::Input(Input {
                    purpose: InputPurpose::ExportCsv {
                        node,
                        series: chart,
                    },
                    value: format!("taba-{short}-{key}.csv"),
                }));
            }
        }
        _ => {}
    }
    Vec::new()
}

/// How many windows of `WINDOWS_MIN` fit in the retention (never above it, TUI-MON-03); at least one.
pub fn allowed_windows(retention_ms: u64) -> usize {
    WINDOWS_MIN
        .iter()
        .filter(|min| **min * 60_000 <= retention_ms)
        .count()
        .max(1)
}

/// The window in milliseconds, clipped to what is retained.
pub fn window_ms(m: &Model) -> u64 {
    let index = m
        .cluster
        .monitor
        .window
        .min(allowed_windows(m.history.retention_ms()) - 1);
    WINDOWS_MIN[index] * 60_000
}

/// The points of a line of a chart: the window, up to the instant it was frozen at.
pub fn line_points(m: &Model, node: &str, series: Series) -> Vec<Point> {
    let at = m.cluster.monitor.frozen.unwrap_or(m.now);
    m.history
        .series(node, series)
        .map(|r| r.window(at, window_ms(m)))
        .unwrap_or_default()
}

/// The visible series as CSV (TUI-MON-07): the time in UTC and one column per line.
pub fn export_csv(m: &Model, node: &str, chart: usize) -> String {
    let def = &MON_CHARTS[chart.min(MON_CHARTS.len() - 1)];
    let tx = m.tx();
    let mut out = String::from("time");
    for (_, name) in def.lines {
        out.push(',');
        out.push_str(&tx.t(name).replace(',', " "));
    }
    out.push('\n');
    let lines: Vec<Vec<Point>> = def
        .lines
        .iter()
        .map(|(series, _)| line_points(m, node, *series))
        .collect();
    let offset = m.wall_ms - i64::try_from(m.now).unwrap_or(0);
    if let Some(first) = lines.first() {
        for point in first {
            let wall = offset + i64::try_from(point.at).unwrap_or(0);
            let c = i18n::civil_from_ms(wall);
            out.push_str(&format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                c.year, c.month, c.day, c.hour, c.minute, c.second
            ));
            for line in &lines {
                out.push(',');
                if let Some(p) = line.iter().find(|p| p.at == point.at) {
                    out.push_str(&p.value.to_string());
                }
            }
            out.push('\n');
        }
    }
    out
}

/// Starts watching a node's drain until it reaches `goal`.
pub fn watch(m: &mut Model, node: &str, goal: Goal) {
    m.cluster.watch = Some(DrainWatch {
        node: node.to_owned(),
        goal,
    });
    m.cluster.drain.reset();
}

/// The call an action's confirmation runs.
pub fn call_of(pending: &Pending) -> Option<ApiCall> {
    pending.call()
}

#[cfg(test)]
mod tests;
