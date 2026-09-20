/* Bundled, not a CDN link: the shipped .app has to render offline, and a missing
   icon font shows the literal glyph names ("play_arrow") instead of icons. */
import "@material-symbols/font-400/outlined.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "./styles.css";
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
import { applyTheme, type ThemeMode } from "./theme";

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
  ui.fab.setAttribute("variant", "primary");
  const fabIcon = el("md-icon", { slot: "icon", text: "play_arrow" });
  ui.fab.appendChild(fabIcon);
  ui.connectBtn.addEventListener("click", () => void connect());
  ui.fab.addEventListener("click", () => void toggleRun());
  actions.append(ui.connectBtn, ui.fab);

  ui.pages.run = buildRunPage();
  ui.pages.stats = buildStatsPage();
  ui.pages.settings = buildSettingsPage();

  const body = el("div", { class: "page", id: "page-run" });
  body.appendChild(ui.pages.run);
  main.append(topbar, body, actions);
  app.append(rail, main);
  document.body.appendChild(app);
  go(state.page);
}

function buildRunPage(): HTMLElement {
  const wrap = el("div", { class: "column fill" });
  const card = el("div", { class: "card bordered grow" });
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
  device.append(
    el("div", { class: "sect", text: "设备" }),
    el("div", { class: "field", id: "f-device" }),
    el("div", { class: "field", id: "f-entry" }),
    el("div", { class: "field", id: "f-interval" }),
    el("div", { class: "field", id: "f-autostart" }),
  );

  const diag = el("div", { class: "card bordered" });
  diag.append(
    el("div", { class: "sect", text: "诊断" }),
    el("div", { class: "field", id: "f-misses" }),
    el("div", { class: "field", id: "f-overlay" }),
    el("div", { class: "field", id: "f-record" }),
  );

  const look = el("div", { class: "card bordered" });
  look.append(el("div", { class: "sect", text: "外观" }), el("div", { class: "field", id: "f-theme" }));

  const about = el("div", { class: "card bordered" });
  about.append(
    el("div", { class: "sect", text: "关于" }),
    el("div", { class: "field", id: "f-version" }),
    el("div", { class: "field danger", id: "f-reset" }),
  );

  wrap.append(device, diag, look, about);
  return wrap;
}

/** Controls are created the first time they are needed and only updated
    afterwards. Rebuilding them on every 250 ms poll made the outlined selects
    visibly flicker. */
function fieldOf(id: string, title: string, hint?: string): HTMLElement | null {
  const field = document.getElementById(id);
  if (!field) return null;
  if (!field.firstElementChild) {
    const left = el("div");
    left.append(el("div", { class: "k", text: title }), ...(hint ? [el("div", { class: "hint", text: hint })] : []));
    field.appendChild(left);
  }
  return field;
}

interface Option {
  value: string;
  label: string;
}

/** The MWC custom elements we touch, narrowed to the properties used here. */
interface Select extends HTMLElement {
  value: string;
  disabled: boolean;
}
interface Switch extends HTMLElement {
  selected: boolean;
}

const asOptions = (values: string[]): Option[] => values.map((value) => ({ value, label: value }));

/** The field's own heading is the visible label, so the select only carries an
    accessible one — a second copy inside the notch read as a duplicate.
    MWC derives the displayed text when the field first renders and does not
    refresh it for a later programmatic selection, so a select whose option list
    changed is rebuilt off-DOM with the choice already applied. MWC has no
    placeholder, so an unavailable choice shows as a disabled stand-in row. */
function syncSelect(
  id: string,
  title: string,
  hint: string | undefined,
  options: Option[],
  value: string,
  onChange: (v: string) => void,
  emptyLabel: string,
) {
  const field = fieldOf(id, title, hint);
  if (!field) return null;
  const items = options.length ? options : [{ value: "-", label: emptyLabel }];
  const wanted = options.length ? value : "-";
  const signature = items.map((o) => o.value).join("\n");
  let select = field.querySelector<Select>("md-outlined-select");
  if (select?.dataset.items !== signature) {
    const fresh = el("md-outlined-select") as Select;
    fresh.setAttribute("aria-label", title);
    fresh.style.minWidth = "260px";
    fresh.disabled = !options.length;
    fresh.dataset.items = signature;
    fresh.addEventListener("change", () => onChange(fresh.value));
    for (const option of items) {
      const item = el("md-select-option");
      item.value = option.value;
      item.appendChild(el("div", { slot: "headline", text: option.label }));
      fresh.appendChild(item);
    }
    fresh.value = wanted;
    if (select) select.replaceWith(fresh);
    else field.appendChild(fresh);
    select = fresh;
  }
  return select;
}

function syncSwitch(id: string, title: string, hint: string | undefined, value: boolean, onChange: (v: boolean) => void) {
  const field = fieldOf(id, title, hint);
  if (!field) return null;
  let sw = field.querySelector<Switch>("md-switch");
  if (!sw) {
    sw = el("md-switch") as Switch;
    sw.setAttribute("aria-label", title);
    sw.addEventListener("change", () => onChange(sw!.selected));
    field.appendChild(sw);
  }
  if (sw.selected !== value) sw.selected = value;
  return sw;
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
  const host = document.querySelector(".page");
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

  // The frame is letterboxed inside the stage, so the overlay has to cover the
  // letterboxed rect rather than the stage — otherwise every box lands in the
  // wrong place. Stroke widths are divided back out so lines stay 2-3 screen px.
  const stage = ui.stage.getBoundingClientRect();
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
  for (const event of state.events.slice(-12)) {
    if (!event.hit || !event.boxRect) continue;
    const [x, y, w, h] = event.boxRect;
    ctx.strokeRect(x, y, w, h);
    ctx.fillText(`${event.node} ${event.score.toFixed(2)}`, x, Math.max(px(12), y - px(4)));
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

const REFRESH_RATES: Option[] = [
  { value: "500", label: "0.5 秒" },
  { value: "1000", label: "1 秒" },
  { value: "2000", label: "2 秒" },
  { value: "0", label: "关闭" },
];

const THEME_MODES: Option[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

function renderSettings() {
  syncSelect(
    "f-device",
    "设备",
    "仅一个设备时自动选中",
    state.devices.length ? asOptions(state.devices.map((d) => d.label)) : [],
    state.settings.preferredDevice ?? state.devices[0]?.label ?? "",
    (value) => void persist({ preferredDevice: value || null }),
    "未检测到设备",
  );
  syncSelect(
    "f-entry",
    "任务入口",
    undefined,
    asOptions(state.nodes.length ? state.nodes : ["Main"]),
    state.settings.entry,
    (value) => void persist({ entry: value }),
    "未加载资源",
  );
  syncSelect("f-interval", "画面刷新", undefined, REFRESH_RATES, String(state.settings.frameIntervalMs), (value) =>
    void persist({ frameIntervalMs: Number(value) }),
    "—",
  );
  syncSwitch("f-autostart", "连接后开始战斗", undefined, state.settings.autoStart, (v) => void persist({ autoStart: v }));
  syncSwitch("f-misses", "显示未命中节点", undefined, state.settings.showMisses, (v) => void persist({ showMisses: v }));
  syncSwitch("f-overlay", "画面叠加命中框", undefined, state.settings.overlayHits, (v) => void persist({ overlayHits: v }));
  syncSwitch("f-record", "记录节点画面", "保存到应用数据目录下的 frames", state.settings.recordFrames, (v) => void persist({ recordFrames: v }));
  syncSelect("f-theme", "外观", undefined, THEME_MODES, state.settings.themeMode, (value) => {
    applyTheme(value as ThemeMode);
    void persist({ themeMode: value as ThemeMode });
  }, "跟随系统");

  const version = fieldOf("f-version", "版本");
  if (version) {
    let text = version.querySelector("code");
    if (!text) {
      text = el("code");
      version.appendChild(text);
    }
    const textValue = `0.1.0 · ${api.mode}`;
    if (text.textContent !== textValue) text.textContent = textValue;
  }
  const reset = fieldOf("f-reset", "恢复默认设置");
  if (reset && !reset.querySelector("md-text-button")) {
    const btn = el("md-text-button");
    btn.appendChild(el("span", { text: "重置" }));
    btn.addEventListener("click", () => {
      void persist({ ...DEFAULT_SETTINGS }).then(() => applyTheme(state.settings.themeMode));
    });
    reset.appendChild(btn);
  }
}

function render() {
  const phase = state.status.phase;
  const chipLabels: Record<string, string> = { idle: "未连接", connecting: "连接中", ready: "已连接", running: "战斗中", error: "出错" };
  const problem = state.error || (phase === "error" ? state.status.detail : "");
  ui.chip.setAttribute("label", problem ? `${chipLabels[phase]} · ${problem}` : chipLabels[phase]);
  ui.chip.toggleAttribute("data-running", phase === "running");
  ui.meta.textContent =
    phase === "running" ? `${formatUptime(state.status.uptimeMs)} · 第 ${state.status.battles + 1} 局` : phase === "error" ? "" : state.status.detail;

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
    state.error = "";
  } catch (err) {
    state.error = String(err);
  }
  render();
}

/** Dev-only URL overrides so a headless screenshot run can target one page and
    one appearance without a pointer. */
function devOverrides() {
  if (!import.meta.env.DEV) return false;
  const params = new URLSearchParams(location.search);
  const page = params.get("page");
  if (page === "run" || page === "stats" || page === "settings") state.page = page;
  const theme = params.get("theme");
  if (theme === "light" || theme === "dark" || theme === "system") state.settings.themeMode = theme;
  return params.get("run") === "1";
}

async function boot() {
  api = await loadApi();
  state.settings = await api.settings();
  const startImmediately = devOverrides();
  applyTheme(state.settings.themeMode);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (state.settings.themeMode === "system") applyTheme("system");
  });
  buildShell();
  state.devices = await api.devices();
  state.nodes = await api.nodes();
  render();
  if (startImmediately) {
    await connect();
    await toggleRun();
  }
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

/** `?probe=1` publishes the layout metrics as the document title, which a
    headless `--dump-dom` run can read without a developer console. */
function probeLayout() {
  if (!import.meta.env.DEV || new URLSearchParams(location.search).get("probe") !== "1") return;
  const height = (selector: string) => {
    const node = document.querySelector(selector);
    return node ? Math.round(node.getBoundingClientRect().height) : null;
  };
  const rect = (selector: string) => {
    const node = document.querySelector(selector);
    if (!node) return null;
    const box = node.getBoundingClientRect();
    return [Math.round(box.top), Math.round(box.bottom)];
  };
  document.title = JSON.stringify({
    viewport: [innerWidth, innerHeight],
    column: height(".column"),
    stage: height(".card.grow"),
    timeline: height(".timeline"),
    columnRect: rect(".column"),
    actionsRect: rect(".actions"),
    scrolling: (document.querySelector(".page")?.scrollHeight ?? 0) > (document.querySelector(".page")?.clientHeight ?? 0),
  });
}

boot()
  .then(probeLayout)
  .catch((err) => {
    // A silent exception here leaves a blank window with nothing to read.
    document.body.textContent = `界面启动失败: ${String(err)}`;
  });
