use std::collections::VecDeque;
use std::sync::Mutex;

/// Explicit file actions run in click order before ordinary queued work.
/// A running worker is never interrupted; the gate is checked before starting work.
#[derive(Debug, Default)]
pub struct PriorityQueue(Mutex<VecDeque<(String, bool)>>);

impl PriorityQueue {
    pub fn push(&self, id: String) {
        let mut queue = self.0.lock().expect("priority queue lock poisoned");
        if !queue.iter().any(|(existing, _)| existing == &id) {
            queue.push_back((id, false));
        }
    }

    pub fn front(&self) -> Option<(String, bool)> {
        self.0
            .lock()
            .expect("priority queue lock poisoned")
            .front()
            .cloned()
    }

    pub fn allows(&self, id: &str) -> bool {
        self.front().is_none_or(|(first, _)| first == id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.0
            .lock()
            .expect("priority queue lock poisoned")
            .iter()
            .any(|(item, _)| item == id)
    }

    pub fn mark_started(&self, id: &str) {
        if let Some((_, started)) = self
            .0
            .lock()
            .expect("priority queue lock poisoned")
            .iter_mut()
            .find(|(item, _)| item == id)
        {
            *started = true;
        }
    }

    pub fn remove(&self, id: &str) {
        self.0
            .lock()
            .expect("priority queue lock poisoned")
            .retain(|(item, _)| item != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_are_fifo_deduplicated_and_release_ordinary_work() {
        let queue = PriorityQueue::default();
        assert!(queue.allows("ordinary"));
        queue.push("first".into());
        queue.push("second".into());
        queue.push("first".into());
        queue.mark_started("first");
        assert_eq!(queue.front(), Some(("first".into(), true)));
        assert!(!queue.allows("ordinary"));
        assert!(!queue.allows("second"));
        queue.remove("first");
        assert_eq!(queue.front(), Some(("second".into(), false)));
        queue.remove("second");
        assert!(queue.allows("ordinary"));
    }
}
