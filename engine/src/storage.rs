//! Compare village resource counters with the capacity tooltip, never a bar estimate.
use maa_framework::{custom::FnRecognition, resource::Resource};
use serde_json::{json, Value};

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

pub fn register(resource: &Resource) -> crate::Result<()> {
    resource.register_custom_recognition(
        "ResourceCapacity",
        Box::new(FnRecognition::new(|context, args| {
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
    Ok(())
}

pub fn pipeline_override(enabled: bool) -> Value {
    if enabled {
        json!({"AttackStart":{"action":"DoNothing","next":"StorageOpenGold"}})
    } else {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
