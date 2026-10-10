import type { ArmyCard, Tactics } from "./api";

export function validateTactics(plan: Tactics, cards: ArmyCard[]): void {
  if (plan.steps.length > 32 || (plan.enabled && !plan.steps.length)) throw new Error("战术需包含 1～32 个步骤");
  let taps = 0;
  for (const s of plan.steps) {
    if (!cards.some(c => c.id === s.card) || ![s.count,s.x,s.y,s.delayMs,s.intervalMs].every(Number.isInteger)
      || s.count < 1 || s.count > 400 || s.x < 0 || s.x >= 1280 || s.y < 40 || s.y > 560
      || s.delayMs < 0 || s.delayMs > 60000 || s.intervalMs < 50 || s.intervalMs > 5000) {
      throw new Error("战术参数无效：数量 1～400，X 0～1279，Y 40～560，等待 0～60000 毫秒，间隔 50～5000 毫秒");
    }
    taps += s.count;
  }
  if (taps > 600) throw new Error("战术点击总次数不能超过 600");
}
