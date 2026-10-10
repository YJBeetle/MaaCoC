//! Fixed deployment steps, compiled to ordinary Maa pipeline nodes.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tactics {
    pub enabled: bool,
    pub prepare_camera: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub card: String,
    pub count: u32,
    pub x: u32,
    pub y: u32,
    pub delay_ms: u32,
    pub interval_ms: u32,
}

impl Tactics {
    pub fn validate(&self) -> crate::Result<()> {
        let catalog: Vec<crate::army::ArmyCard> = serde_json::from_str(include_str!("../../assets/army.json"))?;
        if self.steps.len() > 32 || (self.enabled && self.steps.is_empty()) {
            return Err("战术需包含 1～32 个步骤".into());
        }
        let mut taps = 0;
        for step in &self.steps {
            if !catalog.iter().any(|c| c.id == step.card)
                || step.count == 0 || step.count > 400
                || step.x >= 1280 || !(40..=560).contains(&step.y)
                || step.delay_ms > 60_000 || !(50..=5000).contains(&step.interval_ms)
            {
                return Err("战术卡牌或参数无效：数量 1～400，X 0～1279，Y 40～560，等待最多 60 秒，间隔 50～5000 毫秒".into());
            }
            taps += step.count;
        }
        if taps > 600 { return Err("战术点击总次数不能超过 600".into()); }
        Ok(())
    }

    pub fn apply_override(&self, pipeline: &mut Value) -> crate::Result<()> {
        self.validate()?;
        if !self.enabled { return Ok(()); }
        let first = "Tactic0Gate";
        let entry = if self.prepare_camera { "SpreadZoom" } else { first };
        pipeline["FindNext"] = json!({"next":entry});
        pipeline["BattleReady"] = json!({"next":entry});
        if self.prepare_camera { pipeline["SpreadPan"] = json!({"next":first}); }
        for (i, step) in self.steps.iter().enumerate() {
            let gate = format!("Tactic{i}Gate");
            let select = format!("Tactic{i}Select");
            let next = if i+1 == self.steps.len() { "Deploy".into() } else { format!("Tactic{}Gate", i+1) };
            // The surrender button proves this is still an active battle, including after a wait.
            pipeline[&gate] = json!({"focus":format!("战术 {}：{}",i+1,step.card),"recognition":"TemplateMatch","template":"Surrender","roi":[20,483,218,107],"threshold":0.95,"rate_limit":0,"pre_delay":step.delay_ms,"post_delay":0,"timeout":500,"next":[select,next],"on_error":"AttackLoop"});
            pipeline[&select] = json!({"recognition":"TemplateMatch","template":format!("Cards/{}.png",step.card),"green_mask":true,"method":10001,"roi":[46,584,1191,135],"threshold":0.95,"action":"Click","target_offset":[0,0,0,-30],"rate_limit":0,"pre_delay":0,"post_delay":50,"timeout":500,"next":format!("Tactic{i}Tap0"),"on_error":"AttackLoop"});
            for n in 0..step.count {
                let name = format!("Tactic{i}Tap{n}");
                let after = if n+1 == step.count { next.clone() } else { format!("Tactic{i}Tap{}", n+1) };
                // Refresh the controller image before each click; never reuse an empty action cache.
                pipeline[&name] = json!({"recognition":"TemplateMatch","template":"Surrender","roi":[20,483,218,107],"threshold":0.95,"action":"Click","target":[step.x,step.y,1,1],"rate_limit":0,"pre_delay":0,"post_delay":step.interval_ms,"timeout":500,"next":after,"on_error":"AttackLoop"});
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Tactics {
        Tactics { enabled:true, prepare_camera:true, steps:vec![Step {card:"Unit/0_Barbarian".into(),count:2,x:150,y:310,delay_ms:1000,interval_ms:100}] }
    }
    #[test]
    fn finite_steps_skip_missing_cards_and_keep_loot_filter_and_fallback() {
        let mut nodes = crate::Strategy::UnitsFirst.pipeline_override();
        plan().apply_override(&mut nodes).unwrap();
        assert_eq!(nodes["SpreadPan"]["next"],"Tactic0Gate");
        assert_eq!(nodes["Tactic0Gate"]["next"],json!(["Tactic0Select","Deploy"]));
        assert_eq!(nodes["Tactic0Select"]["on_error"],"AttackLoop");
        assert_eq!(nodes["Tactic0Tap1"]["next"],"Deploy");
        assert_eq!(nodes["Tactic0Tap1"]["on_error"],"AttackLoop");
        crate::loot::apply_override(&mut nodes,&crate::loot::LootFilter { enabled:true,..Default::default() });
        assert_eq!(nodes["LootCheck"]["custom_recognition_param"]["accept_next"],"SpreadZoom");
        assert_eq!(nodes["Deploy"]["next"][1],"FindUnit");
    }
    #[test]
    fn bounds_unknown_cards_and_empty_enabled_plan_are_rejected() {
        let mut p=plan(); p.steps[0].y=600; assert!(p.validate().is_err());
        p=plan(); p.steps[0].card="Unit/unknown".into(); assert!(p.validate().is_err());
        p=plan(); p.steps.clear(); assert!(p.validate().is_err());
        p.enabled=false; assert!(p.validate().is_ok());
        p=plan(); p.prepare_camera=false;
        let mut nodes=crate::Strategy::LeftSpread.pipeline_override();
        p.apply_override(&mut nodes).unwrap();
        assert_eq!(nodes["FindNext"]["next"],"Tactic0Gate");
    }
}
