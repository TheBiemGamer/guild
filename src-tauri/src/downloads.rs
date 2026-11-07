//! The Epic download queue: one Legendary job at a time, the rest wait in order. Pure; `commands::game`
//! runs it and stores it as the `download_queue` setting.

use serde::{Deserialize, Serialize};

/// The settings value holding the queue's JSON.
pub const DOWNLOAD_QUEUE: &str = "download_queue";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueuedAction {
    Install,
    Update,
    Verify,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// The Epic copy key, `epic-<app_name>`.
    pub key: String,
    pub action: QueuedAction,
    /// Install: just this DLC of the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlc: Option<String>,
    /// Install: take the owned DLC too, or skip them; `None` for a DLC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_dlc: Option<bool>,
    /// Install: the base folder chosen when it was queued; `None` is Legendary's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
}

impl Job {
    pub fn new(key: &str, action: QueuedAction) -> Self {
        Self { key: key.into(), action, dlc: None, with_dlc: None, dest: None }
    }
}

/// `jobs[0]` is the active job, also while paused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    pub jobs: Vec<Job>,
    #[serde(default)]
    pub paused: bool,
}

impl Queue {
    /// Adds a job at the end. A job with the same key and action already queued wins; returns whether it was added.
    pub fn push(&mut self, job: Job) -> bool {
        if self.jobs.iter().any(|j| j.key == job.key && j.action == job.action) {
            return false;
        }
        self.jobs.push(job);
        true
    }

    /// The job to start: the head, unless paused.
    pub fn next(&self) -> Option<&Job> {
        self.jobs.first().filter(|_| !self.paused)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.jobs.iter().any(|j| j.key == key)
    }

    pub fn is_head(&self, key: &str) -> bool {
        self.jobs.first().is_some_and(|j| j.key == key)
    }

    /// The active job `key` ended: the next one becomes the head.
    pub fn finish(&mut self, key: &str) -> bool {
        let head = self.is_head(key);
        if head {
            self.jobs.remove(0);
        }
        head
    }

    /// Takes the first job with `key` out of the queue, the head included.
    pub fn remove(&mut self, key: &str) -> bool {
        let Some(i) = self.position(key) else { return false };
        self.jobs.remove(i);
        true
    }

    /// One place earlier, never ahead of the active job.
    pub fn move_up(&mut self, key: &str) -> bool {
        match self.position(key) {
            Some(i) if i >= 2 => {
                self.jobs.swap(i, i - 1);
                true
            }
            _ => false,
        }
    }

    /// One place later; the active job stays put.
    pub fn move_down(&mut self, key: &str) -> bool {
        match self.position(key) {
            Some(i) if i >= 1 && i + 1 < self.jobs.len() => {
                self.jobs.swap(i, i + 1);
                true
            }
            _ => false,
        }
    }

    /// The whole queue waits; the head stays the active job.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    /// The stored queue without the jobs whose games are gone (`has` says which keys still exist).
    /// Nothing stored, or unreadable, is an empty queue.
    pub fn restore(json: Option<&str>, has: impl Fn(&str) -> bool) -> Self {
        let mut queue = json.and_then(Self::from_json).unwrap_or_default();
        queue.jobs.retain(|j| has(&j.key));
        queue
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.jobs.iter().position(|j| j.key == key)
    }
}

/// One queued job as `poll` reports it.
#[derive(Debug, PartialEq, Serialize)]
pub struct QueuedView {
    pub key: String,
    pub action: QueuedAction,
    pub title: String,
}

/// The queue for the UI, in order; `active` is the head's key (also while paused).
#[derive(Debug, PartialEq, Serialize)]
pub struct QueueView {
    pub jobs: Vec<QueuedView>,
    pub paused: bool,
    pub active: Option<String>,
}

pub fn view(queue: &Queue, title: impl Fn(&str) -> Option<String>) -> QueueView {
    QueueView {
        jobs: queue
            .jobs
            .iter()
            .map(|j| QueuedView { key: j.key.clone(), action: j.action, title: title(&j.key).unwrap_or_default() })
            .collect(),
        paused: queue.paused,
        active: queue.jobs.first().map(|j| j.key.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use QueuedAction::*;

    fn queue(keys: &[&str]) -> Queue {
        let mut q = Queue::default();
        for k in keys {
            q.push(Job::new(k, Install));
        }
        q
    }

    fn keys(q: &Queue) -> Vec<&str> {
        q.jobs.iter().map(|j| j.key.as_str()).collect()
    }

    #[test]
    fn jobs_wait_in_the_order_they_came() {
        let q = queue(&["epic-a", "epic-b", "epic-c"]);
        assert_eq!(keys(&q), ["epic-a", "epic-b", "epic-c"]);
        assert_eq!(q.next().map(|j| j.key.as_str()), Some("epic-a"));
    }

    #[test]
    fn a_duplicate_key_and_action_is_ignored() {
        let mut q = queue(&["epic-a"]);
        assert!(!q.push(Job::new("epic-a", Install)));
        assert!(q.push(Job::new("epic-a", Verify)), "another action on the same game queues");
        assert_eq!(q.jobs.len(), 2);
    }

    #[test]
    fn finish_advances_to_the_next_job() {
        let mut q = queue(&["epic-a", "epic-b"]);
        assert!(!q.finish("epic-b"), "only the active job finishes");
        assert!(q.finish("epic-a"));
        assert_eq!(q.next().map(|j| j.key.as_str()), Some("epic-b"));
        assert!(q.finish("epic-b"));
        assert_eq!(q.next(), None);
    }

    #[test]
    fn queued_jobs_move_but_the_active_one_stays() {
        let mut q = queue(&["epic-a", "epic-b", "epic-c"]);
        assert!(!q.move_up("epic-b"), "nothing goes ahead of the active job");
        assert!(!q.move_down("epic-a"), "the active job can't move");
        assert!(!q.move_up("epic-a"));
        assert!(!q.move_down("epic-c"), "already last");
        assert!(q.move_up("epic-c"));
        assert_eq!(keys(&q), ["epic-a", "epic-c", "epic-b"]);
        assert!(q.move_down("epic-c"));
        assert_eq!(keys(&q), ["epic-a", "epic-b", "epic-c"]);
        assert!(!q.move_up("epic-x"));
    }

    #[test]
    fn remove_takes_a_job_out() {
        let mut q = queue(&["epic-a", "epic-b", "epic-c"]);
        assert!(q.remove("epic-b"));
        assert_eq!(keys(&q), ["epic-a", "epic-c"]);
        assert!(q.remove("epic-a"));
        assert_eq!(keys(&q), ["epic-c"]);
        assert!(!q.remove("epic-b"));
        assert!(q.contains("epic-c") && !q.contains("epic-a"));
    }

    #[test]
    fn pause_and_resume_keep_the_head() {
        let mut q = queue(&["epic-a", "epic-b"]);
        q.pause();
        assert_eq!(q.next(), None, "nothing starts while paused");
        assert!(q.is_head("epic-a"));
        assert!(q.push(Job::new("epic-c", Update)), "jobs still queue while paused");
        q.resume();
        assert_eq!(q.next().map(|j| j.key.as_str()), Some("epic-a"));
        assert_eq!(keys(&q), ["epic-a", "epic-b", "epic-c"]);
    }

    #[test]
    fn json_round_trip() {
        let mut q = queue(&["epic-a"]);
        q.push(Job { dlc: Some("HatDlc".into()), dest: Some(r"D:\Games".into()), ..Job::new("epic-b", Install) });
        q.push(Job { with_dlc: Some(false), ..Job::new("epic-c", Install) });
        q.push(Job::new("epic-d", Verify));
        q.pause();
        let json = q.to_json();
        assert_eq!(Queue::from_json(&json), Some(q));
        assert!(json.contains(r#"{"key":"epic-d","action":"verify"}"#), "{json}");
        assert_eq!(Queue::from_json("junk"), None);
    }

    #[test]
    fn restore_drops_jobs_whose_games_are_gone() {
        let mut stored = queue(&["epic-gone", "epic-b", "epic-c"]);
        stored.pause();
        let q = Queue::restore(Some(&stored.to_json()), |k| k != "epic-gone");
        assert_eq!(keys(&q), ["epic-b", "epic-c"]);
        assert!(q.paused);
        assert_eq!(Queue::restore(None, |_| true), Queue::default());
        assert_eq!(Queue::restore(Some("junk"), |_| true), Queue::default());
    }

    #[test]
    fn the_view_lists_titles_in_order() {
        let v = view(&queue(&["epic-a", "epic-b"]), |k| (k == "epic-a").then(|| "Alpha".into()));
        assert_eq!(v.active.as_deref(), Some("epic-a"));
        assert_eq!(v.jobs[0], QueuedView { key: "epic-a".into(), action: Install, title: "Alpha".into() });
        assert_eq!(v.jobs[1].title, "");
    }
}
