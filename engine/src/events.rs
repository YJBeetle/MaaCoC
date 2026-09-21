//! Turning framework notifications into UI-safe events.
//!
//! Callbacks fire on framework threads, so the handler only parses and pushes.
//! The notification payload already carries `reco_details`, so candidate
//! scores come for free without re-entering the C API.

use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeEvent {
    pub at: f64,
    pub reco_id: i64,
    pub wall: f64,
    pub kind: &'static str,
    pub node: String,
    pub focus: String,
    pub hit: bool,
    pub box_rect: Option<[i32; 4]>,
    pub score: f64,
    pub candidates: usize,
    pub filtered: usize,
    pub error: bool,
}

impl NodeEvent {
    pub fn label(&self) -> &str {
        if self.focus.is_empty() {
            &self.node
        } else {
            &self.focus
        }
    }

    pub fn summary(&self) -> String {
        if self.kind == "action" {
            return format!("action {}", self.label());
        }
        let state = if self.error {
            "失败"
        } else if self.hit {
            "命中"
        } else {
            "未中"
        };
        format!(
            "{} {state} 分数={:.3} 框={:?} 候选={}/{}",
            self.label(),
            self.score,
            self.box_rect,
            self.candidates,
            self.filtered
        )
    }
}

pub(crate) fn parse_box(value: &serde_json::Value) -> Option<[i32; 4]> {
    let arr = value.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    let nums: Vec<i32> = arr.iter().filter_map(|v| v.as_i64().map(|n| n as i32)).collect();
    if nums.len() != 4 {
        return None;
    }
    [nums[2], nums[3]]
        .iter()
        .all(|v| *v > 0)
        .then_some([nums[0], nums[1], nums[2], nums[3]])
}

/// Score to surface: the winner if there is one, otherwise the best rejected
/// candidate — a miss is far more useful showing "0.668 vs 阈值 0.7" than "0".
pub(crate) fn best_score(detail: &serde_json::Value) -> f64 {
    if let Some(score) = detail
        .get("best")
        .and_then(|best| best.get("score"))
        .and_then(Value::as_f64)
    {
        return score;
    }
    let mut best = 0.0_f64;
    if let Some(items) = detail.get("all").and_then(Value::as_array) {
        for item in items {
            if let Some(score) = item.get("score").and_then(Value::as_f64) {
                best = best.max(score);
            }
        }
    }
    best
}

fn focus_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(map)) => map
            .values()
            .find_map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.get("content").and_then(|c| c.as_str()).map(str::to_string))
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Parse one notification. Returns None for messages we do not surface.
pub fn parse_event(msg: &str, details: &str, at: f64, wall: f64) -> Option<NodeEvent> {
    let state = msg.rsplit('.').next().unwrap_or("");
    if state == "Starting" {
        return None;
    }
    let kind = if msg.starts_with("Node.Recognition.") {
        "recognition"
    } else if msg.starts_with("Node.Action.") {
        "action"
    } else {
        return None;
    };
    let payload: serde_json::Value = serde_json::from_str(details).ok()?;
    let node = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let focus = focus_text(payload.get("focus"));
    let succeeded = state == "Succeeded";

    let mut event = NodeEvent {
        at,
        wall,
        reco_id: payload.get("reco_id").and_then(Value::as_i64).unwrap_or(0),
        kind,
        node,
        focus,
        hit: succeeded,
        box_rect: None,
        score: 0.0,
        candidates: 0,
        filtered: 0,
        // A recognition that did not match is a miss, not an error; only a
        // failed action means something actually went wrong.
        error: !succeeded && kind == "action",
    };

    if kind == "recognition" {
        if let Some(reco) = payload.get("reco_details") {
            event.box_rect = reco.get("box").and_then(parse_box);
            if let Some(detail) = reco.get("detail") {
                let count = |key: &str| detail.get(key).and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0);
                event.candidates = count("all");
                event.filtered = count("filtered");
                event.score = best_score(detail);
            }
        }
    }
    Some(event)
}

#[derive(Default)]
struct State {
    queue: VecDeque<NodeEvent>,
    /// Notifications we recognise but do not turn into events (step/controller chatter).
    ignored: usize,
    /// Events that never reached a reader because the queue hit its cap.
    dropped: usize,
    started: Option<Instant>,
}

/// Thread-safe collector shared between framework callbacks and the UI/CLI.
#[derive(Clone, Default)]
pub struct EventBus {
    state: Arc<Mutex<State>>,
}

fn now_wall() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks t=0 for relative timestamps and clears history.
    pub fn restart(&self) {
        let mut state = self.state.lock().unwrap();
        state.queue.clear();
        state.dropped = 0;
        state.ignored = 0;
        state.started = Some(Instant::now());
    }

    pub fn handle(&self) -> impl Fn(&str, &str) + Send + Sync + 'static {
        let state = Arc::clone(&self.state);
        move |msg: &str, details: &str| {
            let (at, wall) = {
                let guard = state.lock().unwrap();
                (
                    guard.started.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0),
                    now_wall(),
                )
            };
            match parse_event(msg, details, at, wall) {
                Some(event) => {
                    let mut guard = state.lock().unwrap();
                    guard.queue.push_back(event);
                    while guard.queue.len() > 2000 {
                        guard.queue.pop_front();
                        guard.dropped += 1;
                    }
                }
                None => {
                    let mut guard = state.lock().unwrap();
                    guard.ignored += 1;
                }
            }
        }
    }

    /// Removes and returns everything collected since the last drain.
    pub fn drain(&self) -> Vec<NodeEvent> {
        let mut state = self.state.lock().unwrap();
        state.queue.drain(..).collect()
    }

    /// Events lost to the queue cap. Non-zero means the reader is too slow.
    pub fn dropped(&self) -> usize {
        self.state.lock().unwrap().dropped
    }

    /// Notifications seen and deliberately not turned into events.
    pub fn ignored(&self) -> usize {
        self.state.lock().unwrap().ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape taken from a real Node.Recognition.Succeeded payload on the device.
    const HIT: &str = r#"{
        "task_id":200000001,"reco_id":400000001,"name":"FindSoldier","focus":"FindSoldier!",
        "reco_details":{"algorithm":"TemplateMatch","box":[92,594,90,118],
            "detail":{"all":[{"box":[53,619,86,90],"score":0.34},{"box":[92,594,90,118],"score":0.996}],
                      "filtered":[{"box":[92,594,90,118],"score":0.996}],
                      "best":{"box":[92,594,90,118],"score":0.996}},
            "name":"FindSoldier","reco_id":400000001}}"#;

    const MISS: &str = r#"{"task_id":2,"reco_id":3,"name":"FindNext","focus":null,
        "reco_details":{"algorithm":"TemplateMatch","box":null,
            "detail":{"all":[{"box":[1037,466,197,92],"score":0.668}],"filtered":[],"best":null}}}"#;

    #[test]
    fn carries_the_reco_id_for_frame_lookup() {
        let event = parse_event("Node.Recognition.Succeeded", HIT, 0.0, 0.0).unwrap();
        assert_eq!(event.reco_id, 400000001);
    }

    #[test]
    fn extracts_scores_and_counts_from_a_hit() {
        let event = parse_event("Node.Recognition.Succeeded", HIT, 1.5, 100.0).unwrap();
        assert_eq!(event.kind, "recognition");
        assert_eq!(event.node, "FindSoldier");
        assert_eq!(event.focus, "FindSoldier!");
        assert!(event.hit);
        assert_eq!(event.box_rect, Some([92, 594, 90, 118]));
        assert!((event.score - 0.996).abs() < 1e-6);
        assert_eq!((event.candidates, event.filtered), (2, 1));
        assert_eq!(event.label(), "FindSoldier!");
    }

    #[test]
    fn a_run_without_a_box_is_a_miss_not_an_error() {
        let event = parse_event("Node.Recognition.Failed", MISS, 0.1, 0.0).unwrap();
        assert!(!event.hit);
        assert!(!event.error, "未命中不是异常");
        assert_eq!(event.candidates, 1);
        assert_eq!(event.filtered, 0);
        assert!((event.score - 0.668).abs() < 1e-6);
        assert_eq!(event.label(), "FindNext");
    }

    #[test]
    fn direct_hit_nodes_are_hits_even_without_a_box() {
        let details = r#"{"task_id":1,"reco_id":2,"name":"Main","focus":null}"#;
        let event = parse_event("Node.Recognition.Succeeded", details, 0.0, 0.0).unwrap();
        assert!(event.hit, "DirectHit 节点没有框，但确实是命中");
        assert!(!event.error);
    }

    #[test]
    fn a_failed_action_is_an_error() {
        let details = r#"{"task_id":1,"action_id":2,"name":"SetScreen","focus":null}"#;
        let event = parse_event("Node.Action.Failed", details, 0.0, 0.0).unwrap();
        assert!(event.error);
    }

    #[test]
    fn starting_and_unrelated_messages_are_skipped() {
        assert!(parse_event("Node.Recognition.Starting", HIT, 0.0, 0.0).is_none());
        assert!(parse_event("Controller.Action.Succeeded", "{}", 0.0, 0.0).is_none());
        assert!(parse_event("Node.Recognition.Succeeded", "not json", 0.0, 0.0).is_none());
    }

    #[test]
    fn action_events_carry_focus_from_object_form() {
        let details = r#"{"task_id":1,"action_id":2,"name":"Deploy","focus":{"Node.Action.Starting":"放兵"}}"#;
        let event = parse_event("Node.Action.Succeeded", details, 3.0, 0.0).unwrap();
        assert_eq!(event.kind, "action");
        assert_eq!(event.focus, "放兵");
        assert_eq!(event.summary(), "action 放兵");
    }

    #[test]
    fn bus_collects_in_order_and_drains_once() {
        let bus = EventBus::new();
        bus.restart();
        let sink = bus.handle();
        sink("Node.Recognition.Succeeded", HIT);
        sink("Node.Recognition.Starting", HIT);
        let first = bus.drain();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].node, "FindSoldier");
        assert!(bus.drain().is_empty(), "drain must not replay events");
        assert_eq!(bus.ignored(), 1);
        assert_eq!(bus.dropped(), 0, "nothing was lost, only not understood");
    }

    #[test]
    fn an_unread_queue_fills_up_and_reports_what_it_lost() {
        let bus = EventBus::new();
        bus.restart();
        let sink = bus.handle();
        for _ in 0..2500 {
            sink("Node.Recognition.Succeeded", HIT);
        }
        assert_eq!(bus.dropped(), 500);
        assert_eq!(bus.drain().len(), 2000);
    }

    #[test]
    fn bus_is_safe_to_share_across_threads() {
        let bus = EventBus::new();
        bus.restart();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let sink = bus.handle();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        sink("Node.Recognition.Succeeded", HIT);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(bus.drain().len(), 800);
    }
}
