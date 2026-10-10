//! Custom army configuration. Inspect every requested card before removing troops.
use maa_framework::{custom::FnRecognition, resource::Resource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmyCard {
    pub id: String,
    pub group: String,
    pub name: String,
    pub template: String,
    pub current_template: String,
    pub preview: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArmyEntry {
    pub id: String,
    pub count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ArmyPlan {
    pub entries: Vec<ArmyEntry>,
}

impl ArmyPlan {
    pub fn validate(&self, cards: &[ArmyCard]) -> crate::Result<()> {
        let mut seen = BTreeSet::new();
        let mut groups = BTreeMap::new();
        if self.entries.is_empty() {
            return Err("请先添加卡牌".into());
        }
        for entry in &self.entries {
            if !(1..=400).contains(&entry.count) || !seen.insert(&entry.id) {
                return Err("卡牌数量必须是 1～400，且不能重复".into());
            }
            let card = cards.iter().find(|c| c.id == entry.id).ok_or("未知卡牌")?;
            *groups.entry(&card.group).or_insert(0) += 1;
        }
        if groups
            .iter()
            .any(|(group, count)| *count > if *group == "Siege" { 3 } else { 6 })
        {
            return Err("第一版支持最多 6 种兵、6 种法术和 3 种战车".into());
        }
        Ok(())
    }
}

fn counter(group: &str) -> [i32; 4] {
    match group {
        "Unit" => [580, 159, 104, 32],
        "Spell" => [590, 330, 46, 32],
        _ => [1018, 330, 42, 32],
    }
}

fn ratio(text: &str) -> Option<(u32, u32)> {
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '!' | '！'))
        .collect();
    let (used, max) = text.split_once('/')?;
    let (used, max) = (used.parse().ok()?, max.parse().ok()?);
    (max > 0 && used <= max).then_some((used, max))
}

fn quantity(text: &str) -> Option<u32> {
    let text = text.trim().trim_start_matches(['x', 'X', '×']);
    let text: String = text
        .chars()
        .map(|c| match c {
            'I' | 'l' => '1',
            'O' => '0',
            _ => c,
        })
        .collect();
    text.parse().ok()
}

fn single_text(detail: &Value, minimum: f64) -> Option<&str> {
    let values = detail["filtered"].as_array()?;
    if values.len() != 1 || values[0]["score"].as_f64()? < minimum {
        return None;
    }
    values[0]["text"].as_str()
}

#[derive(Default)]
struct Inspection {
    task: i64,
    capacity: BTreeMap<String, u32>,
    costs: BTreeMap<String, u32>,
    planned: BTreeMap<String, u32>,
}

fn enlarged_crop(
    image: &maa_framework::buffer::MaaImageBuffer,
    roi: [i32; 4],
) -> Option<maa_framework::buffer::MaaImageBuffer> {
    let [x, y, width, height] = roi;
    if image.channels() != 3
        || x < 0
        || y < 0
        || width <= 0
        || height <= 0
        || x + width > image.width()
        || y + height > image.height()
    {
        return None;
    }
    let source = image.raw_data()?;
    let mut pixels = Vec::with_capacity((width * height * 16 * 3) as usize);
    for row in 0..height * 4 {
        for column in 0..width * 4 {
            let offset = (((y + row / 4) * image.width() + x + column / 4) * 3) as usize;
            pixels.extend_from_slice(&source[offset..offset + 3]);
        }
    }
    let mut crop = maa_framework::buffer::MaaImageBuffer::new().ok()?;
    crop.set(&pixels, width * 4, height * 4).ok()?;
    Some(crop)
}

pub fn register(resource: &Resource) -> crate::Result<()> {
    let inspection = Arc::new(Mutex::new(Inspection::default()));
    resource.register_custom_recognition("ArmyInspect", Box::new(FnRecognition::new(move |context, args| {
        let param: Value = serde_json::from_str(args.param).ok()?;
        let mode = param["mode"].as_str()?;
        let confidence = if mode == "quantity" {0.65} else if matches!(mode, "init" | "used" | "empty" | "totals") { 0.8 } else { 0.9 };
        let read = |roi| {
            let enlarged = if matches!(mode, "quantity" | "inspect") { enlarged_crop(args.image, roi) } else { None };
            let roi = if enlarged.is_some() { [0,0,roi[2]*4,roi[3]*4] } else { roi };
            let image = enlarged.as_ref().unwrap_or(args.image);
            let result = context.run_recognition_direct("OCR", &json!({"roi":roi,"only_rec":mode == "inspect", "threshold":confidence}).to_string(), image).ok()??;
            single_text(&result.detail, confidence).map(str::to_owned)
        };
        let mut state = inspection.lock().ok()?;
        if mode == "init" {
            *state = Inspection { task: args.task_id, ..Default::default() };
            for group in ["Unit", "Spell", "Siege"] {
                let (_, max) = ratio(&read(counter(group))?)?;
                if max > match group { "Unit" => 500, "Spell" => 30, _ => 5 } { return None; }
                state.capacity.insert(group.into(), max);
            }
            return Some((*args.roi, "容量已读取".into()));
        }
        if mode == "used" || mode == "empty" {
            let group = param["group"].as_str()?;
            let (used, _) = ratio(&read(counter(group))?)?;
            return (if mode == "used" { used > 0 } else { used == 0 }).then(|| (*args.roi, used.to_string()));
        }
        if mode == "totals" {
            if state.task != args.task_id { return None; }
            for group in ["Unit", "Spell", "Siege"] {
                let text = read(counter(group))?;
                let (used, _) = ratio(&text)?;
                let expected = *state.planned.get(group).unwrap_or(&0);
                if used != expected {
                    eprintln!("配卡容量核对失败: {group} 读取 {text}，预期 {expected}");
                    return None;
                }
            }
            return Some((*args.roi, "总容量核对通过".into()));
        }
        if mode == "ready" {
            if state.task != args.task_id { return None; }
            let plan: ArmyPlan = serde_json::from_value(param["plan"].clone()).ok()?;
            let mut totals = BTreeMap::<String, u32>::new();
            for entry in plan.entries {
                let group = entry.id.split_once('/')?.0.to_owned();
                let cost = *state.costs.get(&entry.id)?;
                let total = totals.entry(group).or_default();
                *total = total.checked_add(cost.checked_mul(entry.count)?)?;
            }
            if !totals.iter().all(|(group, total)| state.capacity.get(group).is_some_and(|max| total <= max)) { return None; }
            state.planned = totals.clone();
            return Some((*args.roi, json!({"used":totals,"capacity":state.capacity}).to_string()));
        }
        let card: ArmyCard = serde_json::from_value(param["card"].clone()).ok()?;
        let templates = if mode == "inspect" {
            vec![card.template.clone(), card.template.replace(".png", "-gray.png")]
        } else if mode == "quantity" { vec![card.current_template.clone()] }
        else { vec![card.template.clone()] };
        let roi = if mode == "quantity" {
            match card.group.as_str() { "Unit" => [554,193,665,113], "Spell" => [554,368,426,115], _ => [993,368,226,115] }
        } else { [55,440,1175,265] };
        let result = context.run_recognition_direct("TemplateMatch", &json!({"template":templates,"roi":roi,"method":10001,"green_mask":true,"threshold":if mode == "inspect" {0.98} else {0.95}}).to_string(), args.image).ok()??;
        if !result.hit { return None; }
        let bbox = result.box_rect;
        if mode == "inspect" {
            if state.task != args.task_id { return None; }
            let cost: u32 = read(if card.group == "Siege" { [bbox.x+94,bbox.y+215,27,25] } else if card.group == "Spell" { [bbox.x+47,bbox.y+86,23,27] } else { [bbox.x+37,bbox.y+86,46,27] })?.parse().ok()?;
            if cost == 0 || cost > 400 { return None; }
            eprintln!("配卡预检: {} 占用 {}", card.id, cost);
            state.costs.insert(card.id.clone(), cost);
        } else if mode == "quantity" {
            let text = read([bbox.x,bbox.y,43,25])?;
            let count = quantity(&text)?;
            if count != param["count"].as_u64()? as u32 { return None; }
        }
        let target = if mode == "find" {
            maa_framework::common::Rect { x: bbox.x + bbox.width/2 - 2, y: bbox.y + bbox.height/2 - 2, width: 4, height: 4 }
        } else { bbox };
        Some((target, json!({"card":card.id}).to_string()))
    })))?;
    Ok(())
}

/// Every search has a finite page budget. Failed checks lead to StopTask.
pub fn pipeline(plan: &ArmyPlan, cards: &[ArmyCard]) -> crate::Result<Value> {
    plan.validate(cards)?;
    let mut nodes = json!({
        "ArmyApply": {"next":["ArmyReload","ArmyOverview","ArmyOpenVillage"],"timeout":2000,"on_error":"ArmyFailed"},
        "ArmyReload":{"recognition":"OCR","expected":"重新载入游戏","roi":[250,400,380,70],"action":"Click","post_delay":4000,"next":"ArmyApply"},
        "ArmyOpenVillage":{"recognition":"TemplateMatch","template":"Army/UI/Open.png","roi":[30,470,100,110],"threshold":0.95,"action":"Click","next":"ArmyOverview","timeout":2000,"on_error":"ArmyFailed"},
        "ArmyOverview":{"recognition":"TemplateMatch","template":"Army/UI/Overview.png","roi":[380,0,100,140],"threshold":0.95,"action":"Click","target":true,"target_offset":[-150,0,20,30],"post_delay":500,"next":"ArmyCapacity","timeout":2000,"on_error":"ArmyFailed"},
        "ArmyCapacity":{"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"init"},"timeout":0,"on_error":"ArmyFailed"},
        "ArmyReady":{"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"ready","plan":plan},"timeout":0,"on_error":"ArmyFailed"},
        "ArmyComplete":{"action":"StopTask","focus":"配卡完成"},
        "ArmyFailed":{"action":"StopTask","focus":"配卡失败，已停止"}
    });
    let mut ordered: Vec<String> = vec!["ArmyCapacity".into()];
    for inspect in [true, false] {
        if !inspect {
            ordered.push("ArmyReady".into());
        }
        for group in ["Unit", "Spell", "Siege"] {
            let entries: Vec<_> = plan
                .entries
                .iter()
                .filter(|e| e.id.starts_with(&format!("{group}/")))
                .collect();
            let prefix = format!("Army{}_{}", if inspect { "Inspect" } else { "Add" }, group);
            let point = match group {
                "Unit" => [900, 245],
                "Spell" => [820, 414],
                _ => [1100, 414],
            };
            if !inspect {
                let used = format!("{prefix}Used");
                let clear = format!("{prefix}Clear");
                let confirm = format!("{prefix}Confirm");
                let skip = format!("{prefix}SkipClear");
                nodes[&used] = json!({"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"used","group":group},"next":clear,"timeout":0,"on_error":skip});
                let trash = match group {
                    "Unit" => [1207, 177],
                    "Spell" => [963, 346],
                    _ => [1207, 346],
                };
                nodes[&clear] =
                    json!({"action":"Click","target":[trash[0],trash[1],1,1],"next":confirm,"post_delay":300});
                nodes[&confirm] = json!({"recognition":"TemplateMatch","template":"Army/UI/Remove.png","roi":[320,150,640,410],"threshold":0.95,"action":"Click","target":[778,466,1,1],"post_delay":400,"timeout":1000,"on_error":"ArmyFailed"});
                nodes[&skip] = json!({"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"empty","group":group},"timeout":0,"on_error":"ArmyFailed"});
                ordered.push(used);
                // Both paths converge on the same next step, wired below.
                ordered.push(confirm);
                nodes[&skip]["next"] = json!(format!("{prefix}Open"));
            }
            if entries.is_empty() {
                continue;
            }
            let open = format!("{prefix}Open");
            nodes[&open] = json!({"action":"Click","target":[point[0],point[1],1,1],"post_delay":400});
            ordered.push(open);
            let reset = format!("{prefix}Reset");
            nodes[&reset] =
                json!({"action":"Swipe","begin":[140,520,1,1],"end":[1160,520,1,1],"duration":300,"post_delay":400});
            ordered.push(reset);
            for (i, entry) in entries.iter().enumerate() {
                let card = cards.iter().find(|c| c.id == entry.id).unwrap();
                let first = format!("{prefix}_{i}_0");
                for page in 0..7 {
                    let find = format!("{prefix}_{i}_{page}");
                    let swipe = format!("{find}Swipe");
                    nodes[&find] = json!({"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":if inspect {"inspect"} else {"find"},"card":card},"timeout":0,"on_error":if page==6 {"ArmyFailed".into()} else {swipe.clone()}});
                    if !inspect {
                        nodes[&find]["action"] = json!("Click");
                        nodes[&find]["post_delay"] = json!(120);
                    }
                    if page < 6 {
                        nodes[&swipe] = json!({"action":"Swipe","begin":[1120,520,1,1],"end":[150,520,1,1],"duration":300,"post_delay":400,"next":format!("{prefix}_{i}_{}",page+1)});
                    }
                }
                ordered.push(first.clone());
                if !inspect {
                    for n in 1..entry.count {
                        let repeat = format!("{prefix}_{i}Repeat{n}");
                        nodes[&repeat] = json!({"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"find","card":card},"action":"Click","post_delay":120,"timeout":0,"on_error":"ArmyFailed"});
                        ordered.push(repeat);
                    }
                }
                // Search again from the first page for the next card.
                let reset = format!("{prefix}_{i}Reset");
                nodes[&reset] = json!({"action":"Swipe","begin":[140,520,1,1],"end":[1160,520,1,1],"duration":300,"post_delay":400});
                ordered.push(reset);
            }
            let close = format!("{prefix}Close");
            nodes[&close] = json!({"action":"Click","target":[260,25,1,1],"post_delay":500});
            ordered.push(close);
            if !inspect {
                for (i, entry) in entries.iter().enumerate() {
                    let card = cards.iter().find(|c| c.id == entry.id).unwrap();
                    let verify = format!("{prefix}Verify{i}");
                    nodes[&verify] = json!({"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"quantity","card":card,"count":entry.count},"timeout":1000,"on_error":"ArmyFailed"});
                    ordered.push(verify);
                }
            }
        }
    }
    nodes["ArmyCheckTotals"] =
        json!({"recognition":"Custom","custom_recognition":"ArmyInspect","custom_recognition_param":{"mode":"totals"}});
    ordered.push("ArmyCheckTotals".into());
    nodes["ArmyClose"] = json!({"action":"Click","target":[1208,80,1,1],"post_delay":400});
    ordered.push("ArmyClose".into());
    ordered.push("ArmyComplete".into());
    for pair in ordered.windows(2) {
        // The 'used' branch must keep its clear/confirm chain.
        if !pair[0].ends_with("Used") {
            nodes[&pair[0]]["next"] = json!(pair[1]);
        }
        if pair[0].ends_with("_0") {
            let stem = pair[0].strip_suffix('0').unwrap();
            for page in 1..7 {
                nodes[format!("{stem}{page}")]["next"] = json!(pair[1]);
            }
        }
        if pair[0].ends_with("Confirm") {
            let skip = pair[0].replace("Confirm", "SkipClear");
            nodes[&skip]["next"] = json!(pair[1]);
        }
    }
    for node in nodes.as_object_mut().unwrap().values_mut() {
        if let Some(next) = node.get("next").and_then(Value::as_str).map(str::to_owned) {
            if next.ends_with("Used") {
                node["next"] = json!([next.clone(), next.replace("Used", "SkipClear")]);
            } else if next
                .rsplit_once('_')
                .is_some_and(|(_, suffix)| suffix.parse::<usize>().is_ok())
            {
                let fallback = if next.ends_with("_6") {
                    "ArmyFailed".into()
                } else {
                    format!("{next}Swipe")
                };
                node["next"] = json!([next, fallback]);
            }
        }
        if node["action"] == "Swipe" && node["begin"][0] == 140 {
            node["repeat"] = json!(6);
            node["duration"] = json!(150);
        }
        node["timeout"] = json!(1000);
        node["on_error"] = json!("ArmyFailed");
        if node.get("recognition").is_none() && matches!(node["action"].as_str(), Some("Click" | "Swipe")) {
            node["recognition"] = json!("ColorMatch");
            node["roi"] = json!([0, 0, 1, 1]);
            node["lower"] = json!([0, 0, 0]);
            node["upper"] = json!([255, 255, 255]);
            node["count"] = json!(1);
        }
    }
    Ok(nodes)
}

/// Inspect and check an existing army without deleting or adding cards.
pub fn verification_pipeline(plan: &ArmyPlan, cards: &[ArmyCard]) -> crate::Result<Value> {
    let mut nodes = pipeline(plan, cards)?;
    let mut checks = vec!["ArmyReady".to_owned()];
    for group in ["Unit", "Spell", "Siege"] {
        for i in 0..plan
            .entries
            .iter()
            .filter(|e| e.id.starts_with(&format!("{group}/")))
            .count()
        {
            checks.push(format!("ArmyAdd_{group}Verify{i}"));
        }
    }
    checks.push("ArmyCheckTotals".into());
    for pair in checks.windows(2) {
        nodes[&pair[0]]["next"] = json!(pair[1]);
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacities_require_a_complete_ratio() {
        assert_eq!(ratio("348 / 352"), Some((348, 352)));
        assert_eq!(ratio("352"), None);
        assert_eq!(ratio("353/352"), None);
        assert_eq!(ratio("0/0"), None);
    }
    #[test]
    fn quantities_normalize_ocr_strokes_without_accepting_arbitrary_text() {
        assert_eq!(quantity("XI"), Some(1));
        assert_eq!(quantity("×10"), Some(10));
        assert_eq!(quantity("XlO"), Some(10));
        assert_eq!(quantity("X?"), None);
    }

    #[test]
    fn verification_has_no_reachable_deletion_or_addition() {
        let cards: Vec<ArmyCard> = serde_json::from_str(include_str!("../../assets/army.json")).unwrap();
        let plan = ArmyPlan {
            entries: vec![ArmyEntry {
                id: cards[0].id.clone(),
                count: 1,
            }],
        };
        for (nodes, readonly) in [
            (pipeline(&plan, &cards).unwrap(), false),
            (verification_pipeline(&plan, &cards).unwrap(), true),
        ] {
            let mut pending = vec!["ArmyApply".to_owned()];
            let mut visited = BTreeSet::new();
            while let Some(name) = pending.pop() {
                if !visited.insert(name.clone()) {
                    continue;
                }
                let node = nodes.get(&name).unwrap_or_else(|| panic!("missing node {name}"));
                if readonly {
                    assert!(
                        !name.ends_with("Clear") && !name.ends_with("Confirm") && !name.contains("Repeat"),
                        "{name}"
                    );
                    assert_ne!(node["custom_recognition_param"]["mode"], "find");
                }
                for edge in ["next", "on_error"] {
                    if let Some(value) = node.get(edge) {
                        if let Some(value) = value.as_str() {
                            pending.push(value.into());
                        }
                        if let Some(values) = value.as_array() {
                            pending.extend(values.iter().map(|v| v.as_str().unwrap().to_owned()));
                        }
                    }
                }
            }
            assert!(visited.contains("ArmyCheckTotals"));
            assert!(visited.contains("ArmyComplete"));
        }
    }

    #[test]
    fn reject_unknown_duplicate_and_unbounded_counts() {
        let cards: Vec<ArmyCard> = serde_json::from_str(include_str!("../../assets/army.json")).unwrap();
        for entries in [
            vec![],
            vec![ArmyEntry {
                id: "unknown".into(),
                count: 1,
            }],
            vec![ArmyEntry {
                id: cards[0].id.clone(),
                count: 401,
            }],
            vec![
                ArmyEntry {
                    id: cards[0].id.clone(),
                    count: 1
                };
                2
            ],
        ] {
            assert!(ArmyPlan { entries }.validate(&cards).is_err());
        }
    }
}
