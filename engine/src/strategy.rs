//! Small battle presets: override deployment order without duplicating the pipeline.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    #[default]
    SpellsFirst,
    UnitsFirst,
}

impl Strategy {
    pub fn parse(name: &str) -> crate::Result<Self> {
        match name {
            "spells-first" => Ok(Self::SpellsFirst),
            "units-first" => Ok(Self::UnitsFirst),
            _ => Err(format!("未知策略 {name}，可选 spells-first 或 units-first").into()),
        }
    }

    pub fn pipeline_override(self) -> Value {
        let next = match self {
            Self::SpellsFirst => ["ReturnHome", "FindSpell", "FindUnit", "FindSiege", "FindHero"],
            Self::UnitsFirst => ["ReturnHome", "FindUnit", "FindSiege", "FindHero", "FindSpell"],
        };
        json!({"Deploy": {"next": next}})
    }
}

#[cfg(test)]
mod tests {
    use super::Strategy;

    #[test]
    fn presets_keep_end_detection_first_and_deploy_each_category() {
        for strategy in [Strategy::SpellsFirst, Strategy::UnitsFirst] {
            let data = strategy.pipeline_override();
            let next = data["Deploy"]["next"].as_array().unwrap();
            assert_eq!(next[0], "ReturnHome");
            for name in ["FindSpell", "FindUnit", "FindSiege", "FindHero"] {
                assert_eq!(next.iter().filter(|v| **v == name).count(), 1);
            }
        }
        assert_eq!(
            Strategy::UnitsFirst.pipeline_override()["Deploy"]["next"][1],
            "FindUnit"
        );
        assert_eq!(Strategy::default(), Strategy::SpellsFirst);
        assert!(Strategy::parse("unknown").is_err());
    }
}
