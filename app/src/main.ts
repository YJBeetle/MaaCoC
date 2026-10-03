/* Bundled, not a CDN link: the shipped .app has to render offline, and a missing
   icon font shows the literal glyph names ("play_arrow") instead of icons. */
import "@material-symbols/font-400/outlined.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "./styles.css";
import "@material/web/button/filled-button.js";
import "@material/web/button/outlined-button.js";
import "@material/web/button/text-button.js";
import "@material/web/fab/fab.js";
import "@material/web/chips/assist-chip.js";
import "@material/web/icon/icon.js";
import "@material/web/switch/switch.js";
import "@material/web/select/outlined-select.js";
import "@material/web/select/select-option.js";

import type { EngineApi, Frame, NodeEvent, Paths, Phase, Settings, Status } from "./api";
import { DEFAULT_SETTINGS, formatUptime } from "./api";
import { applyTheme } from "./theme";
import { el } from "./dom";
import { RunPage } from "./pages/run-page";
import { StatsPage } from "./pages/stats-page";
import { SettingsPage } from "./pages/settings-page";

const stateLabels: Record<Phase, string> = {
  idle: "未连接",
  connecting: "连接中",
  ready: "已连接",
  running: "战斗中",
  error: "出错",
};

const idle: Status = { phase: "idle", detail: "", battles: 0, uptimeMs: 0, currentNode: "", panel: "" };

const state = {
  page: "run" as "run" | "stats" | "settings",
  status: idle,
  settings: { ...DEFAULT_SETTINGS } as Settings,
  events: [] as NodeEvent[],
  frame: null as Frame | null,
  devices: [] as { label: string; address: string }[],
  paths: { logDir: "" } as Paths,
  nodes: [] as string[],
  error: "",
  autoConnect: false,
};

async function loadApi(): Promise<EngineApi> {
  const inTauri = "__TAURI_INTERNALS__" in window;
  // ?mock=1 lets the real Tauri window run on the fake engine, so layout can be
  // checked in WKWebView (which sizes things differently from Chrome) without a
  // device and without clicking anything.
  const forceMock = import.meta.env.DEV && new URLSearchParams(location.search).get("mock") === "1";
  if (inTauri && !forceMock) {
    const { invoke } = await import("@tauri-apps/api/core");
    const call = <T>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args);
    return {
      mode: "tauri",
      devices: () => call("devices"),
      nodes: () => call("nodes"),
      settings: () => call("settings"),
      paths: () => call<Paths>("paths"),
      saveSettings: (next) => call("save_settings", { next }),
      connect: () => call("connect"),
      disconnect: () => call("disconnect"),
      setScreenSize: (reset) => call("screen_size", { reset }),
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

/** Swap a label in place; rebuilding the button on every 4 Hz render flickered. */
function setLabel(host: HTMLElement, className: string, text: string) {
  let node = host.querySelector(`.${className}`);
  if (!node) {
    node = el("span", { class: className });
    host.appendChild(node);
  }
  if (node.textContent !== text) node.textContent = text;
}

const ui = {
  title: el("h1", { text: "挂机", id: "page-title" }),
  chip: el("span", { id: "status-note" }),
  meta: el("span", { class: "meta", id: "run-meta", text: "" }),
  link: el("md-outlined-button", { id: "btn-link" }),
  pages: {} as Record<string, HTMLElement>,
};

const runPage = new RunPage({ toggleRun, changeScreenSize });
const statsPage = new StatsPage();
const settingsPage = new SettingsPage(persist);

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
  const linkLabel = el("span", { class: "swap" });
  linkLabel.append(el("span", { class: "state", text: "未连接" }), el("span", { class: "action", text: "连接设备" }));
  ui.link.append(linkLabel);
  ui.link.addEventListener("click", () => {
    const connected = state.status.phase === "ready" || state.status.phase === "running";
    void (connected ? disconnectNow() : connect());
  });
  topbar.append(ui.title, el("span", { class: "grow" }), ui.meta, ui.chip, ui.link);

  const main = el("div", { class: "main" });
  ui.pages.run = runPage.element;
  ui.pages.stats = statsPage.element;
  ui.pages.settings = settingsPage.element;

  const body = el("div", { class: "page", id: "page-run" });
  body.appendChild(ui.pages.run);
  main.append(topbar, body);
  app.append(rail, main);
  document.body.appendChild(app);
  go(state.page);
}

async function persist(next: Partial<Settings>) {
  state.settings = { ...state.settings, ...next };
  state.settings = await api.saveSettings(state.settings);
  render();
}

async function connect() {
  state.error = "";
  render();
  try {
    state.status = await api.connect();
    state.devices = await api.devices();
    state.nodes = await api.nodes();
  } catch (err) {
    state.error = String(err);
  } finally {
    render();
  }
}

async function disconnectNow() {
  state.error = "";
  state.frame = null;
  runPage.clearPreview();
  try {
    state.status = await api.disconnect();
  } catch (err) {
    state.error = String(err);
  }
  render();
}

let sizeBusy = false;

/** `wm size` over the framework's own adb channel. Both directions live here
    because the phone has to be told twice a session at most. The command
    re-reads the panel afterwards, so a status poll picks up the new size and
    the stage badge updates itself. */
async function changeScreenSize(reset: boolean) {
  state.error = "";
  sizeBusy = true;
  render();
  try {
    await api.setScreenSize(reset);
    state.status = await api.status();
  } catch (err) {
    state.error = String(err);
  }
  sizeBusy = false;
  render();
}

function renderConnection() {
  const phase = state.status.phase;
  const connected = phase === "ready" || phase === "running";
  // Swap text in place: rebuilding the button on every 4 Hz render flickered.
  setLabel(ui.link, "state", stateLabels[phase]);
  setLabel(ui.link, "action", connected ? "断开连接" : "连接设备");
  ui.link.toggleAttribute("disabled", phase === "connecting");
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
  // The action row lives inside the run page, so switching pages hides it.
  document.querySelector(".page")?.replaceChildren(ui.pages[page]);
  render();
}

function render() {
  const phase = state.status.phase;
  const problem = state.error || (phase === "error" ? state.status.detail : "");
  ui.chip.textContent = problem;
  ui.chip.classList.toggle("error", Boolean(problem));
  const running = phase === "running";
  const node = state.status.currentNode;
  ui.meta.textContent = running
    ? `${formatUptime(state.status.uptimeMs)} · 第 ${state.status.battles + 1} 局${node ? ` · ${node}` : ""}`
    : phase === "error"
      ? ""
      : state.status.detail;

  renderConnection();

  if (probing) probeLayout();
  if (state.page === "run") {
    runPage.render({ ...state, sizeBusy });
  } else if (state.page === "stats") {
    statsPage.render(state);
  } else {
    settingsPage.render({ ...state, mode: api.mode });
  }
}

let lastFrameAt = 0;
let frameInFlight = false;

async function tick() {
  try {
    state.status = await api.status();
    const incoming = await api.events();
    if (incoming.length) {
      state.events.push(...incoming);
      if (state.events.length > 400) state.events.splice(0, state.events.length - 400);
    }
    const interval = state.settings.frameIntervalMs;
    // One capture at a time: a slow screenshot must not queue up behind itself.
    if (interval > 0 && !frameInFlight && Date.now() - lastFrameAt > interval) {
      frameInFlight = true;
      lastFrameAt = Date.now();
      try {
        const frame = await api.frame();
        // A dropped poll keeps the last picture instead of blanking the stage.
        if (frame) state.frame = frame;
      } finally {
        frameInFlight = false;
      }
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
  // With ?probe=1 the live layout numbers go into the window title, so a real
  // WKWebView screenshot can be measured instead of guessed at from Chrome.
  probing = params.get("probe") === "1";
  const page = params.get("page");
  if (page === "run" || page === "stats" || page === "settings") state.page = page;
  const theme = params.get("theme");
  if (theme === "light" || theme === "dark" || theme === "system") state.settings.themeMode = theme;
  if (params.get("connect") === "1") state.autoConnect = true;
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
  window.addEventListener("pagehide", () => runPage.dispose(), { once: true });
  state.paths = await api.paths();
  state.devices = await api.devices();
  state.nodes = await api.nodes();
  render();
  if (startImmediately) {
    await connect();
    await toggleRun();
  } else if (state.autoConnect) {
    await connect();
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
let probing = false;

function probeLayout() {
  if (!probing) return;
  const box = (selector: string) => {
    const node = document.querySelector(selector);
    if (!node) return "-";
    const r = node.getBoundingClientRect();
    return `${Math.round(r.top)}..${Math.round(r.bottom)} (h${Math.round(r.height)})`;
  };
  const page = document.querySelector(".page");
  const timeline = document.getElementById("timeline");
  let readout = document.getElementById("probe-box");
  if (!readout) {
    readout = el("div", { id: "probe-box" });
    readout.style.cssText =
      "position:fixed;left:96px;top:70px;z-index:999;padding:6px 9px;border-radius:6px;" +
      "background:#000c;color:#7cfc00;font:12px/1.45 ui-monospace,Menlo,monospace;white-space:pre;pointer-events:none";
    document.body.appendChild(readout);
  }
  readout.textContent = [
    `viewport      ${innerWidth} x ${innerHeight}`,
    `.page         ${box(".page")}  client ${page?.clientHeight} scroll ${page?.scrollHeight}`,
    `.run-grid     ${box(".run-grid")}`,
    `.card.grow    ${box(".card.grow")}`,
    `.timeline-card ${box(".timeline-card")}`,
    `.timeline     h${timeline?.clientHeight} scrollH ${timeline?.scrollHeight}`,
  ].join("\n");
}

boot()
  .then(probeLayout)
  .catch((err) => {
    // A silent exception here leaves a blank window with nothing to read.
    document.body.textContent = `界面启动失败: ${String(err)}`;
  });
