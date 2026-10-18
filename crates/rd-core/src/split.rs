//! Split tabs (Swift `SplitLayout`, `BroadcastGroup`, `SavedSplit.connections(in:)`): the pane grid, which
//! panes broadcast input to which, and what a saved split opens.

use std::collections::HashSet;

use crate::library::Library;
use crate::model::{Connection, SavedSplit};

/// The most panes a split tab holds.
pub const MAX_PANES: usize = 4;

/// Pane indices per column, left to right and top to bottom: 1 → one column; 2 → side by side;
/// 3 → one left, two stacked right; 4 → 2×2 (row-major: 0 1 / 2 3). Counts above [`MAX_PANES`] are
/// clamped.
#[must_use]
pub fn columns(count: usize) -> Vec<Vec<usize>> {
    match count.min(MAX_PANES) {
        0 => vec![],
        1 => vec![vec![0]],
        2 => vec![vec![0], vec![1]],
        3 => vec![vec![0], vec![1, 2]],
        _ => vec![vec![0, 2], vec![1, 3]],
    }
}

/// Panes of a split tab and which of them broadcast input (iTerm style: typing in any broadcasting pane
/// goes to every other broadcasting pane).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BroadcastGroup {
    pane_ids: Vec<String>,
    broadcasting: HashSet<String>,
}

impl BroadcastGroup {
    /// An empty group.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The panes in order.
    #[must_use]
    pub fn pane_ids(&self) -> &[String] {
        &self.pane_ids
    }

    /// The broadcasting panes.
    #[must_use]
    pub fn broadcasting(&self) -> &HashSet<String> {
        &self.broadcasting
    }

    /// No more panes fit.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.pane_ids.len() >= MAX_PANES
    }

    /// Adds a pane (not broadcasting); false when the split is full or the pane is already in it.
    pub fn add(&mut self, id: &str) -> bool {
        if self.is_full() || self.pane_ids.iter().any(|p| p == id) {
            return false;
        }
        self.pane_ids.push(id.to_owned());
        true
    }

    /// Removes a pane and its broadcast flag.
    pub fn remove(&mut self, id: &str) {
        self.pane_ids.retain(|p| p != id);
        self.broadcasting.remove(id);
    }

    /// Whether `id` broadcasts.
    #[must_use]
    pub fn is_broadcasting(&self, id: &str) -> bool {
        self.broadcasting.contains(id)
    }

    /// Switches one pane's broadcast; unknown panes are ignored.
    pub fn set_broadcasting(&mut self, id: &str, on: bool) {
        if !self.pane_ids.iter().any(|p| p == id) {
            return;
        }
        if on {
            self.broadcasting.insert(id.to_owned());
        } else {
            self.broadcasting.remove(id);
        }
    }

    /// Every pane broadcasts (and there is at least one).
    #[must_use]
    pub fn is_all_broadcasting(&self) -> bool {
        !self.pane_ids.is_empty() && self.broadcasting.len() == self.pane_ids.len()
    }

    /// Switches every pane.
    pub fn set_all(&mut self, on: bool) {
        self.broadcasting = if on {
            self.pane_ids.iter().cloned().collect()
        } else {
            HashSet::new()
        };
    }

    /// Panes that receive input typed into `source`: none unless it broadcasts, else the other
    /// broadcasting, running panes in pane order.
    #[must_use]
    pub fn targets(&self, source: &str, running: &HashSet<String>) -> Vec<String> {
        if !self.broadcasting.contains(source) {
            return vec![];
        }
        self.pane_ids
            .iter()
            .filter(|p| *p != source && self.broadcasting.contains(*p) && running.contains(*p))
            .cloned()
            .collect()
    }
}

impl Library {
    /// The connections a saved split opens: existing ones, in order, duplicates kept, at most
    /// [`MAX_PANES`]. Broadcast always starts off (it is never saved).
    #[must_use]
    pub fn saved_split_connections(&self, split: &SavedSplit) -> Vec<&Connection> {
        split
            .connection_ids
            .iter()
            .filter_map(|id| self.connections.iter().find(|c| &c.id == id))
            .take(MAX_PANES)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnectionType, Timestamp};

    #[test]
    fn columns_per_pane_count() {
        assert!(columns(0).is_empty());
        assert_eq!(columns(1), [vec![0]]);
        assert_eq!(columns(2), [vec![0], vec![1]]);
        assert_eq!(columns(3), [vec![0], vec![1, 2]]);
        assert_eq!(columns(4), [vec![0, 2], vec![1, 3]]);
    }

    #[test]
    fn clamps_above_maximum() {
        assert_eq!(MAX_PANES, 4);
        assert_eq!(columns(7), columns(4));
    }

    #[test]
    fn every_pane_appears_once() {
        for count in 1..=4 {
            let mut all: Vec<usize> = columns(count).into_iter().flatten().collect();
            all.sort_unstable();
            assert_eq!(all, (0..count).collect::<Vec<_>>());
        }
    }

    fn group(ids: &[&str], broadcasting: &[&str]) -> BroadcastGroup {
        let mut group = BroadcastGroup::new();
        for id in ids {
            group.add(id);
        }
        for id in broadcasting {
            group.set_broadcasting(id, true);
        }
        group
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn adds_up_to_four_and_refuses_duplicates() {
        let mut group = BroadcastGroup::new();
        let added: Vec<bool> = ["a", "a", "b", "c", "d", "e"]
            .iter()
            .map(|id| group.add(id))
            .collect();
        assert_eq!(added, [true, false, true, true, true, false]);
        assert!(group.is_full());
        assert_eq!(group.pane_ids(), ["a", "b", "c", "d"]);
        assert!(group.broadcasting().is_empty());
    }

    #[test]
    fn remove_drops_broadcast_flag() {
        let mut group = group(&["a", "b"], &["a", "b"]);
        group.remove("a");
        assert_eq!(group.pane_ids(), ["b"]);
        assert_eq!(group.broadcasting(), &set(&["b"]));
        assert!(!group.is_full());
    }

    #[test]
    fn ignores_broadcast_for_unknown_pane() {
        let mut group = group(&["a"], &[]);
        group.set_broadcasting("x", true);
        assert!(group.broadcasting().is_empty());
        assert!(!group.is_broadcasting("x"));
    }

    #[test]
    fn no_targets_when_source_does_not_broadcast() {
        let group = group(&["a", "b", "c"], &["b", "c"]);
        assert!(group.targets("a", &set(&["a", "b", "c"])).is_empty());
    }

    #[test]
    fn targets_are_other_broadcasting_running_panes_in_order() {
        // d: syslog pane, not broadcasting; c: exited.
        let group = group(&["a", "b", "c", "d"], &["a", "b", "c"]);
        assert_eq!(group.targets("b", &set(&["a", "b", "d"])), ["a"]);
        assert_eq!(group.targets("a", &set(&["a", "b", "c", "d"])), ["b", "c"]);
    }

    #[test]
    fn set_all_and_is_all_broadcasting() {
        let mut group = group(&["a", "b"], &["a"]);
        assert!(!group.is_all_broadcasting());
        group.set_all(true);
        assert!(group.is_all_broadcasting());
        assert_eq!(group.targets("b", &set(&["a", "b"])), ["a"]);
        group.set_all(false);
        assert!(group.broadcasting().is_empty());
        assert!(!BroadcastGroup::new().is_all_broadcasting());
    }

    fn connection(id: &str) -> Connection {
        let mut connection = Connection::new(ConnectionType::Ssh, id, id);
        connection.id = id.into();
        connection
    }

    fn saved(ids: &[&str]) -> SavedSplit {
        SavedSplit {
            id: "S".into(),
            name: "x".into(),
            connection_ids: ids.iter().map(|s| (*s).to_owned()).collect(),
            sort_order: 0,
            created_at: Timestamp::now(),
        }
    }

    #[test]
    fn saved_split_connections_skip_missing_keep_order_and_cap() {
        let library = Library {
            connections: vec![connection("a"), connection("b")],
            ..Library::default()
        };
        let ids = |split: &SavedSplit| -> Vec<String> {
            library
                .saved_split_connections(split)
                .into_iter()
                .map(|c| c.id.clone())
                .collect()
        };
        assert_eq!(ids(&saved(&["b", "gone", "a"])), ["b", "a"]);
        assert_eq!(ids(&saved(&["a", "a", "b", "b", "a"])), ["a", "a", "b", "b"]);
        assert!(ids(&saved(&["gone"])).is_empty());
    }
}
