//! Compare village resource counters with the capacity tooltip, never a bar estimate.
use maa_framework::{custom::FnRecognition, resource::Resource};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReading {
    pub current: Option<u64>,
    pub capacity: Option<u64>,
    pub read_at: Option<f64>,
    pub current_failed: bool,
    pub capacity_failed: bool,
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
    fn observe(&self, index: usize, current: Option<u64>, capacity: Option<u64>, check_capacity: bool) {
        if let Some(max) = capacity {
            self.0.lock().unwrap()[index] = Some(max);
        }
        let max = self.0.lock().unwrap()[index];
        let mut readings = self.1.lock().unwrap();
        let reading = readings[index].get_or_insert(ResourceReading {
            current: None, capacity: max, read_at: None,
            current_failed: false, capacity_failed: false,
        });
        reading.current_failed = current.is_none();
        if let Some(value) = current {
            reading.current = Some(value);
            reading.read_at = Some(read_time());
        }
        reading.capacity = max;
        if check_capacity { reading.capacity_failed = capacity.is_none(); }
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
    if grouped.contains(['.', ',', '-']) {
        let groups: Vec<_> = grouped.split(['.', ',', '-']).collect();
        // OCR sometimes drops a grouping separator (31.000000) or reads it as '-'.
        // Require complete three-digit groups; never accept decimal fractions.
        if groups[0].is_empty() || groups.iter().skip(1).any(|g| g.is_empty() || g.len() % 3 != 0) {
            return None;
        }
    }
    let compact: String = grouped.chars().filter(|c| *c != ',' && *c != '.' && *c != '-').collect();
    if compact.is_empty() || !compact.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    compact.parse().ok()
}

fn capacity(text: &str) -> Option<u64> {
    let text = text.replace('：', ":").replace("最人储量", "最大储量");
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

// Use detected text positions instead of the pre-update resource-row spacing.
// Never compact a list of successful reads: a missing elixir must not become dark elixir.
fn counter_values(detail: &Value) -> [Option<u64>; 3] {
    let mut layouts = [[None; 3]; 2];
    let rows = [[38, 87, 136], [42, 110, 175]];
    if let Some(items) = detail["filtered"].as_array() {
        for item in items {
            let Some(rect) = item["box"].as_array().filter(|r| r.len() == 4) else { continue; };
            let Some(y) = rect[1].as_i64().zip(rect[3].as_i64()).map(|(y,h)| y+h/2) else { continue; };
            let Some(value) = ocr_number(&json!({"filtered":[item]}), false) else { continue; };
            for (layout, centers) in layouts.iter_mut().zip(rows) {
                for (slot, center) in layout.iter_mut().zip(centers) {
                    if (y-center).abs() <= 10 {
                        // Ambiguous boxes are rejected rather than choosing a number.
                        *slot = match *slot { None => Some(Some(value)), Some(_) => Some(None) };
                    }
                }
            }
        }
    }
    let layouts = layouts.map(|slots| slots.map(Option::flatten));
    let counts = layouts.map(|slots| slots.iter().filter(|v| v.is_some()).count());
    match counts[0].cmp(&counts[1]) {
        std::cmp::Ordering::Greater => layouts[0],
        std::cmp::Ordering::Less => layouts[1],
        std::cmp::Ordering::Equal => std::array::from_fn(|i| {
            (layouts[0][i] == layouts[1][i]).then_some(layouts[0][i]).flatten()
        }),
    }
}

// The tooltip can overlap the other bars. Anchor the amount to its maximum-
// capacity label, not a fixed Y coordinate or a nearby numeric line.
fn capacity_value(detail: &Value) -> Option<u64> {
    let items = detail["filtered"].as_array()?;
    let mut values = Vec::new();
    for label in items {
        if label["score"].as_f64()? < 0.9 { continue; }
        let text = label["text"].as_str()?;
        if let Some(value) = capacity(text) { values.push(value); continue; }
        let normalized = text.replace('：', ":");
        if normalized.trim_end_matches(':').trim() != "最大储量"
            && !normalized.trim_end_matches(':').trim().eq_ignore_ascii_case("max storage") { continue; }
        let rect = label["box"].as_array()?;
        let right = rect[0].as_i64()? + rect[2].as_i64()?;
        let center = rect[1].as_i64()? + rect[3].as_i64()?/2;
        for amount in items {
            let Some(value) = ocr_number(&json!({"filtered":[amount]}), false) else { continue; };
            let r = amount["box"].as_array()?;
            let y = r[1].as_i64()? + r[3].as_i64()?/2;
            if r[0].as_i64()? >= right - 12 && (y-center).abs() <= 6
                && (1_000..=1_000_000_000).contains(&value) { values.push(value); }
        }
    }
    (values.len() == 1).then(|| values[0])
}

fn read_counters(context: &maa_framework::context::Context, image: &maa_framework::buffer::MaaImageBuffer) -> [Option<u64>; 3] {
    let params = json!({"roi":[1020,15,165,180],"only_rec":false,"threshold":0.9});
    context.run_recognition_direct("OCR", &params.to_string(), image).ok().flatten()
        .map(|result| counter_values(&result.detail)).unwrap_or([None;3])
}

pub(crate) fn register(resource: &Resource) -> crate::Result<CapacityCache> {
    let cache = CapacityCache::default();
    let observed = cache.clone();
    resource.register_custom_recognition(
        "ResourceCapacity",
        Box::new(FnRecognition::new(move |context, args| {
            let param: Value = serde_json::from_str(args.param).ok()?;
            let index = param["resource"].as_u64()? as usize;
            if index >= 3 { return None; }
            let width = args.image.width();
            if args.image.height() != 720 || width != 1280 {
                observed.observe(index, None, None, true);
                return None;
            }
            let current = read_counters(context, args.image)[index];
            let mut max = None;
            // The transparent tooltip overlaps the other bars. Keep bright text
            // first so dim digits behind it cannot become part of the capacity.
            for filter in ["StorageTooltipText", ""] {
                let params = json!({"roi":[960,60,285,205],"only_rec":false,"threshold":0.9,"color_filter":filter});
                max = context.run_recognition_direct("OCR", &params.to_string(), args.image)
                    .ok().flatten().and_then(|r| capacity_value(&r.detail));
                if max.is_some() { break; }
            }
            let known = current.zip(max);
            observed.observe(index, current, max, true);
            if let Some((current, max)) = known {
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
                for index in 0..3 { cached.observe(index, None, None, false); }
                return None;
            }
            let limits = *cached.0.lock().unwrap();
            let counters = read_counters(context, args.image);
            let mut not_full = None;
            // Keep all three historical counters available to the status panel.
            // Check dark elixir first: gold and elixir often fill up earlier.
            for index in (0..3).rev() {
                let max = limits[index];
                let best = counters[index];
                cached.observe(index, best, None, false);
                if let Some(current) = best {
                    if not_full.is_none() && below_capacity(Some(current), max) {
                        not_full = Some((index, current, max.unwrap()));
                    }
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
    fn failed_reads_preserve_history_and_recovery_clears_only_the_recovered_field() {
        let cache = CapacityCache::default();
        cache.observe(2, None, None, true);
        let empty = cache.readings()[2].unwrap();
        assert_eq!(empty.current, None);
        assert_eq!(empty.read_at, None);
        assert!(empty.current_failed && empty.capacity_failed);
        cache.observe(2, Some(499_962), Some(500_000), true);
        let good = cache.readings()[2].unwrap();
        cache.observe(2, None, None, true);
        let failed = cache.readings()[2].unwrap();
        assert_eq!(failed.current, good.current);
        assert_eq!(failed.capacity, good.capacity);
        assert_eq!(failed.read_at, good.read_at);
        assert!(failed.current_failed && failed.capacity_failed);
        cache.observe(2, Some(500_000), None, false);
        let partial = cache.readings()[2].unwrap();
        assert_eq!(partial.current, Some(500_000));
        assert!(!partial.current_failed && partial.capacity_failed);
        cache.observe(2, Some(500_000), Some(500_000), true);
        let recovered = cache.readings()[2].unwrap();
        assert!(!recovered.current_failed && !recovered.capacity_failed);
        cache.reset();
        assert!(cache.readings().iter().all(Option::is_none));
    }

    #[test]
    fn tooltip_capacity_is_anchored_to_the_label_row() {
        let detail = json!({"filtered":[
            {"box":[1013,175,99,27],"score":0.99,"text":"最大储量："},
            {"box":[1107,176,99,22],"score":0.99,"text":"500000"},
            {"box":[1108,202,46,20],"score":0.99,"text":"409"}
        ]});
        assert_eq!(capacity_value(&detail), Some(500_000));
        let mut missing = detail.clone();
        missing["filtered"][0]["text"] = json!("每小时产量：");
        assert_eq!(capacity_value(&missing), None);
        let mut low = detail;
        low["filtered"][1]["score"] = json!(0.5);
        assert_eq!(capacity_value(&low), None);
    }

    #[test]
    fn compact_and_legacy_rows_do_not_shift_missing_resources() {
        let detail = |ys: [i32;3]| json!({"filtered": ys.into_iter().zip(["31000000","29827273","386614"]).map(|(y,text)| json!({"box":[1050,y-10,120,20],"score":0.99,"text":text})).collect::<Vec<_>>()});
        for ys in [[38,87,136], [42,110,175]] {
            assert_eq!(counter_values(&detail(ys)), [Some(31_000_000),Some(29_827_273),Some(386_614)]);
        }
        let mut missing = detail([38,87,136]);
        missing["filtered"][1]["score"] = json!(0.5);
        assert_eq!(counter_values(&missing), [Some(31_000_000),None,Some(386_614)]);
        missing["filtered"][2]["text"] = json!("最大储量:500000");
        assert_eq!(counter_values(&missing), [Some(31_000_000),None,None]);
    }

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
        assert_eq!(number("500-000"), Some(500_000));
        assert_eq!(capacity("最大储量：31.000000"), Some(31_000_000));
        assert_eq!(number("-500000"), None);
        assert_eq!(number("500-00"), None);
        assert_eq!(capacity("最大储量：500 000"), Some(500_000));
        assert_eq!(capacity("最人储量：31000000"), Some(31_000_000));
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
