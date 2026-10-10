import type { ArmyCard, ArmyPlan, Settings, Strategy } from "./api";

const PREFIX = "MC1:";
const STRATEGIES: Strategy[] = ["spells-first", "units-first", "left-spread", "air-probe"];
// These indices are part of the share format: append new names, never reorder.
const NAMES: Record<string, string[]> = {
  e: ["SuperBarbarian", "SuperArcher", "SuperGiant", "SneakyGoblin", "SuperWallBreaker", "RocketBalloon", "SuperWizard", "SuperDragon", "InfernoDragon", "SuperMiner", "SuperYeti", "SuperMinion", "SuperHogRider", "SuperValkyrie", "SuperWitch", "IceHound", "SuperBowler"],
  s: ["SpellLightning", "SpellHealing", "SpellRage", "SpellJump", "SpellFreeze", "SpellClone", "SpellInvisibility", "SpellRecall", "SpellRevive", "SpellPoison", "SpellEarthquake", "SpellSpeed", "SpellSkeleton", "SpellBat", "SpellTotem", "SpellOvergrowth", "SpellIceBlock", "SpellAngry"],
  v: ["WallWrecker", "BattleBlimp", "StoneSlammer", "SiegeBarracks", "LogLauncher", "FlameFlinger", "BattleDrill", "TroopLauncher", "SkyWagon"],
};

function cardCode(id: string): string {
  const [group, name] = id.split("/");
  const number = /^(\d+)_/.exec(name || "");
  if (group === "Unit" && number) return "u" + Number(number[1]).toString(36);
  const kind = group === "Unit" ? "e" : group === "Spell" ? "s" : group === "Siege" ? "v" : "";
  const index = NAMES[kind]?.indexOf(name) ?? -1;
  if (index < 0) throw new Error("卡牌尚未分配分享编号");
  return kind + index.toString(36);
}

function checksum(text: string): string {
  let crc = 0xffff;
  for (const character of text) {
    crc ^= character.charCodeAt(0) << 8;
    for (let i = 0; i < 8; i++) crc = ((crc << 1) ^ (crc & 0x8000 ? 0x1021 : 0)) & 0xffff;
  }
  return crc.toString(36).padStart(4, "0");
}

/** Compact, versioned data. Importing never operates the phone. */
export function encodeArmy(settings: Pick<Settings, "army" | "strategy">): string {
  const strategy = STRATEGIES.indexOf(settings.strategy);
  if (strategy < 0 || !settings.army.entries.length) throw new Error("请先配置卡组和策略");
  const body = strategy + ":" + settings.army.entries.map(e => `${cardCode(e.id)}.${e.count.toString(36)}`).join(",");
  return PREFIX + body + ":" + checksum(body);
}

export function decodeArmy(code: string, cards: ArmyCard[]): { army: ArmyPlan; strategy: Strategy } {
  const text = code.trim();
  if (!text.startsWith(PREFIX) || text.length > 1000) throw new Error("分享码格式或版本不支持");
  const parts = text.slice(PREFIX.length).split(":");
  if (parts.length !== 3 || !/^[0-3]$/.test(parts[0]) || checksum(parts[0] + ":" + parts[1]) !== parts[2]) throw new Error("分享码已损坏");
  const seen = new Set<string>();
  const groups: Record<string, number> = {};
  const entries = parts[1].split(",").map(token => {
    if (!/^[uesv][0-9a-z]+\.[0-9a-z]+$/.test(token)) throw new Error("卡牌格式无效");
    const [key, amount] = token.split(".");
    const card = cards.find(c => cardCode(c.id) === key);
    const count = parseInt(amount, 36);
    if (!card || seen.has(card.id) || count < 1 || count > 400) throw new Error("分享码包含未知卡牌、重复卡牌或无效数量");
    seen.add(card.id);
    groups[card.group] = (groups[card.group] || 0) + 1;
    if (groups[card.group] > (card.group === "Siege" ? 3 : 6)) throw new Error("卡牌种类超过当前支持数量");
    return { id: card.id, count };
  });
  return { army: { entries }, strategy: STRATEGIES[Number(parts[0])] };
}
