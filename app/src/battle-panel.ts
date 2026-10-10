import type { Frame, NodeEvent, Settings, Status } from "./api";
import { el } from "./dom";

const stages: Record<string, string> = {
  Main: "启动", Launch: "启动", AttackStart: "寻找对手", FindMatch: "寻找对手",
  AttackStartAgain: "寻找对手", FindNext: "寻找对手", BattleReady: "准备投放",
  AirOpening: "准备气球探路", AirProbeBalloon: "选择探路气球", AirProbe: "分点投放探路气球",
  SpreadZoom: "缩小镜头", SpreadZoomAgain: "缩小镜头", SpreadPan: "移动镜头",
  SpreadHeroWait: "等待英雄技能时机", SpreadHeroSkillReady: "确认战斗仍在进行",
  SpreadUnit: "分散投放兵种", SpreadSiege: "投放战车", SpreadHero: "投放英雄", SpreadSpell: "分散投放法术",
  SlideLeft: "准备投放", Deploy: "投放中", FindUnit: "投放兵种", DeployUnit: "投放兵种",
  FindSiege: "投放战车", DeploySiege: "投放战车", FindHero: "投放英雄", DeployHero: "投放英雄",
  SkillHero: "释放英雄技能", FindSpell: "投放法术", DeploySpell: "投放法术",
  WaitingEndLoop: "等待战斗结束", WaitingEnd: "战斗结束", ReturnHome: "返回村庄",
  RewardOkay: "领取奖励", OpenBox: "领取奖励",
  Surrender: "结束战斗", SurrenderConfirmation: "结束战斗", TryReturnHome: "返回村庄",
  Exception: "恢复连接", OpenBoxTap: "领取奖励", CloseDialog: "关闭弹窗",
};

function nodeName(event: NodeEvent): string {
  return event.node.replace(/!$/, "")
    .replace(/^DeploySpell\d+$/, "DeploySpell")
    .replace(/^SpreadUnit\d+$/, "SpreadUnit")
    .replace(/^SpreadSpell\d+$/, "SpreadSpell")
    .replace(/^AirProbe\d+$/, "AirProbe");
}

/** Events identify the phase; the card bar itself stays a live image, including
 * counts and disabled cards. No guessed template names or second recognition. */
export class BattlePanel {
  readonly element = el("div", { class: "card bordered battle-panel" });
  readonly canvas = el("canvas", { "aria-label": "实时卡牌栏" }) as HTMLCanvasElement;
  private readonly title = el("span", { text: "卡牌栏 · 实时画面" });
  private capturedAt: number | null = null;
  private readonly progress = el("span", { class: "battle-progress" });
  private readonly empty = el("div", { class: "battle-empty", text: "进入战斗后显示卡牌栏" });

  constructor() {
    const heading = el("div", { class: "battle-heading" });
    heading.append(this.title, this.progress);
    this.element.append(heading, this.canvas, this.empty);
  }

  clear() {
    this.capturedAt = null;
    this.canvas.hidden = true;
  }

  render(state: { frame: Frame | null; status: Status; settings: Settings; events: NodeEvent[] }, draw: () => boolean) {
    const event = [...state.events].reverse().find((event) =>
      !event.error && (event.hit || event.kind === "action") && stages[nodeName(event)]);
    const node = event ? nodeName(event) : "";
    const strategy = { "units-first": "兵种优先", "spells-first": "法术优先", "left-spread": "左侧分散投放", "air-probe": "气球探路" }[state.settings.strategy];
    this.progress.textContent = `${strategy} · ${state.status.phase === "running" ? stages[node] ?? "运行中" : "已停止"}`;
    const inBattle = ["AirOpening", "AirProbeBalloon", "AirProbe", "SpreadHeroWait", "SpreadHeroSkillReady", "SpreadZoom", "SpreadZoomAgain", "SpreadPan", "SpreadUnit", "SpreadSiege", "SpreadHero", "SpreadSpell", "BattleReady", "SlideLeft", "Deploy", "FindUnit", "DeployUnit", "FindSiege",
      "DeploySiege", "FindHero", "DeployHero", "SkillHero", "FindSpell", "DeploySpell", "WaitingEndLoop"].includes(node);
    const connected = state.status.phase === "ready" || state.status.phase === "running";
    const visible = connected && !!state.frame?.landscape && inBattle && draw();
    if (visible) {
      this.capturedAt = Date.now();
      this.title.textContent = "卡牌栏 · 实时画面";
    } else if (this.capturedAt) {
      this.title.textContent = `最近卡牌画面 · ${new Date(this.capturedAt).toLocaleTimeString("zh-CN", {hour12:false})}`;
    }
    this.canvas.hidden = !visible && !this.capturedAt;
    this.empty.hidden = visible || !!this.capturedAt;
    this.empty.textContent = state.frame && !state.frame.landscape ? "设备处于竖屏" : "进入战斗后显示卡牌栏";
  }
}
