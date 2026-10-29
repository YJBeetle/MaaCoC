import type { Frame, NodeEvent, Settings, Status } from "../api";
import { eventLabel, eventValue, formatUptime } from "../api";
import { el } from "../dom";
import { DevicePreview } from "../device-preview";
import { BattlePanel, runStage, strategyLabel } from "../battle-panel";
import "./run-page.css";

interface RunPageState {
  frame: Frame | null;
  status: Status;
  settings: Settings;
  events: NodeEvent[];
  sizeBusy: boolean;
  runAction: "starting" | "stopping" | null;
}

export class RunPage {
  private readonly battle = new BattlePanel();
  private state: RunPageState | null = null;
  private readonly preview = new DevicePreview(() => this.renderBattle());
  private readonly resources = el("div", { class: "resource-readings" });
  private readonly timeline = el("div", { class: "timeline", id: "timeline" });
  private readonly fab = el("md-filled-button", { id: "fab" });
  private readonly actionLabel = el("span", { text: "开始刷资源" });
  private readonly overview = el("div", { class: "run-overview" });
  private readonly stageLabel = el("strong");
  private readonly duration = el("span", { class: "run-duration" });
  private readonly strategy = el("span", { class: "run-strategy" });
  private readonly policy = el("span", { class: "run-policy" });
  private readonly logs = el("details", { class: "card timeline-card" });
  private readonly logCount = el("span", { class: "log-count" });
  private lastEvent: NodeEvent | undefined;
  private lastShowMisses = false;
  private resourceKey = "";
  private readonly secondary = el("div", { class: "secondary" });
  private readonly sizeSet = el("md-text-button", {
    id: "btn-size-set",
    title: "把设备锁成 1080×1920，横屏画面才正好缩到 1280×720 匹配空间",
  });
  private readonly sizeReset = el("md-text-button", { id: "btn-size-reset", title: "还回设备自己的分辨率" });
  private readonly actions = el("div", { class: "actions", id: "actions" });
  readonly element: HTMLElement;

  constructor(actions: { toggleRun: () => Promise<void>; changeScreenSize: (reset: boolean) => Promise<void> }) {
    this.fab.append(el("md-icon", { slot: "icon", text: "play_arrow" }), this.actionLabel);
    this.fab.addEventListener("click", () => void actions.toggleRun());
    this.sizeSet.appendChild(el("span", { text: "设为 1080×1920" }));
    this.sizeReset.appendChild(el("span", { text: "恢复默认" }));
    this.sizeSet.addEventListener("click", () => void actions.changeScreenSize(false));
    this.sizeReset.addEventListener("click", () => void actions.changeScreenSize(true));
    const deviceSetup = el("details", { class: "device-setup" });
    deviceSetup.append(el("summary", { text: "画面设置" }), this.secondary);
    this.secondary.append(this.sizeSet, this.sizeReset);
    this.actions.append(this.fab, deviceSetup);
    const overviewTop = el("div", { class: "run-overview-heading" });
    overviewTop.append(el("span", { text: "运行状态" }), this.duration);
    this.overview.append(overviewTop, this.stageLabel, this.strategy, this.policy);
    this.logs.addEventListener("toggle", () => { this.lastEvent = undefined; if (this.state) this.renderTimeline(this.state); });

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
    const pending = state.runAction !== null;
    const pendingLabel = state.runAction === "starting" ? "正在开始…" : "正在停止…";

    this.fab.toggleAttribute("disabled", !connected || pending || state.sizeBusy);
    this.fab.setAttribute("aria-busy", String(pending));
    this.actionLabel.textContent = pending ? pendingLabel : running ? "停止刷资源" : "开始刷资源";
    const icon = this.fab.querySelector("md-icon");
    if (icon) icon.textContent = pending ? "hourglass_top" : running ? "stop" : "play_arrow";

    for (const button of [this.sizeSet, this.sizeReset]) button.toggleAttribute("disabled", !connected || running || pending || state.sizeBusy);
    this.stageLabel.textContent = pending ? pendingLabel : runStage(state.status, state.events);
    this.duration.textContent = formatUptime(state.status.uptimeMs);
    this.strategy.textContent = strategyLabel(state.settings);
    this.policy.textContent = `${state.settings.stopWhenFull ? "三项全满后停止" : "满仓停止未开启"} · ${state.settings.lootFilter.enabled ? "收益筛选已开启" : "不筛选收益"}`;
    this.overview.classList.toggle("is-running", running);

    this.preview.render(state);
    this.renderBattle();
    this.renderTimeline(state);
    this.renderResources(state);
  }

  private build(): HTMLElement {
    const grid = el("div", { class: "run-grid" });
    const left = el("div", { class: "run-left" });
    const card = el("div", { class: "card bordered grow preview-card" });
    card.appendChild(this.preview.element);
    left.append(card, this.actions);

    const logHeading = el("summary");
    logHeading.append(el("span", { text: "详细日志" }), this.logCount, el("md-icon", { text: "expand_more" }));
    this.logs.append(logHeading, this.timeline);

    const right = el("div", { class: "run-right" });
    const summary = el("div", { class: "card bordered status-card" });
    summary.append(this.overview, el("div", { class: "card-title", text: "资源 · 最近读取" }), this.resources, this.battle.element);
    right.append(summary, this.logs);
    grid.append(left, right);
    return grid;
  }

  private renderResources(state: RunPageState) {
    const key = JSON.stringify(state.status.resources);
    if (key === this.resourceKey) return;
    this.resourceKey = key;
    const format = new Intl.NumberFormat("zh-CN");
    const rows = ["金币", "圣水", "黑油"].map((name, index) => {
      const reading = state.status.resources?.[index];
      const row = el("div", { class: "resource-row" });
      const hasValue = reading?.current != null;
      const nameNode = el("span", { class: "resource-name", text: name });
      row.append(nameNode, el("strong", { text: hasValue ? format.format(reading!.current!) : "未读取" }));
      const capacity = reading?.capacity;
      const percent = hasValue && capacity != null && capacity > 0 ? Math.min(100, reading!.current! / capacity * 100) : null;
      if (percent !== null) {
        const meter = el("div", { class: "resource-meter", role: "progressbar", "aria-label": `${name}最近读数占容量比例`, "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(percent) });
        const fill = el("span"); fill.style.width = `${percent}%`;
        meter.append(fill); row.append(meter);
      }
      const meta = el("div", { class: "resource-meta" });
      meta.append(el("span", { text: capacity == null ? "容量未读取" : `容量 ${format.format(capacity)}` }),
        el("span", { text: percent === null ? "—" : `${Math.floor(percent * 10) / 10}%` }));
      const time = reading?.readAt == null ? "尚无成功读数" : `读取于 ${new Date(reading.readAt * 1000).toLocaleTimeString("zh-CN", { hour12: false })}`;
      const failures = [];
      if (reading?.currentFailed) failures.push(hasValue ? "数量读取失败，保留旧值" : "数量读取失败");
      if (reading?.capacityFailed) failures.push(capacity == null ? "容量读取失败" : "容量读取失败，保留旧值");
      row.append(meta, el("span", { class: "resource-time", text: reading ? time : "等待仓库检查" }));
      if (failures.length) row.append(el("span", { class: "resource-warning", text: failures.join("；") }));
      row.classList.toggle("read-failed", !!(reading?.currentFailed || reading?.capacityFailed));
      return row;
    });
    this.resources.replaceChildren(...rows);
  }

  private renderBattle() {
    if (this.state) this.battle.render(this.state, () => this.preview.drawCardBar(this.battle.canvas));
  }

  private renderTimeline(state: RunPageState) {
    const visible = state.settings.showMisses ? state.events : state.events.filter((e) => e.error || e.hit || e.kind === "action");
    this.logCount.textContent = visible.length ? `最近 ${Math.min(200, visible.length)} 条` : "暂无事件";
    if (!this.logs.open) return;
    const last = state.events.at(-1);
    if (last && last === this.lastEvent && this.lastShowMisses === state.settings.showMisses) return;
    this.lastEvent = last; this.lastShowMisses = state.settings.showMisses;
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
