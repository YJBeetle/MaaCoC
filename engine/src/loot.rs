//! Filter opponents before deployment, using their advertised available loot.
use maa_framework::{custom::FnRecognition, resource::Resource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LootFilter {
    pub enabled: bool,
    pub min_gold: u64,
    pub min_elixir: u64,
    pub min_dark: u64,
    pub min_gold_elixir: u64,
    pub match_any: bool,
    pub max_skips: u32,
}
impl Default for LootFilter {
    fn default() -> Self {
        Self {
            enabled: false,
            min_gold: 0,
            min_elixir: 0,
            min_dark: 0,
            min_gold_elixir: 1_000_000,
            match_any: true,
            max_skips: 30,
        }
    }
}
impl LootFilter {
    pub fn validate(&self) -> crate::Result<()> {
        if self.max_skips == 0
            || self.max_skips > 1000
            || [self.min_gold, self.min_elixir, self.min_dark, self.min_gold_elixir]
                .iter()
                .any(|v| *v > 1_000_000_000)
        {
            return Err("收益门槛需为 0～10 亿的整数，连续跳过上限需为 1～1000".into());
        }
        Ok(())
    }
    fn accepts(&self, loot: [u64; 3]) -> bool {
        let values = [loot[0], loot[1], loot[2], loot[0].saturating_add(loot[1])];
        let thresholds = [self.min_gold, self.min_elixir, self.min_dark, self.min_gold_elixir];
        let enabled: Vec<_> = values
            .iter()
            .zip(thresholds)
            .filter(|(_, min)| *min > 0)
            .map(|(v, min)| *v >= min)
            .collect();
        enabled.is_empty()
            || if self.match_any {
                enabled.iter().any(|v| *v)
            } else {
                enabled.iter().all(|v| *v)
            }
    }
}

pub fn apply_override(pipeline: &mut Value, filter: &LootFilter) {
    if !filter.enabled {
        return;
    }
    let accept_next = pipeline["FindNext"]["next"].as_str().unwrap_or("SlideLeft").to_owned();
    pipeline["FindNext"] = json!({"next":"LootCheck"});
    pipeline["LootCheck"] = json!({"custom_recognition_param":{"filter":filter,"accept_next":accept_next}});
}

// 战利品三行的 Y 随对手名/徽章行数变化，固定 ROI 会错位读到空行。
// 一次 OCR 整个面板，按行聚类（同一行可能拆成多框）再按 Y 取金/水/黑。
fn panel_loot(detail: &Value) -> Option<[u64; 3]> {
    let mut boxes: Vec<(i64, i64, String)> = Vec::new();
    for item in detail["filtered"].as_array()? {
        if item["score"].as_f64()? < 0.6 {
            continue;
        }
        let Some(rect) = item["box"].as_array().filter(|r| r.len() == 4) else { continue };
        let (Some(x), Some(y), Some(h)) = (rect[0].as_i64(), rect[1].as_i64(), rect[3].as_i64()) else { continue };
        let Some(text) = item["text"].as_str() else { continue };
        boxes.push((y + h / 2, x, text.to_owned()));
    }
    boxes.sort_by_key(|(y, x, _)| (*y, *x));
    let mut lines: Vec<(i64, String)> = Vec::new();
    for (y, _x, text) in boxes {
        match lines.last_mut() {
            Some((ly, acc)) if (y - *ly).abs() <= 10 => {
                acc.push(' ');
                acc.push_str(&text);
            }
            _ => lines.push((y, text)),
        }
    }
    let values: Vec<u64> = lines
        .iter()
        .filter_map(|(_, t)| crate::storage::number(t))
        .filter(|v| *v <= 1_000_000_000)
        .collect();
    (values.len() == 3).then(|| [values[0], values[1], values[2]])
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LootDecision { Attack, Skip, SkipLimit }

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpponentLoot {
    pub amounts: [u64; 3],
    pub decision: LootDecision,
    pub skips: u32,
    pub max_skips: u32,
}

pub type LootReading = Arc<Mutex<Option<OpponentLoot>>>;

pub fn register(resource: &Resource) -> crate::Result<LootReading> {
    let reading = LootReading::default();
    let latest = reading.clone();
    let skips = Mutex::new((0_i64, 0_u32, 0_u32));
    resource.register_custom_recognition(
        "AvailableLoot",
        Box::new(FnRecognition::new(move |context, args| {
            let param: Value = serde_json::from_str(args.param).ok()?;
            let filter: LootFilter = serde_json::from_value(param["filter"].clone()).ok()?;
            filter.validate().ok()?;
            let accept = param["accept_next"].as_str()?;
            if args.image.width() != 1280 || args.image.height() != 720 {
                return None;
            }
            // 收益筛选只在准备页生效：没有"下一个"按钮说明已开战或仍在加载，
            // 直接走原流程（已开战则投放打完），绝不因读不到战利品而停局。
            let prep = context
                .run_recognition_direct(
                    "TemplateMatch",
                    &json!({"template":"Next","green_mask":true,"roi":[1005,440,253,146],"threshold":0.9}).to_string(),
                    args.image,
                )
                .ok()
                .flatten()
                .is_some();
            if !prep {
                context.override_next(args.node_name, &[accept]).ok()?;
                return Some((*args.roi, json!({"loot":null,"next":accept}).to_string()));
            }
            let panel = context
                .run_recognition_direct(
                    "OCR",
                    &json!({"roi":[90,70,180,160],"only_rec":false,"threshold":0.6}).to_string(),
                    args.image,
                )
                .ok()??;
            let loot = panel_loot(&panel.detail);
            let mut state = skips.lock().ok()?;
            if state.0 != args.task_id {
                *state = (args.task_id, 0, 0);
            }
            let next = match loot {
                // 点"下一个"后的换对手过渡帧面板为空，属正常瞬态；重试而非停局。
                None => {
                    state.2 += 1;
                    if state.2 > 6 {
                        "LootReadFailed"
                    } else {
                        "LootRetry"
                    }
                }
                Some(values) if filter.accepts(values) => {
                    state.1 = 0;
                    state.2 = 0;
                    accept
                }
                Some(_) => {
                    state.1 += 1;
                    state.2 = 0;
                    if state.1 > filter.max_skips {
                        "LootSkipLimit"
                    } else {
                        "LootSkip"
                    }
                }
            };
            eprintln!("收益筛选: {loot:?}，连续跳过 {}，下一步 {next}", state.1);
            if !context.override_next(args.node_name, &[next]).ok()? {
                return None;
            }
            if let Some(amounts) = loot {
                let decision = match next {
                    "LootSkip" => LootDecision::Skip,
                    "LootSkipLimit" => LootDecision::SkipLimit,
                    _ => LootDecision::Attack,
                };
                *latest.lock().ok()? = Some(OpponentLoot {
                    amounts, decision, skips: state.1, max_skips: filter.max_skips,
                });
            }
            Some((*args.roi, json!({"loot":loot,"skips":state.1,"next":next}).to_string()))
        })),
    )?;
    Ok(reading)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thresholds_support_any_all_zero_and_exact_boundary() {
        let mut f = LootFilter {
            min_dark: 8000,
            ..Default::default()
        };
        assert!(f.accepts([500_000, 500_000, 0]));
        assert!(f.accepts([0, 0, 8000]));
        assert!(!f.accepts([499_999, 500_000, 7999]));
        f.match_any = false;
        assert!(!f.accepts([500_000, 500_000, 7999]));
        assert!(f.accepts([500_000, 500_000, 8000]));
        f.min_gold_elixir = 0;
        f.min_dark = 0;
        assert!(f.accepts([0, 0, 0]));
    }
    #[test]
    fn filtering_preserves_the_camera_preset_and_resume_path() {
        for strategy in [
            crate::Strategy::SpellsFirst,
            crate::Strategy::LeftSpread,
            crate::Strategy::AirProbe,
        ] {
            let mut data = strategy.pipeline_override();
            let before = data["BattleReady"].clone();
            apply_override(
                &mut data,
                &LootFilter {
                    enabled: true,
                    ..Default::default()
                },
            );
            assert_eq!(data["BattleReady"], before);
            assert_eq!(data["FindNext"]["next"], "LootCheck");
            assert_eq!(
                data["LootCheck"]["custom_recognition_param"]["accept_next"],
                "SpreadZoom"
            );
        }
        assert!(LootFilter {
            max_skips: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
    }
    #[test]
    fn panel_loot_tolerates_shifted_rows_split_boxes_and_the_label() {
        let item = |x, y, w, h, score, text: &str| json!({"box":[x,y,w,h],"score":score,"text":text});
        let detail = |ys: [i64; 3]| json!({"filtered":[
            item(101, ys[0] - 24, 117, 17, 0.99, "可获得的战利品"),
            item(98, ys[0], 109, 21, 0.99, "1820646"),
            item(96, ys[1], 120, 26, 0.99, "1900646"),
            item(100, ys[2], 76, 24, 0.99, "11728"),
        ]});
        for ys in [[92, 127, 168], [103, 142, 180], [80, 118, 156]] {
            assert_eq!(panel_loot(&detail(ys)), Some([1_820_646, 1_900_646, 11_728]));
        }
        // 同一行被拆成两框时按 X 拼接。
        let split = json!({"filtered":[
            item(98, 92, 50, 21, 0.99, "1820"),
            item(150, 92, 59, 21, 0.99, "646"),
            item(96, 127, 120, 26, 0.99, "1900646"),
            item(100, 168, 76, 24, 0.99, "11728"),
        ]});
        assert_eq!(panel_loot(&split), Some([1_820_646, 1_900_646, 11_728]));
        // 缺行 / 低置信度 / 多出一行数字都判读取失败，而不是猜一个值。
        let missing = json!({"filtered":[
            item(98, 92, 109, 21, 0.99, "1820646"),
            item(96, 127, 120, 26, 0.99, "1900646"),
        ]});
        assert_eq!(panel_loot(&missing), None);
        let low = json!({"filtered":[
            item(98, 92, 109, 21, 0.99, "1820646"),
            item(96, 127, 120, 26, 0.99, "1900646"),
            item(100, 168, 76, 24, 0.5, "11728"),
        ]});
        assert_eq!(panel_loot(&low), None);
        let extra = json!({"filtered":[
            item(98, 92, 109, 21, 0.99, "1820646"),
            item(96, 127, 120, 26, 0.99, "1900646"),
            item(100, 168, 76, 24, 0.99, "11728"),
            item(100, 205, 76, 24, 0.99, "999"),
        ]});
        assert_eq!(panel_loot(&extra), None);
    }
}
