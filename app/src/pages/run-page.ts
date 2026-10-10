import type { Frame, NodeEvent, Settings, Status } from "../api";
import { eventLabel, eventValue } from "../api";
import { el } from "../dom";
import { DevicePreview } from "../device-preview";
import { BattlePanel } from "../battle-panel";

interface RunPageState {
  frame: Frame | null;
  status: Status;
  settings: Settings;
  events: NodeEvent[];
  sizeBusy: boolean;
}

export class RunPage {
  private readonly battle = new BattlePanel();
  private state: RunPageState | null = null;
  private readonly preview = new DevicePreview(() => this.renderBattle());
  private readonly resources = el("div", { class: "resource-readings" });
  private readonly timeline = el("div", { class: "timeline", id: "timeline" });
  private readonly fab = el("md-fab", { id: "fab" });
  private readonly secondary = el("div", { class: "secondary" });
  private readonly sizeSet = el("md-text-button", {
    id: "btn-size-set",
    title: "把设备锁成 1080×1920，横屏画面才正好缩到 1280×720 匹配空间",
  });
  private readonly sizeReset = el("md-text-button", { id: "btn-size-reset", title: "还回设备自己的分辨率" });
  private readonly actions = el("div", { class: "actions", id: "actions" });
  readonly element: HTMLElement;

  constructor(actions: { toggleRun: () => Promise<void>; changeScreenSize: (reset: boolean) => Promise<void> }) {
    this.fab.setAttribute("label", "开始刷资源");
    this.fab.setAttribute("variant", "primary");
    this.fab.appendChild(el("md-icon", { slot: "icon", text: "play_arrow" }));
    this.fab.addEventListener("click", () => void actions.toggleRun());
    this.sizeSet.appendChild(el("span", { text: "设为 1080×1920" }));
    this.sizeReset.appendChild(el("span", { text: "恢复默认" }));
    this.sizeSet.addEventListener("click", () => void actions.changeScreenSize(false));
    this.sizeReset.addEventListener("click", () => void actions.changeScreenSize(true));
    // The FAB is the one primary action; the resolution pair is device setup and
    // belongs on a quieter second row, not competing for the same line.
    this.secondary.append(this.sizeSet, this.sizeReset);
    this.actions.append(this.fab, this.secondary);

    this.element = this.build();
  }

  clearPreview() {
    this.preview.clear();
    this.battle.clear();
  }

  dispose() {
    this.preview.dispose();
  }

  /** Keep action controls in place as connection and run status change. */
  render(state: RunPageState) {
    this.state = state;
    const phase = state.status.phase;
    const connected = phase === "ready" || phase === "running";
    const running = phase === "running";

    if (connected) this.fab.removeAttribute("aria-disabled");
    else this.fab.setAttribute("aria-disabled", "true");
    this.fab.setAttribute("label", running ? "停止" : "开始刷资源");
    const icon = this.fab.querySelector("md-icon");
    if (icon) icon.textContent = running ? "stop" : "play_arrow";

    for (const button of [this.sizeSet, this.sizeReset]) button.toggleAttribute("disabled", !connected || state.sizeBusy);

    this.preview.render(state);
    this.renderBattle();
    this.renderTimeline(state);
    this.renderResources(state);
  }

  private build(): HTMLElement {
    const grid = el("div", { class: "run-grid" });
    const left = el("div", { class: "run-left" });
    const card = el("div", { class: "card bordered grow" });
    card.appendChild(this.preview.element);
    left.append(card, this.actions);

    const tlCard = el("div", { class: "card bordered timeline-card" });
    tlCard.append(el("div", { class: "card-title", text: "运行日志" }), this.timeline);

    const right = el("div", { class: "run-right" });
    const summary = el("div", { class: "card bordered status-card" });
    summary.append(el("div", { class: "card-title", text: "状态 · 最近读取" }), this.resources, this.battle.element);
    right.append(summary, tlCard);
    grid.append(left, right);
    return grid;
  }

  private renderResources(state: RunPageState) {
    const format = new Intl.NumberFormat("zh-CN");
    const rows = ["金币", "圣水", "黑油"].map((name, index) => {
      const reading = state.status.resources?.[index];
      const row = el("div", { class: "resource-row" });
      const hasValue = reading?.current != null;
      row.append(el("span", { text: name }), el("strong", { text: hasValue ? format.format(reading!.current!) : "未读取" }));
      const meta: string[] = [];
      if (reading) {
        if (reading.currentFailed) meta.push(hasValue ? "本次数量读取失败 · 保留旧读数" : "本次数量读取失败");
        if (reading.capacityFailed) meta.push(reading.capacity === null ? "本次容量读取失败" : "本次容量读取失败 · 保留已知容量");
        meta.push(reading.capacity === null ? "容量未读取" : `容量 ${format.format(reading.capacity)}`);
        if (reading.readAt !== null) meta.push(`最近成功读取 ${new Date(reading.readAt * 1000).toLocaleTimeString("zh-CN", {hour12:false})}`);
        else meta.push("尚无成功数量读数");
      } else meta.push("等待仓库检查");
      row.append(el("span", { class: "resource-meta", text: meta.join(" · ") }));
      row.classList.toggle("read-failed", !!(reading?.currentFailed || reading?.capacityFailed));
      return row;
    });
    this.resources.replaceChildren(...rows);
  }

  private renderBattle() {
    if (this.state) this.battle.render(this.state, () => this.preview.drawCardBar(this.battle.canvas));
  }

  private renderTimeline(state: RunPageState) {
    const visible = state.settings.showMisses ? state.events : state.events.filter((e) => e.hit || e.kind === "action");
    if (!visible.length) {
      this.timeline.replaceChildren(el("div", { class: "tl-empty", text: "尚无事件" }));
      return;
    }
    const stuck = this.timeline.scrollHeight - this.timeline.scrollTop - this.timeline.clientHeight < 40;
    const rows = visible.slice(-200).map((event) => {
      const row = el("div", { class: `tl-row${event.error ? " failed" : ""}` });
      const dot = el("span", { class: `dot ${event.error ? "failed" : event.kind === "action" ? "act" : event.hit ? "" : "miss"}` });
      row.append(
        el("span", { class: "t", text: event.at.toFixed(1) }),
        dot,
        el("span", { text: eventLabel(event) }),
        el("span", { class: "v", text: eventValue(event) }),
      );
      return row;
    });
    this.timeline.replaceChildren(...rows);
    if (stuck) this.timeline.scrollTop = this.timeline.scrollHeight;
  }
}
