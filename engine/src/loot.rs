//! Filter opponents before deployment, using their advertised available loot.
use maa_framework::{custom::FnRecognition, resource::Resource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;

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

pub fn register(resource: &Resource) -> crate::Result<()> {
    let skips = Mutex::new((0_i64, 0_u32));
    resource.register_custom_recognition(
        "AvailableLoot",
        Box::new(FnRecognition::new(move |context, args| {
            let param: Value = serde_json::from_str(args.param).ok()?;
            let filter: LootFilter = serde_json::from_value(param["filter"].clone()).ok()?;
            filter.validate().ok()?;
            let accept = param["accept_next"].as_str()?;
            let read = |y| {
                if args.image.width() != 1280 || args.image.height() != 720 {
                    return None;
                }
                let result = context
                    .run_recognition_direct(
                        "OCR",
                        &json!({"roi":[94,y,125,25],"only_rec":false,"threshold":0.9}).to_string(),
                        args.image,
                    )
                    .ok()??;
                crate::storage::ocr_number(&result.detail, false).filter(|v| *v <= 1_000_000_000)
            };
            let loot = read(98).zip(read(137)).zip(read(176)).map(|((g, e), d)| [g, e, d]);
            let mut state = skips.lock().ok()?;
            if state.0 != args.task_id {
                *state = (args.task_id, 0);
            }
            let next = match loot {
                None => "LootReadFailed",
                Some(values) if filter.accepts(values) => {
                    state.1 = 0;
                    accept
                }
                Some(_) => {
                    state.1 += 1;
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
            Some((*args.roi, json!({"loot":loot,"skips":state.1,"next":next}).to_string()))
        })),
    )?;
    Ok(())
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
                if strategy == crate::Strategy::SpellsFirst {
                    "SlideLeft"
                } else {
                    "SpreadZoom"
                }
            );
        }
        assert!(LootFilter {
            max_skips: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
