//! The FIFO queues while the previous lives are restored (doc 37, section 4.10): they receive messages and do not
//! deliver them, nothing is deduplicated, and the generator of IDs starts above the watermark of the old lives. At
//! the release the group's primary deduplicates what it accepted meanwhile, in `id` order, against what the
//! restoration brought, and only then delivery starts.

use super::Messages;

impl Messages {
    /// From now on the FIFO queues receive and do not deliver (the cluster is being restored from disk).
    pub fn hold_fifo(&self) {
        self.fifo_held
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Are the FIFO queues held?
    pub fn fifo_held(&self) -> bool {
        self.fifo_held.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The publications this node, as primary of the group, accepted during the hold and that repeat a key: a
    /// restored message with the same `(group, deduplicationId)`, or an earlier publication. `(topic, id)`, in `id`
    /// order per topic. They are the ones the release discards (doc 37, section 4.10).
    pub fn fifo_duplicates(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (name, state) in self.existing_states() {
            let state = super::lock(&state);
            if !state.index.is_fifo() {
                continue;
            }
            for (id, key) in &state.provisional {
                if !state.index.contains(id) {
                    continue;
                }
                let original = state.dedup.get(key).map(|(first, _)| first);
                if original.is_some_and(|first| first != id) {
                    out.push((name.clone(), id.clone()));
                }
            }
        }
        out
    }

    /// Releases the FIFO queues: forgets the provisional list and starts delivering.
    pub fn release_fifo(&self) {
        for (name, state) in self.existing_states() {
            let fifo = {
                let mut state = super::lock(&state);
                state.provisional.clear();
                state.index.is_fifo()
            };
            if fifo {
                self.wake(&name, 1);
            }
        }
        self.fifo_held
            .store(false, std::sync::atomic::Ordering::SeqCst);
        for (name, state) in self.existing_states() {
            let mut state = super::lock(&state);
            if state.index.is_fifo() {
                self.ready_changed(&name, &mut state);
            }
        }
    }

    /// The new generator stays above the watermark of an old life: its IDs are all greater than the old ones
    /// (doc 37, section 4.10). An ID that is not a GUID v7 is ignored.
    pub fn raise_id_floor(&self, watermark: &str) {
        if let Some(ms) = crate::core::id::unix_millis_of(watermark) {
            self.ids.raise_floor(ms.saturating_add(1));
        }
    }

    /// The largest ID this node generated (or stays above): what the manifest of the running life records.
    pub fn watermark(&self) -> Option<String> {
        self.ids.watermark()
    }
}
