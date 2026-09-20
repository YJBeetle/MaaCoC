//! Running one node's recognition against a still frame.
//!
//! This is the answer to "why didn't it match": the node's own configuration,
//! evaluated on a saved frame, with every candidate score exposed.

use crate::events::{parse_box, EventBus, NodeEvent};
use crate::Result;
use maa_framework::{buffer::MaaImageBuffer, resource::Resource, tasker::Tasker};
use serde_json::{Map, Value};

/// Recognition parameters that ride alongside `recognition` in pipeline v1.
const RECOGNITION_KEYS: &[&str] = &[
    "template",
    "threshold",
    "roi",
    "roi_offset",
    "order_by",
    "index",
    "method",
    "green_mask",
    "expected",
    "count",
    "lower",
    "upper",
    "detector",
    "ratio",
    "only_rec",
    "model",
    "labels",
    "replace",
    "color_filter",
    "custom_recognition",
    "custom_recognition_param",
];

#[derive(Debug, Clone, Default)]
pub struct Trial {
    pub node: String,
    pub hit: bool,
    pub algorithm: String,
    pub score: f64,
    pub box_rect: Option<[i32; 4]>,
    pub candidates: usize,
    pub filtered: usize,
    pub threshold: Option<f64>,
    pub roi: Option<[i32; 4]>,
    pub error: Option<String>,
}

impl Trial {
    pub fn summary(&self) -> String {
        match &self.error {
            Some(err) => format!("{:<22} 错误: {err}", self.node),
            None => format!(
                "{:<22} {} 分数={:.3} 框={:?} 候选={}/{} 阈值={:?} roi={:?}",
                self.node,
                if self.hit { "命中" } else { "未中" },
                self.score,
                self.box_rect,
                self.candidates,
                self.filtered,
                self.threshold,
                self.roi
            ),
        }
    }
}

/// Split a node definition into (recognition type, parameters), accepting both
/// pipeline v1 (flat) and v2 (nested under `param`).
pub fn recognition_of(node: &Value) -> Option<(String, Map<String, Value>)> {
    match node.get("recognition")? {
        Value::String(kind) => {
            let mut params = Map::new();
            if let Some(obj) = node.as_object() {
                for key in RECOGNITION_KEYS {
                    if let Some(value) = obj.get(*key) {
                        params.insert((*key).to_string(), value.clone());
                    }
                }
            }
            Some((kind.clone(), params))
        }
        Value::Object(obj) => {
            let kind = obj.get("type")?.as_str()?.to_string();
            let params = obj.get("param").and_then(Value::as_object).cloned().unwrap_or_default();
            Some((kind, params))
        }
        _ => None,
    }
}

fn apply_overrides(
    params: &mut Map<String, Value>,
    threshold: Option<f64>,
    roi: Option<[i32; 4]>,
) {
    if let Some(value) = threshold {
        // A list threshold must be as long as the template list.
        let count = match params.get("template") {
            Some(Value::Array(items)) => items.len(),
            Some(Value::String(_)) => 1,
            _ => match params.get("threshold") {
                Some(Value::Array(items)) => items.len().max(1),
                _ => 1,
            },
        };
        params.insert(
            "threshold".to_string(),
            Value::Array((0..count).map(|_| Value::from(value)).collect()),
        );
    }
    if let Some([x, y, w, h]) = roi {
        params.insert("roi".to_string(), serde_json::json!([x, y, w, h]));
    }
}

fn first_number(params: &Map<String, Value>, key: &str) -> Option<f64> {
    match params.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::Array(items) => items.first().and_then(Value::as_f64),
        _ => None,
    }
}

fn from_event(node: &str, event: &NodeEvent, threshold: Option<f64>, roi: Option<[i32; 4]>) -> Trial {
    Trial {
        node: node.to_string(),
        hit: event.hit,
        algorithm: String::new(),
        score: event.score,
        box_rect: event.box_rect,
        candidates: event.candidates,
        filtered: event.filtered,
        threshold,
        roi,
        error: None,
    }
}

/// Runs the node's recognition on one frame and reads back what the framework
/// reported. `post_recognition` hands back a *task* id, so the notification
/// payload is the reliable place to get the recognition result from.
pub fn trial_node(
    resource: &Resource,
    tasker: &Tasker,
    bus: &EventBus,
    node: &str,
    png: &[u8],
    threshold: Option<f64>,
    roi: Option<[i32; 4]>,
) -> Result<Trial> {
    let raw = resource.get_node_data(node)?;
    let Some(raw) = raw else {
        return Ok(Trial { node: node.to_string(), error: Some("节点不存在".into()), ..Default::default() });
    };
    let parsed: Value = serde_json::from_str(&raw)?;
    let Some((algorithm, mut params)) = recognition_of(&parsed) else {
        return Ok(Trial {
            node: node.to_string(),
            error: Some("节点没有识别配置".into()),
            ..Default::default()
        });
    };

    let used_threshold = first_number(&params, "threshold");
    let used_roi = params.get("roi").and_then(parse_box);
    apply_overrides(&mut params, threshold, roi);

    let mut buffer = MaaImageBuffer::new()?;
    buffer.set_encoded(png)?;
    bus.drain();

    let job = tasker.post_recognition(&algorithm, &serde_json::to_string(&params)?, &buffer)?;
    job.wait();
    let _ = job;
    let events = bus.drain();
    match events.into_iter().find(|event| event.kind == "recognition") {
        Some(event) => Ok(from_event(node, &event, used_threshold, used_roi)),
        None => Ok(Trial {
            node: node.to_string(),
            algorithm,
            error: Some("框架未回报识别结果".into()),
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_v1_params_are_collected() {
        let node: Value = serde_json::json!({
            "recognition": "TemplateMatch", "template": ["Soldier"],
            "threshold": 0.95, "roi": [46, 584, 1191, 135], "green_mask": true,
            "action": "Click", "next": "DeploySoldier", "focus": "FindSoldier!"
        });
        let (kind, params) = recognition_of(&node).unwrap();
        assert_eq!(kind, "TemplateMatch");
        assert_eq!(params.len(), 4, "只收识别参数，不收 action/next/focus");
        assert!(params.contains_key("template"));
        assert!(!params.contains_key("next"));
    }

    #[test]
    fn nested_v2_params_are_accepted() {
        let node: Value = serde_json::json!({
            "recognition": { "type": "OCR", "param": { "expected": ["回营"], "roi": [1,2,3,4] } }
        });
        let (kind, params) = recognition_of(&node).unwrap();
        assert_eq!(kind, "OCR");
        assert_eq!(params.get("expected").unwrap(), &serde_json::json!(["回营"]));
    }

    #[test]
    fn nodes_without_recognition_are_reported() {
        let node: Value = serde_json::json!({ "action": "Click" });
        assert!(recognition_of(&node).is_none());
    }

    #[test]
    fn threshold_override_matches_template_count() {
        let mut params: Map<String, Value> =
            serde_json::from_str(r#"{"template":["a","b","c"],"threshold":[0.9,0.9,0.9]}"#).unwrap();
        apply_overrides(&mut params, Some(0.5), Some([0, 0, 10, 10]));
        assert_eq!(params["threshold"], serde_json::json!([0.5, 0.5, 0.5]));
        assert_eq!(params["roi"], serde_json::json!([0, 0, 10, 10]));
    }

    #[test]
    fn scalar_template_still_gets_one_threshold() {
        let mut params: Map<String, Value> =
            serde_json::from_str(r#"{"template":"Next","threshold":0.7}"#).unwrap();
        apply_overrides(&mut params, Some(0.4), None);
        assert_eq!(params["threshold"], serde_json::json!([0.4]));
    }
}
