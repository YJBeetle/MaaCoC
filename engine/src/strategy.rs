//! Battle presets: deployment order plus optional camera and placement actions.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    #[default]
    SpellsFirst,
    UnitsFirst,
    LeftSpread,
    AirProbe,
}

impl Strategy {
    pub fn parse(name: &str) -> crate::Result<Self> {
        match name {
            "spells-first" => Ok(Self::SpellsFirst),
            "units-first" => Ok(Self::UnitsFirst),
            "left-spread" => Ok(Self::LeftSpread),
            "air-probe" => Ok(Self::AirProbe),
            _ => Err(format!("未知策略 {name}，可选 spells-first、units-first 、left-spread 或 air-probe").into()),
        }
    }

    pub fn pipeline_override(self) -> Value {
        let next = match self {
            Self::SpellsFirst => ["ReturnHome", "FindSpell", "FindUnit", "FindSiege", "FindHero"],
            Self::UnitsFirst | Self::LeftSpread | Self::AirProbe => {
                ["ReturnHome", "FindUnit", "FindSiege", "FindHero", "FindSpell"]
            }
        };
        let mut pipeline = json!({"Deploy": {"next": next}});
        if matches!(self, Self::LeftSpread | Self::AirProbe) {
            pipeline["FindNext"] = json!({"next": "SpreadZoom"});
            // Resuming an existing battle also prepares the camera before placement.
            pipeline["BattleReady"] = json!({"next": "SpreadZoom"});
            pipeline["FindUnit"] = json!({"next": "SpreadUnit1"});
            pipeline["FindSiege"] = json!({"next": "SpreadSiege"});
            pipeline["FindHero"] = json!({"next": "SpreadHero"});
            pipeline["FindSpell"] = json!({"next": "SpreadSpell1"});
        }
        if self == Self::AirProbe {
            pipeline["SpreadPan"] = json!({"next": "AirOpening"});
        }
        pipeline
    }
}

#[cfg(test)]
mod tests {
    use super::Strategy;

    #[test]
    fn presets_keep_end_detection_first_and_deploy_each_category() {
        for strategy in [
            Strategy::SpellsFirst,
            Strategy::UnitsFirst,
            Strategy::LeftSpread,
            Strategy::AirProbe,
        ] {
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

    #[test]
    fn air_probe_is_an_optional_opening_not_a_deployment_loop() {
        let data = Strategy::AirProbe.pipeline_override();
        assert_eq!(data["SpreadPan"]["next"], "AirOpening");
        assert!(!data["Deploy"]["next"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("AirOpening")));
        let nodes: serde_json::Value =
            serde_json::from_str(include_str!("../../assets/pipeline/air-probe.json")).unwrap();
        assert_eq!(
            nodes["AirOpening"]["next"],
            serde_json::json!(["ReturnHome", "AirProbeBalloon", "Deploy"])
        );
        assert_eq!(nodes["AirProbe3"]["next"], "Deploy");
        assert_eq!(Strategy::parse("air-probe").unwrap(), Strategy::AirProbe);
    }

    #[test]
    fn spread_prepares_camera_once_before_the_deployment_loop() {
        let data = Strategy::LeftSpread.pipeline_override();
        assert_eq!(data["FindNext"]["next"], "SpreadZoom");
        assert_eq!(data["BattleReady"]["next"], "SpreadZoom");
        assert_eq!(data["FindUnit"]["next"], "SpreadUnit1");
        assert_eq!(data["FindSpell"]["next"], "SpreadSpell1");
        assert!(Strategy::UnitsFirst.pipeline_override().get("FindNext").is_none());
        let nodes: serde_json::Value =
            serde_json::from_str(include_str!("../../assets/pipeline/left-spread.json")).unwrap();
        assert_eq!(nodes["SpreadUnit3"]["next"], "Deploy");
        assert_eq!(nodes["SpreadPan"]["next"], "Deploy");
        assert_eq!(nodes["SpreadSpell5"]["next"], "Deploy");
        assert_eq!(nodes["SpreadSpell1"]["repeat"], 3);
        assert_eq!(nodes["SpreadHero"]["next"], "SpreadHeroWait");
        assert_eq!(nodes["SpreadHeroWait"]["next"], "SpreadHeroSkillReady");
        assert_eq!(nodes["SpreadHeroSkillReady"]["next"], "SkillHero");
        assert_eq!(nodes["SpreadHeroSkillReady"]["on_error"], "AttackLoop");
        assert_eq!(nodes["SpreadHeroSkillReady"]["recognition"], "TemplateMatch");
        for name in [
            "SpreadZoom",
            "SpreadZoomAgain",
            "SpreadPan",
            "SpreadUnit1",
            "SpreadUnit2",
            "SpreadUnit3",
        ] {
            assert_eq!(nodes[name]["on_error"], "AttackLoop");
        }
    }
}
