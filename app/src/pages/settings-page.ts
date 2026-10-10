import type { DeviceItem, Settings, Strategy, LootFilter } from "../api";
import { DEFAULT_SETTINGS } from "../api";
import { el } from "../dom";
import { applyTheme, type ThemeMode } from "../theme";

interface SettingsPageState {
  settings: Settings;
  devices: DeviceItem[];
  nodes: string[];
  mode: "tauri" | "mock";
  running: boolean;
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

function buildSettingsPage(): HTMLElement {
  const wrap = el("div", { class: "column" });
  const device = el("div", { class: "card bordered", id: "card-device" });
  device.append(
    el("div", { class: "sect", text: "设备" }),
    el("div", { class: "field", id: "f-device" }),
    el("div", { class: "field", id: "f-entry" }),
    el("div", { class: "field", id: "f-strategy" }),
    el("div", { class: "field", id: "f-full-stop" }),
    el("div", { class: "field", id: "f-interval" }),
    el("div", { class: "field", id: "f-autostart" }),
  );

  const loot = el("div", { class: "card bordered" });
  loot.append(el("div", { class: "sect", text: "开战收益筛选" }));
  for (const id of ["enabled", "mode", "gold", "elixir", "dark", "total", "skips"]) {
    loot.append(el("div", { class: "field", id: `f-loot-${id}` }));
  }

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
    el("div", { class: "field", id: "f-log" }),
    el("div", { class: "field", id: "f-version" }),
    el("div", { class: "field danger", id: "f-reset" }),
  );

  wrap.append(device, loot, diag, look, about);
  return wrap;
}

export class SettingsPage {
  readonly element = buildSettingsPage();

  private lootFilter = DEFAULT_SETTINGS.lootFilter;

  private updateLoot = (patch: Partial<LootFilter>) => {
    this.lootFilter = { ...this.lootFilter, ...patch };
    void this.persist({ lootFilter: this.lootFilter });
  };

  constructor(private persist: (next: Partial<Settings>) => Promise<void>) {}

  /** Controls are created the first time they are needed and only updated
      afterwards. Rebuilding them on every 250 ms poll made the outlined selects
      visibly flicker. */
  private fieldOf(id: string, title: string, hint?: string): HTMLElement | null {
    const field = this.element.querySelector<HTMLElement>(`#${id}`);
    if (!field) return null;
    if (!field.firstElementChild) {
      const left = el("div");
      left.append(el("div", { class: "k", text: title }), ...(hint ? [el("div", { class: "hint", text: hint })] : []));
      field.appendChild(left);
    } else if (hint !== undefined) {
      // The hint can be live text (which device is connected), so it updates too.
      const node = field.querySelector(".hint");
      if (node && node.textContent !== hint) node.textContent = hint;
    }
    return field;
  }

  /** The field's own heading is the visible label, so the select only carries an
      accessible one — a second copy inside the notch read as a duplicate.
      MWC derives the displayed text when the field first renders and does not
      refresh it for a later programmatic selection, so a select whose option list
      changed is rebuilt off-DOM with the choice already applied. MWC has no
      placeholder, so an unavailable choice shows as a disabled stand-in row. */
  private syncSelect(
    id: string,
    title: string,
    hint: string | undefined,
    options: Option[],
    value: string,
    onChange: (v: string) => void,
    emptyLabel: string,
  ) {
    const field = this.fieldOf(id, title, hint);
    if (!field) return null;
    const items = options.length ? options : [{ value: "-", label: emptyLabel }];
    const wanted = options.length ? value : "-";
    const signature = items.map((o) => o.value).join("\n");
    let select = field.querySelector<Select>("md-outlined-select");
    if (select?.dataset.items !== signature || select.value !== wanted) {
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

  private syncSwitch(id: string, title: string, hint: string | undefined, value: boolean, onChange: (v: boolean) => void) {
    const field = this.fieldOf(id, title, hint);
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

  private syncNumber(id: string, title: string, value: number, min: number, max: number, key: keyof LootFilter, disabled: boolean) {
    const field = this.fieldOf(id, title, key === "maxSkips" ? "达到上限后停止，避免一直消耗搜索费用。" : "输入整数，0 表示不限制该项。单位：资源数量。");
    if (!field) return;
    let input = field.querySelector<HTMLInputElement>("input");
    if (!input) {
      input = document.createElement("input");
      input.type = "number"; input.min = String(min); input.max = String(max); input.step = "1";
      input.setAttribute("aria-label", title);
      input.style.cssText = "width:260px;max-width:100%;padding:12px;border:1px solid var(--md-sys-color-outline);border-radius:4px;background:transparent;color:inherit;font:inherit;box-sizing:border-box";
      const control = input;
      input.addEventListener("change", () => {
        const number = control.valueAsNumber;
        control.setCustomValidity(Number.isSafeInteger(number) && number >= min && number <= max ? "" : `请输入 ${min}～${max} 的整数`);
        if (control.reportValidity()) this.updateLoot({ [key]: number });
      });
      field.append(input);
    }
    if (document.activeElement !== input) input.value = String(value);
    input.disabled = disabled;
  }

  render(state: SettingsPageState) {
    this.lootFilter = state.settings.lootFilter ?? DEFAULT_SETTINGS.lootFilter;
    const loot = this.lootFilter;
    const enabled = this.syncSwitch("f-loot-enabled", "启用收益筛选", "停止后修改，下一次开始生效。只在搜索对手时判断，已开战时继续原流程。", loot.enabled, (v) => this.updateLoot({ enabled: v }));
    enabled?.toggleAttribute("disabled", state.running);
    const mode = this.syncSelect("f-loot-mode", "条件关系", "只计算大于 0 的门槛；全部为 0 时不限制收益。", [{value:"any",label:"任一条件满足就开战"},{value:"all",label:"全部条件满足才开战"}], loot.matchAny ? "any" : "all", (v) => this.updateLoot({matchAny:v === "any"}), "—");
    if (mode) mode.disabled = state.running || !loot.enabled;
    for (const [id,title,key] of [["gold","最低金币","minGold"],["elixir","最低圣水","minElixir"],["dark","最低黑油","minDark"],["total","最低金币＋圣水合计","minGoldElixir"],["skips","连续跳过上限","maxSkips"]] as const) {
      this.syncNumber(`f-loot-${id}`, title, loot[key], key === "maxSkips" ? 1 : 0, key === "maxSkips" ? 1000 : 1000000000, key, state.running || !loot.enabled);
    }

    const strategy = this.syncSelect(
      "f-strategy", "投放策略", "停止后切换，下一次开始生效。分散预设会缩小、拖动画面后在左侧多点投放。",
      [{ value: "spells-first", label: "法术优先（原有顺序）" }, { value: "units-first", label: "兵种优先" }, { value: "left-spread", label: "左侧分散投放" }, { value: "air-probe", label: "气球探路 + 分散投放" }],
      state.settings.strategy, (value) => void this.persist({ strategy: value as Strategy }), "—",
    );
    if (strategy) strategy.disabled = state.running;
    const fullStop = this.syncSwitch("f-full-stop", "满仓后停止刷资源", "金币、圣水和黑油全部达到容量时停止；无法读取时提示并继续。", state.settings.stopWhenFull, (v) => void this.persist({ stopWhenFull: v }));
    if (fullStop) fullStop.toggleAttribute("disabled", state.running);

    this.syncSelect(
      "f-device",
      "设备",
      "仅一个设备时自动选中",
      state.devices.length ? asOptions(state.devices.map((d) => d.label)) : [],
      state.settings.preferredDevice ?? state.devices[0]?.label ?? "",
      (value) => void this.persist({ preferredDevice: value || null }),
      "未检测到设备",
    );
    this.syncSelect(
      "f-entry",
      "任务入口",
      undefined,
      asOptions(state.nodes.length ? state.nodes : ["Main"]),
      state.settings.entry,
      (value) => void this.persist({ entry: value }),
      "未加载资源",
    );
    this.syncSelect("f-interval", "画面刷新", undefined, REFRESH_RATES, String(state.settings.frameIntervalMs), (value) =>
      void this.persist({ frameIntervalMs: Number(value) }),
      "—",
    );
    this.syncSwitch("f-autostart", "连接后开始刷资源", undefined, state.settings.autoStart, (v) => void this.persist({ autoStart: v }));
    this.syncSwitch("f-misses", "显示未命中节点", undefined, state.settings.showMisses, (v) => void this.persist({ showMisses: v }));
    this.syncSwitch("f-overlay", "画面叠加命中框", undefined, state.settings.overlayHits, (v) => void this.persist({ overlayHits: v }));
    this.syncSwitch("f-record", "记录节点画面", "保存到应用数据目录下的 frames", state.settings.recordFrames, (v) => void this.persist({ recordFrames: v }));
    this.syncSelect(
      "f-theme",
      "外观",
      undefined,
      THEME_MODES,
      state.settings.themeMode,
      (value) => {
        applyTheme(value as ThemeMode);
        void this.persist({ themeMode: value as ThemeMode });
      },
      "跟随系统",
    );

    const version = this.fieldOf("f-version", "版本");
    if (version) {
      let text = version.querySelector("code");
      if (!text) {
        text = el("code");
        version.appendChild(text);
      }
      const textValue = `0.1.0 · ${state.mode}`;
      if (text.textContent !== textValue) text.textContent = textValue;
    }
    const reset = this.fieldOf("f-reset", "恢复默认设置");
    if (reset && !reset.querySelector("md-text-button")) {
      const btn = el("md-text-button");
      btn.appendChild(el("span", { text: "重置" }));
      btn.addEventListener("click", () => {
        void this.persist({ ...DEFAULT_SETTINGS }).then(() => applyTheme(DEFAULT_SETTINGS.themeMode));
      });
      reset.appendChild(btn);
    }
    reset?.querySelector("md-text-button")?.toggleAttribute("disabled", state.running);
  }
}
