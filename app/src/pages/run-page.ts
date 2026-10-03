import type { Frame, NodeEvent, Settings, Status } from "../api";
import { eventLabel, eventValue } from "../api";
import { el } from "../dom";
import { DevicePreview } from "../device-preview";

interface RunPageState {
  frame: Frame | null;
  status: Status;
  settings: Settings;
  events: NodeEvent[];
  sizeBusy: boolean;
}

export class RunPage {
  private readonly preview = new DevicePreview();
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
    this.fab.setAttribute("label", "开始战斗");
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
  }

  dispose() {
    this.preview.dispose();
  }

  /** Keep action controls in place as connection and run status change. */
  render(state: RunPageState) {
    const phase = state.status.phase;
    const connected = phase === "ready" || phase === "running";
    const running = phase === "running";

    if (connected) this.fab.removeAttribute("aria-disabled");
    else this.fab.setAttribute("aria-disabled", "true");
    this.fab.setAttribute("label", running ? "停止" : "开始战斗");
    const icon = this.fab.querySelector("md-icon");
    if (icon) icon.textContent = running ? "stop" : "play_arrow";

    for (const button of [this.sizeSet, this.sizeReset]) button.toggleAttribute("disabled", !connected || state.sizeBusy);

    this.preview.render(state);
    this.renderTimeline(state);
  }

  private build(): HTMLElement {
    const grid = el("div", { class: "run-grid" });
    const left = el("div", { class: "run-left" });
    const card = el("div", { class: "card bordered grow" });
    card.appendChild(this.preview.element);
    left.append(card, this.actions);

    const tlCard = el("div", { class: "card bordered timeline-card" });
    tlCard.append(el("div", { class: "card-title", text: "节点" }), this.timeline);

    grid.append(left, tlCard);
    return grid;
  }

  private renderTimeline(state: RunPageState) {
    const visible = state.settings.showMisses ? state.events : state.events.filter((e) => e.hit || e.kind === "action");
    if (!visible.length) {
      this.timeline.replaceChildren(el("div", { class: "tl-empty", text: "尚无事件" }));
      return;
    }
    const stuck = this.timeline.scrollHeight - this.timeline.scrollTop - this.timeline.clientHeight < 40;
    const rows = visible.slice(-200).map((event) => {
      const row = el("div", { class: "tl-row" });
      const dot = el("span", { class: `dot ${event.kind === "action" ? "act" : event.hit ? "" : "miss"}` });
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
