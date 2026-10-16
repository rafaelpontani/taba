//! The strip of charts under the list of topics: the three charts of the selected queue side by side, fed by the
//! same reading as the charts dialog (`GET /stats/topics/{name}`), once per sampling interval, only while the list
//! is what is shown. What was sampled of the topics selected lately is kept, so coming back to one does not start
//! its charts over.

use crate::tui::model::{Model, TopicMonitor, TopicTab};

use super::selected;

/// How many topics keep their samples.
pub const MAX: usize = 8;

/// The strip of the selected topic; none for a distributor, which has no queue, or when nothing is selected.
pub fn current(m: &Model) -> Option<&TopicMonitor> {
    let topic = selected(m)?;
    if topic.is_distributor() {
        return None;
    }
    m.topics.previews.iter().find(|p| p.name == topic.name)
}

/// Makes sure the selected topic has its strip, and that it is the most recent one.
pub fn prepare(m: &mut Model) {
    let Some(topic) = selected(m) else { return };
    if topic.is_distributor() {
        return;
    }
    let previews = &mut m.topics.previews;
    match previews.iter().position(|p| p.name == topic.name) {
        Some(at) => {
            let kept = previews.remove(at);
            previews.push(kept);
        }
        None => previews.push(TopicMonitor::new(
            topic.name,
            TopicTab::Charts,
            false,
            topic.dead_letter,
        )),
    }
    if previews.len() > MAX {
        previews.remove(0);
    }
}
