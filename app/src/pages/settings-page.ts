import type { DeviceItem, Settings } from "../api";
import { DEFAULT_SETTINGS } from "../api";
import { el } from "../dom";
import { applyTheme, type ThemeMode } from "../theme";

interface SettingsPageState {
  settings: Settings;
  devices: DeviceItem[];
  nodes: string[];
  mode: "tauri" | "mock";
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
    el("div", { class: "field", id: "f-log" }),
    el("div", { class: "field", id: "f-version" }),
    el("div", { class: "field danger", id: "f-reset" }),
  );

  wrap.append(device, diag, look, about);
  return wrap;
}

export class SettingsPage {
  readonly element = buildSettingsPage();

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

  render(state: SettingsPageState) {
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
    this.syncSwitch("f-autostart", "连接后开始战斗", undefined, state.settings.autoStart, (v) => void this.persist({ autoStart: v }));
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
  }
}
