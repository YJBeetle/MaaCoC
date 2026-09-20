import "./styles.css";
import "@material/web/button/filled-button.js";
import "@material/web/button/filled-tonal-button.js";
import "@material/web/button/text-button.js";
import "@material/web/fab/fab.js";
import "@material/web/chips/assist-chip.js";
import "@material/web/icon/icon.js";
import "@material/web/switch/switch.js";
import "@material/web/select/outlined-select.js";
import "@material/web/select/select-option.js";

import type { EngineApi, Frame, NodeEvent, Settings, Status } from "./api";
import { DEFAULT_SETTINGS, eventLabel, eventValue, formatUptime } from "./api";
import { applyTheme, prefersDark, type ThemeMode } from "./theme";

const idle: Status = { phase: "idle", detail: "", battles: 0, uptimeMs: 0, currentNode: "" };

const state = {
  page: "run" as "run" | "stats" | "settings",
  status: idle,
  settings: { ...DEFAULT_SETTINGS } as Settings,
  events: [] as NodeEvent[],
  frame: null as Frame | null,
  devices: [] as { label: string; address: string }[],
  nodes: [] as string[],
  busy: false,
  error: "",
};

async function loadApi(): Promise<EngineApi> {
  const inTauri = "__TAURI_INTERNALS__" in window;
  if (inTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const call = <T>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args);
    return {
      mode: "tauri",
      devices: () => call("devices"),
      nodes: () => call("nodes"),
      settings: () => call("settings"),
      saveSettings: (next) => call("save_settings", { next }),
      connect: () => call("connect"),
      disconnect: () => call("disconnect"),
      start: () => call("start"),
      stop: () => call("stop"),
      status: () => call("status"),
      frame: () => call("frame"),
      events: () => call("events"),
    };
  }
  if (import.meta.env.DEV) {
    const { createMockApi } = await import("./mock");
    return createMockApi();
  }
  throw new Error("非 Tauri 环境且非开发构建");
}

let api: EngineApi;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Record<string, string> = {}): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else node.setAttribute(key, value);
  }
  return node;
}

const ui = {
  title: el("h1", { text: "挂机", id: "page-title" }),
  chip: el("md-assist-chip", { label: "未连接", id: "status-chip" }),
  meta: el("span", { class: "meta", id: "run-meta", text: "" }),
  stage: el("div", { class: "stage", id: "stage" }),
  timeline: el("div", { class: "timeline", id: "timeline" }),
  connectBtn: el("md-filled-tonal-button", { id: "btn-connect" }),
  fab: el("md-fab", { id: "fab" }),
  pages: {} as Record<string, HTMLElement>,
};

function buildShell() {
  const app = el("div", { class: "shell" });
  const rail = el("nav", { class: "rail", id: "rail" });
  for (const [key, icon, label] of [
    ["run", "play_arrow", "挂机"],
    ["stats", "insights", "战果"],
    ["settings", "settings", "设置"],
  ] as const) {
    const item = el("button", { class: "rail-item", "data-page": key });
    const pill = el("span", { class: "icon" });
    pill.appendChild(el("md-icon", { text: icon }));
    item.append(pill, el("span", { text: label }));
    item.addEventListener("click", () => go(key));
    rail.appendChild(item);
  }

  const topbar = el("div", { class: "topbar" });
  topbar.append(ui.title, el("span", { class: "grow" }), ui.meta, ui.chip);

  const main = el("div", { class: "main" });
  const actions = el("div", { class: "actions", id: "actions" });
  ui.connectBtn.appendChild(el("span", { text: "连接设备" }));
  ui.fab.setAttribute("label", "开始战斗");
  const fabIcon = el("md-icon", { slot: "icon", text: "play_arrow" });
  ui.fab.appendChild(fabIcon);
  ui.connectBtn.addEventListener("click", () => void connect());
  ui.fab.addEventListener("click", () => void toggleRun());
  actions.append(ui.connectBtn, ui.fab);

  ui.pages.run = buildRunPage();
  ui.pages.stats = buildStatsPage();
  ui.pages.settings = buildSettingsPage();

  const body = el("div", { class: "page", id: "page-run" });
  const column = el("div", { class: "column" });
  column.append(ui.pages.run);
  body.appendChild(column);
  ui.pages.run.dataset.host = "1";
  main.append(topbar, body, actions);
  app.append(rail, main);
  document.body.appendChild(app);
  go("run");
}

function buildRunPage(): HTMLElement {
  const wrap = el("div", { class: "column" });
  const card = el("div", { class: "card bordered" });
  card.appendChild(ui.stage);
  const tlCard = el("div", { class: "card bordered" });
  tlCard.append(el("div", { class: "card-title", text: "节点" }), ui.timeline);
  wrap.append(card, tlCard);
  return wrap;
}

function buildStatsPage(): HTMLElement {
  const wrap = el("div", { class: "column" });
  const stats = el("div", { class: "stats", id: "stats" });
  for (const [key, label] of [["battles", "局数"], ["hits", "命中"], ["misses", "未命中"]] as const) {
    const cell = el("div", { class: "stat" });
    cell.append(el("div", { class: "n", id: `stat-${key}`, text: "0" }), el("div", { class: "l", text: label }));
    stats.appendChild(cell);
  }
  const card = el("div", { class: "card bordered" });
  card.append(el("div", { class: "card-title", text: "未命中最多的节点" }), el("div", { class: "card-body", id: "miss-rank" }));
  wrap.append(stats, card);
  return wrap;
}

function buildSettingsPage(): HTMLElement {
  const wrap = el("div", { class: "column" });
  const device = el("div", { class: "card bordered", id: "card-device" });
  device.append(el("div", { class: "sect", text: "设备" }), el("div", { class: "field", id: "f-device" }), el("div", { class: "field", id: "f-entry" }), el("div", { class: "field", id: "f-interval" }));

  const diag = el("div", { class: "card bordered" });
  diag.append(el("div", { class: "sect", text: "诊断" }), el("div", { class: "field", id: "f-misses" }), el("div", { class: "field", id: "f-overlay" }), el("div", { class: "field", id: "f-record" }));

  const look = el("div", { class: "card bordered" });
  look.append(el("div", { class: "sect", text: "外观" }), el("div", { class: "field", id: "f-theme" }));

  const about = el("div", { class: "card bordered" });
  about.append(el("div", { class: "sect", text: "关于" }), el("div", { class: "field", id: "f-version" }), el("div", { class: "field danger", id: "f-reset" }));

  wrap.append(device, diag, look, about);
  return wrap;
}

function makeSwitch(id: string, title: string, hint: string | undefined, value: boolean, onChange: (v: boolean) => void) {
  const sw = el("md-switch");
  sw.selected = value;
  sw.addEventListener("change", () => onChange(sw.selected));
  const field = document.getElementById(id);
  if (field) {
    const left = el("div");
    left.append(el("div", { class: "k", text: title }), ...(hint ? [el("div", { class: "hint", text: hint })] : []));
    field.replaceChildren(left, sw);
  }
  return sw;
}

function makeSelect(id: string, title: string, hint: string | undefined, options: string[], value: string, onChange: (v: string) => void) {
  const select = el("md-outlined-select");
  select.setAttribute("label", title);
  for (const option of options) {
    const item = el("md-select-option");
    item.value = option;
    item.setAttribute("headline", option);
    const slot = el("div", { slot: "headline", text: option });
    item.appendChild(slot);
    select.appendChild(item);
  }
  select.value = value;
  select.addEventListener("change", () => onChange(select.value));
  const field = document.getElementById(id);
  if (field) {
    const left = el("div");
    left.append(el("div", { class: "k", text: title }), ...(hint ? [el("div", { class: "hint", text: hint })] : []));
    field.replaceChildren(left, select);
  }
  return select;
}

async function persist(next: Partial<Settings>) {
  state.settings = { ...state.settings, ...next };
  state.settings = await api.saveSettings(state.settings);
  render();
}

async function connect() {
  state.busy = true;
  state.error = "";
  render();
  try {
    state.status = await api.connect();
    state.devices = await api.devices();
    state.nodes = await api.nodes();
  } catch (err) {
    state.error = String(err);
  } finally {
    state.busy = false;
    render();
  }
}

async function toggleRun() {
  state.error = "";
  try {
    state.status = state.status.phase === "running" ? await api.stop() : await api.start();
  } catch (err) {
    state.error = String(err);
  }
  render();
}

function go(page: string) {
  state.page = page as typeof state.page;
  for (const item of document.querySelectorAll<HTMLElement>(".rail-item")) {
    item.setAttribute("aria-current", String(item.dataset.page === page));
  }
  const titles: Record<string, string> = { run: "挂机", stats: "战果", settings: "设置" };
  ui.title.textContent = titles[page];
  const host = document.querySelector(".page > .column");
  if (host) host.replaceChildren(ui.pages[page]);
  const actions = document.getElementById("actions");
  if (actions) actions.hidden = page !== "run";
  render();
}

function renderStage() {
  if (!state.frame) {
    ui.stage.replaceChildren(el("div", { class: "void", text: state.status.phase === "connecting" ? "连接中…" : "未连接设备" }));
    return;
  }
  const img = el("img", { src: state.frame.dataUrl, alt: "设备画面" });
  img.id = "frame-img";
  const badge = el("span", { class: "res", text: `${state.frame.width}×${state.frame.height}` });
  const children: HTMLElement[] = [img, badge];
  if (state.settings.overlayHits) {
    const canvas = el("canvas", { id: "overlay" }) as HTMLCanvasElement;
    children.push(canvas);
  }
  ui.stage.replaceChildren(...children);
  if (state.settings.overlayHits) drawOverlay();
}

function drawOverlay() {
  const canvas = document.getElementById("overlay") as HTMLCanvasElement | null;
  const frame = state.frame;
  if (!canvas || !frame) return;
  canvas.width = frame.width;
  canvas.height = frame.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  const style = getComputedStyle(document.documentElement);
  ctx.strokeStyle = style.getPropertyValue("--md-sys-color-primary").trim() || "#f5a623";
  ctx.lineWidth = 3;
  ctx.font = "20px monospace";
  ctx.fillStyle = ctx.strokeStyle;
  for (const event of state.events.slice(-12)) {
    if (!event.hit || !event.boxRect) continue;
    const [x, y, w, h] = event.boxRect;
    ctx.strokeRect(x, y, w, h);
    ctx.fillText(`${event.node} ${event.score.toFixed(2)}`, x, Math.max(18, y - 6));
  }
}

function renderTimeline() {
  const visible = state.settings.showMisses ? state.events : state.events.filter((e) => e.hit || e.kind === "action");
  if (!visible.length) {
    ui.timeline.replaceChildren(el("div", { class: "tl-empty", text: "尚无事件" }));
    return;
  }
  const stuck = ui.timeline.scrollHeight - ui.timeline.scrollTop - ui.timeline.clientHeight < 40;
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
  ui.timeline.replaceChildren(...rows);
  if (stuck) ui.timeline.scrollTop = ui.timeline.scrollHeight;
}

function renderStats() {
  const hits = state.events.filter((e) => e.kind === "recognition" && e.hit).length;
  const misses = state.events.filter((e) => e.kind === "recognition" && !e.hit).length;
  const set = (id: string, value: string) => {
    const node = document.getElementById(id);
    if (node) node.textContent = value;
  };
  set("stat-battles", String(state.status.battles));
  set("stat-hits", String(hits));
  set("stat-misses", String(misses));
  const rank = document.getElementById("miss-rank");
  if (!rank) return;
  const counts = new Map<string, number>();
  for (const event of state.events) {
    if (event.kind === "recognition" && !event.hit) counts.set(event.node, (counts.get(event.node) ?? 0) + 1);
  }
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8);
  rank.replaceChildren(
    ...(top.length
      ? top.map(([node, count]) => {
          const row = el("div", { class: "field" });
          row.append(el("div", { class: "k", text: node }), el("code", { text: String(count) }));
          return row;
        })
      : [el("div", { class: "hint", text: "暂无未命中记录" })]),
  );
}

function renderSettings() {
  const theme = state.status.phase;
  makeSelect(
    "f-device",
    "设备",
    "仅一个设备时自动选中",
    state.devices.length ? state.devices.map((d) => d.label) : ["未检测到设备"],
    state.settings.preferredDevice ?? state.devices[0]?.label ?? "未检测到设备",
    (value) => void persist({ preferredDevice: value }),
  );
  makeSelect(
    "f-entry",
    "任务入口",
    undefined,
    state.nodes.length ? state.nodes : ["Main"],
    state.settings.entry,
    (value) => void persist({ entry: value }),
  );
  makeSelect(
    "f-interval",
    "画面刷新",
    undefined,
    ["500", "1000", "2000", "0"],
    String(state.settings.frameIntervalMs),
    (value) => void persist({ frameIntervalMs: Number(value) }),
  );
  makeSwitch("f-misses", "显示未命中节点", undefined, state.settings.showMisses, (v) => void persist({ showMisses: v }));
  makeSwitch("f-overlay", "画面叠加命中框", undefined, state.settings.overlayHits, (v) => void persist({ overlayHits: v }));
  makeSwitch("f-record", "记录节点画面", undefined, state.settings.recordFrames, (v) => void persist({ recordFrames: v }));
  makeSelect("f-theme", "外观", "默认跟随系统", ["system", "light", "dark"], state.settings.themeMode, (value) => {
    applyTheme(value as ThemeMode);
    void persist({ themeMode: value as ThemeMode });
  });

  const version = document.getElementById("f-version");
  version?.replaceChildren(
    el("div", { class: "k", text: "版本" }),
    el("code", { text: `0.1.0 · ${api.mode}${theme === "error" ? ` · ${state.error}` : ""}` }),
  );
  const reset = document.getElementById("f-reset");
  if (reset) {
    const left = el("div", { class: "k", text: "恢复默认设置" });
    const btn = el("md-text-button");
    btn.appendChild(el("span", { text: "重置" }));
    btn.addEventListener("click", () => {
      void persist({ ...DEFAULT_SETTINGS });
      applyTheme(state.settings.themeMode);
    });
    reset.replaceChildren(left, btn);
  }
}

function render() {
  const phase = state.status.phase;
  const chipLabels: Record<string, string> = { idle: "未连接", connecting: "连接中", ready: "已连接", running: "战斗中", error: "出错" };
  ui.chip.setAttribute("label", state.error ? `${chipLabels[phase]} · ${state.error}` : chipLabels[phase]);
  ui.chip.toggleAttribute("data-running", phase === "running");
  ui.meta.textContent = phase === "running" ? `${formatUptime(state.status.uptimeMs)} · 第 ${state.status.battles + 1} 局` : state.status.detail;

  const connected = phase === "ready" || phase === "running";
  ui.connectBtn.toggleAttribute("disabled", phase === "connecting" || phase === "running");
  ui.fab.toggleAttribute("disabled", !connected);
  ui.fab.setAttribute("label", phase === "running" ? "停止" : "开始战斗");
  const icon = ui.fab.querySelector("md-icon");
  if (icon) icon.textContent = phase === "running" ? "stop_circle" : "play_arrow";

  if (state.page === "run") {
    renderStage();
    renderTimeline();
  } else {
    renderStats();
    renderSettings();
  }
}

let lastFrameAt = 0;
async function tick() {
  try {
    state.status = await api.status();
    const incoming = await api.events();
    if (incoming.length) {
      state.events.push(...incoming);
      if (state.events.length > 400) state.events.splice(0, state.events.length - 400);
    }
    const interval = state.settings.frameIntervalMs;
    if (interval > 0 && Date.now() - lastFrameAt > interval) {
      lastFrameAt = Date.now();
      const frame = await api.frame();
      if (frame) state.frame = frame;
      else state.frame = null;
    }
  } catch (err) {
    state.error = String(err);
  }
  render();
}

async function boot() {
  api = await loadApi();
  state.settings = await api.settings();
  applyTheme(state.settings.themeMode);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (state.settings.themeMode === "system") applyTheme("system");
  });
  buildShell();
  state.devices = await api.devices();
  state.nodes = await api.nodes();
  render();
  window.setInterval(() => void tick(), 250);
}

/** Dev-only seam: the in-app browser can freeze timers, so tests call tick()
    directly instead of waiting for the interval. */
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__maacoc = {
    tick: () => tick(),
    state,
    go,
  };
}

void boot();
export { prefersDark };
