//! Compare village resource counters with the capacity tooltip, never a bar estimate.
use maa_framework::{custom::FnRecognition, resource::Resource};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReading {
    pub current: u64,
    pub capacity: Option<u64>,
    pub read_at: f64,
}

fn read_time() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

#[derive(Clone, Default)]
pub(crate) struct CapacityCache(Arc<Mutex<[Option<u64>; 3]>>, Arc<Mutex<[Option<ResourceReading>; 3]>>);

impl CapacityCache {
    pub fn readings(&self) -> [Option<ResourceReading>; 3] {
        *self.1.lock().unwrap()
    }
    pub fn reset(&self) {
        *self.0.lock().unwrap() = [None; 3];
        *self.1.lock().unwrap() = [None; 3];
    }
}

fn below_capacity(current: Option<u64>, capacity: Option<u64>) -> bool {
    current.zip(capacity).is_some_and(|(current, max)| current < max)
}

fn number(text: &str) -> Option<u64> {
    let grouped: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if grouped.contains('.') {
        let groups: Vec<_> = grouped.split('.').collect();
        if groups[0].is_empty() || groups.iter().skip(1).any(|g| g.len() != 3) {
            return None;
        }
    }
    let compact: String = grouped.chars().filter(|c| *c != ',' && *c != '.').collect();
    if compact.is_empty() || !compact.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    compact.parse().ok()
}

fn capacity(text: &str) -> Option<u64> {
    let text = text.replace('：', ":");
    let (label, amount) = text.split_once(':')?;
    if !label.contains("最大储量") && !label.to_ascii_lowercase().contains("max") {
        return None;
    }
    number(amount).filter(|v| (1_000..=1_000_000_000).contains(v))
}

pub(crate) fn ocr_number(detail: &Value, capacity_line: bool) -> Option<u64> {
    let results = detail.get("filtered")?.as_array()?;
    if results.len() != 1 {
        return None;
    }
    let item = &results[0];
    if item.get("score")?.as_f64()? < 0.9 {
        return None;
    }
    let text = item.get("text")?.as_str()?;
    if capacity_line {
        capacity(text)
    } else {
        number(text)
    }
}

pub(crate) fn register(resource: &Resource) -> crate::Result<CapacityCache> {
    let cache = CapacityCache::default();
    let observed = cache.clone();
    resource.register_custom_recognition(
        "ResourceCapacity",
        Box::new(FnRecognition::new(move |context, args| {
            let param: Value = serde_json::from_str(args.param).ok()?;
            let index = param["resource"].as_u64()? as usize;
            let (counter_y, capacity_y) = *[(27, 83), (95, 152), (160, 220)].get(index)?;
            let width = args.image.width();
            if args.image.height() != 720 || width != 1280 {
                return None;
            }
            let read = |roi, is_capacity| {
                // Whole-line recognition preserves spaced groups on normal bars.
                // Full gold bars need text detection to remove the yellow margins.
                let mut last_detail = json!(null);
                for only_rec in if is_capacity { &[true][..] } else { &[true, false][..] } {
                    let params = json!({"roi": roi, "only_rec": only_rec, "threshold": 0.9});
                    let Some(result) = context
                        .run_recognition_direct("OCR", &params.to_string(), args.image)
                        .ok().flatten() else { continue; };
                    if let Some(value) = ocr_number(&result.detail, is_capacity) {
                        return Some(value);
                    }
                    last_detail = result.detail;
                }
                eprintln!("仓库 OCR 读取失败 roi={roi:?}: {}", last_detail["filtered"]);
                None
            };
            let current = read([width - 251, counter_y, 154, 31], false);
            let max = read([width - 290, capacity_y, 232, 23], true);
            let known = current.zip(max);
            if let Some((current, max)) = known {
                observed.0.lock().unwrap()[index] = Some(max);
                observed.1.lock().unwrap()[index] = Some(ResourceReading {current, capacity:Some(max), read_at:read_time()});
                eprintln!("仓库检查 {}: {current}/{max}", ["金币", "圣水", "黑油"][index]);
            }
            let hit = match param["state"].as_str()? {
                "full" => known.is_some_and(|(current, max)| current >= max),
                "unknown" => known.is_none(),
                _ => false,
            };
            hit.then(|| (*args.roi, json!({"current":current,"capacity":max}).to_string()))
        })),
    )?;
    let cached = cache.clone();
    resource.register_custom_recognition(
        "ResourceBelowCapacity",
        Box::new(FnRecognition::new(move |context, args| {
            if args.image.width() != 1280 || args.image.height() != 720 {
                return None;
            }
            let limits = *cached.0.lock().unwrap();
            let mut not_full = None;
            // Keep all three historical counters available to the status panel.
            // Check dark elixir first: gold and elixir often fill up earlier.
            for index in (0..3).rev() {
                let max = limits[index];
                let roi = [1029, [27, 95, 160][index], 154, 31];
                for only_rec in [true, false] {
                    let params = json!({"roi":roi,"only_rec":only_rec,"threshold":0.9});
                    let Some(result) = context.run_recognition_direct(
                        "OCR", &params.to_string(), args.image).ok().flatten() else { continue; };
                    let current = ocr_number(&result.detail, false);
                    if let Some(current) = current {
                        cached.1.lock().unwrap()[index] = Some(ResourceReading {current, capacity:max, read_at:read_time()});
                    }
                    if not_full.is_none() && below_capacity(current, max) {
                        not_full = Some((index, current.unwrap(), max.unwrap()));
                    }
                    if current.is_some() { break; }
                }
            }
            // Cached data never confirms full storage. Open fresh tooltips instead.
            not_full.map(|(index, current, max)| {
                eprintln!("仓库快速检查 {}: {current}/{max}，未满", ["金币", "圣水", "黑油"][index]);
                (*args.roi, json!({"resource":index,"current":current,"cached_capacity":max}).to_string())
            })
        })),
    )?;
    Ok(cache)
}

pub fn pipeline_override(enabled: bool) -> Value {
    if enabled {
        json!({"AttackStart":{"action":"DoNothing","next":"StorageQuickGate"}})
    } else {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_capacity_can_only_prove_not_full() {
        assert!(below_capacity(Some(409_224), Some(500_000)));
        assert!(!below_capacity(Some(500_000), Some(500_000)));
        assert!(!below_capacity(Some(501_000), Some(500_000)));
        assert!(!below_capacity(None, Some(500_000)));
        assert!(!below_capacity(Some(409_224), None));
        let cache = CapacityCache::default();
        cache.0.lock().unwrap()[2] = Some(500_000);
        cache.reset();
        assert_eq!(*cache.0.lock().unwrap(), [None; 3]);
    }
    #[test]
    fn grouped_counts_and_overflow_rewards_are_supported() {
        assert_eq!(number("31 000 000"), Some(31_000_000));
        assert_eq!(number("31,000,000"), Some(31_000_000));
        assert_eq!(number("31.000.000"), Some(31_000_000));
        assert_eq!(number("31 000.000"), Some(31_000_000));
        assert_eq!(number("31000.000"), Some(31_000_000));
        assert_eq!(number("31.5"), None);
        assert_eq!(capacity("最大储量：500 000"), Some(500_000));
        assert!(number("32 000 000").unwrap() >= capacity("最大储量:31 000 000").unwrap());
    }
    #[test]
    fn full_branch_closes_each_popup_before_reading_the_next_resource() {
        let pipeline: Value = serde_json::from_str(include_str!("../../assets/pipeline/storage.json")).unwrap();
        assert_eq!(pipeline["StorageFullGold"]["next"], "StorageFullCloseGold");
        assert_eq!(pipeline["StorageFullCloseGold"]["next"], "StorageOpenElixir");
        assert_eq!(pipeline["StorageFullCloseElixir"]["next"], "StorageOpenDark");
        assert_eq!(pipeline["StorageFullDark"]["next"], "StorageFullClose");
        assert_eq!(pipeline["StorageFullClose"]["next"], "StorageFullStop");
        assert_eq!(pipeline["StorageFullStop"]["action"], "StopTask");
        assert_eq!(pipeline_override(false), json!({}));
    }
    #[test]
    fn unknown_capacity_dismisses_tooltip_and_continues_farming() {
        let pipeline: Value = serde_json::from_str(include_str!("../../assets/pipeline/storage.json")).unwrap();
        for resource in ["Elixir", "Dark"] {
            assert_eq!(pipeline[format!("StorageUnknown{resource}")]["next"], "StorageReadFailed");
        }
        assert_eq!(pipeline["StorageUnknownGold"]["next"], "StorageRetryGold");
        assert_eq!(pipeline["StorageRetryGold"]["next"], "StorageReopenGoldRetry");
        assert_eq!(pipeline["StorageReopenGoldRetry"]["next"], "StorageRetryGateGold");
        assert_eq!(pipeline["StorageRetryGateGold"]["next"], json!(["StorageFullGold", "StorageRetryUnknownGold", "StorageCloseGold"]));
        assert_eq!(pipeline["StorageRetryUnknownGold"]["next"], "StorageReadFailed");
        assert_eq!(pipeline["StorageReadFailed"]["action"], "Click");
        assert_eq!(pipeline["StorageReadFailed"]["next"], "StorageContinue");
    }

    #[test]
    fn missing_ambiguous_or_untrusted_ocr_is_unknown() {
        assert_eq!(capacity("每小时产量:500 000"), None);
        assert_eq!(capacity("最大储量:0"), None);
        assert_eq!(number("31O00000"), None);
        assert_eq!(
            ocr_number(&json!({"filtered":[{"text":"500000","score":0.89}]}), false),
            None
        );
        assert_eq!(
            ocr_number(
                &json!({"filtered":[{"text":"500000","score":0.99},{"text":"1","score":0.99}]}),
                false
            ),
            None
        );
    }
}
