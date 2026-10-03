import type { Frame, NodeEvent, Phase } from "./api";
import { el } from "./dom";
import { PreviewImage } from "./preview-image";

interface PreviewState {
  frame: Frame | null;
  status: { phase: Phase; panel: string };
  settings: { overlayHits: boolean };
  events: NodeEvent[];
}

/** Owns the device picture, labels and recognition overlay in one coordinate space. */
export class DevicePreview {
  readonly element = el("div", { class: "stage", id: "stage" });
  private ui = {
    frameImg: el("img", { id: "frame-img", alt: "设备画面" }),
    badge: el("span", { class: "res", text: "" }),
    panel: el("span", { class: "panel", id: "panel-size", text: "" }),
    overlay: el("canvas", { id: "overlay" }) as HTMLCanvasElement,
    void: el("div", { class: "void", text: "未连接设备" }),
    portraitNote: el("div", { class: "portrait-note", text: "设备处于竖屏，游戏不在前台" }),
  };
  private state: PreviewState = { frame: null, status: { phase: "idle", panel: "" }, settings: { overlayHits: false }, events: [] };
  private image = new PreviewImage(this.ui.frameImg, () => this.drawOverlay());
  private observer = new ResizeObserver(() => this.drawOverlay());

  constructor() {
    this.observer.observe(this.element);
  }

  clear() {
    this.image.clear();
    this.state = { ...this.state, frame: null };
  }

  dispose() {
    this.observer.disconnect();
    this.clear();
  }

  /** The stage keeps one long-lived <img>: recreating it per render left the dark
      background visible until the new frame decoded, so the picture blinked black. */
  private showStageChildren(children: HTMLElement[]) {
    const current = Array.from(this.element.children);
    if (current.length === children.length && current.every((node, i) => node === children[i])) return;
    this.element.replaceChildren(...children);
  }

  render(next: PreviewState) {
    this.state = next;
    const frame = this.state.frame;
    if (!frame) {
      this.ui.void.textContent = this.state.status.phase === "connecting" ? "连接中…" : "未连接设备";
      this.showStageChildren([this.ui.void]);
      return;
    }
    this.image.update(frame.dataUrl);
    this.ui.badge.textContent = `${frame.width}×${frame.height}`;
    this.ui.badge.classList.toggle("warn", !frame.landscape);
    this.ui.panel.textContent = this.state.status.panel;
    const children: HTMLElement[] = [this.ui.frameImg, this.ui.badge];
    if (this.state.status.panel) children.push(this.ui.panel);
    if (!frame.landscape) children.push(this.ui.portraitNote);
    // In portrait the events on screen came from a landscape frame, so their
    // boxes land anywhere but where the button actually is.
    const overlay = this.state.settings.overlayHits && frame.landscape;
    if (overlay) children.push(this.ui.overlay);
    this.showStageChildren(children);
    if (overlay) this.drawOverlay();
  }

  private drawOverlay() {
    const canvas = this.ui.overlay;
    const frame = this.state.frame;
    if (!canvas.isConnected || !frame || !frame.landscape || !this.state.settings.overlayHits) return;

    // The frame is letterboxed inside the stage, so the overlay has to cover the
    // letterboxed rect rather than the stage — otherwise every box lands in the
    // wrong place. Stroke widths are divided back out so lines stay 2-3 screen px.
    const stage = this.element.getBoundingClientRect();
    const scale = Math.min(stage.width / frame.width, stage.height / frame.height) || 1;
    const shown = { width: frame.width * scale, height: frame.height * scale };
    canvas.style.inset = "auto";
    canvas.style.left = `${(stage.width - shown.width) / 2}px`;
    canvas.style.top = `${(stage.height - shown.height) / 2}px`;
    canvas.style.width = `${shown.width}px`;
    canvas.style.height = `${shown.height}px`;
    canvas.width = frame.width;
    canvas.height = frame.height;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const px = (size: number) => size / scale;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const style = getComputedStyle(document.documentElement);
    ctx.strokeStyle = style.getPropertyValue("--md-sys-color-primary").trim() || "#f5a623";
    ctx.lineWidth = px(2);
    ctx.font = `${px(12)}px monospace`;
    ctx.fillStyle = ctx.strokeStyle;
    for (const event of this.state.events.slice(-12)) {
      if (!event.hit || !event.boxRect) continue;
      const [x, y, w, h] = event.boxRect;
      ctx.strokeRect(x, y, w, h);
      ctx.fillText(`${event.node} ${event.score.toFixed(2)}`, x, Math.max(px(12), y - px(4)));
    }
  }

}
