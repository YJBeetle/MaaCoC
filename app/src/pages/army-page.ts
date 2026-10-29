import type { ArmyCard, Settings, Status } from "../api";
import { encodeArmy, decodeArmy } from "../army-share";
import { el } from "../dom";
import { TacticsEditor } from "./tactics-editor";
import "./army-page.css";

const GROUPS = [["Unit", "兵种", 6], ["Spell", "法术", 6], ["Siege", "战车", 3]] as const;
type QuantityControl = { input: HTMLInputElement; minus: HTMLButtonElement; plus: HTMLButtonElement; host: HTMLElement };

/** Keep editable controls mounted while the engine polls status. */
export class ArmyPage {
  readonly element = el("section", { class: "army-page" });
  private grid = el("div", { class: "army-grid" });
  private selected = el("div", { class: "army-selected" });
  private totals = el("div", { class: "army-totals" });
  private summary = el("span", { class: "muted" });
  private results = el("span", { class: "muted" });
  private error = el("p", { class: "army-error", role: "alert" });
  private hint = el("p", { class: "army-hint" });
  private apply = el("md-filled-button", { text: "应用到手机" });
  private read = el("md-outlined-button", { text: "读取手机卡组" });
  private stop = el("md-outlined-button", { text: "停止当前流程" });
  private search = el("input", { type: "search", placeholder: "搜索英文名称…", "aria-label": "搜索卡牌" });
  private clear = el("md-text-button", { text: "清空" });
  private importButton = el("md-outlined-button", { text: "导入分享码" });
  private filters = new Map<string, HTMLButtonElement>();
  private controls = new Map<string, QuantityControl>();
  private selectedControls = new Map<string, QuantityControl>();
  private group = "Unit";
  private catalogKey = "";
  private selectedKey = "";
  private catalogCards: ArmyCard[] | null = null;
  private selectedCards: ArmyCard[] | null = null;
  private busy = false;
  private saving = false;
  private settings!: Settings;
  private cards: ArmyCard[] = [];
  private status!: Status;
  private tactics: TacticsEditor;

  constructor(private persist: (next: Partial<Settings>) => Promise<void>, apply: () => Promise<void>, stop: () => Promise<void>, read: () => Promise<void>) {
    this.tactics = new TacticsEditor(persist);
    const tabs = el("div", { class: "army-sections", role: "tablist", "aria-label": "配卡工作区" });
    const library = el("section", { class: "army-library card" });
    const toolbar = el("div", { class: "army-library-toolbar" });
    const filters = el("div", { class: "army-filters", role: "group", "aria-label": "卡牌类别" });
    for (const [group, label] of GROUPS) {
      const filter = el("button", { type: "button", text: label, "aria-pressed": String(group === this.group) });
      filter.addEventListener("click", () => { this.group = group; this.updateCatalog(); });
      this.filters.set(group, filter);
      filters.append(filter);
    }
    this.search.addEventListener("input", () => this.updateCatalog());
    toolbar.append(filters, this.search);
    const catalogMeta = el("div", { class: "army-catalog-meta" });
    catalogMeta.append(this.results, el("span", { text: "活动卡以游戏开放情况为准" }));
    library.append(toolbar, catalogMeta, this.grid);

    const deck = el("aside", { class: "army-deck card", "aria-label": "已选卡组" });
    const heading = el("div", { class: "army-deck-heading" });
    const title = el("div");
    title.append(el("h2", { text: "已选卡组" }), this.summary);
    heading.append(title, this.clear);
    this.clear.addEventListener("click", () => void this.save({ army: { entries: [] } }));
    this.apply.addEventListener("click", () => void apply());
    this.read.addEventListener("click", () => void read());
    this.stop.addEventListener("click", () => void stop());
    const actions = el("div", { class: "army-deck-actions" });
    actions.append(this.error, this.hint, this.apply, this.read, this.stop);
    deck.append(heading, this.totals, this.selected, actions);
    const workspace = el("div", { class: "army-workspace" });
    workspace.append(library, deck);
    const sharing = this.buildSharing();
    const panels = [workspace, this.tactics.element, sharing];
    const buttons = ["选卡", "战术", "分享"].map((label, index) => {
      const button = el("button", { type: "button", role: "tab", id: `army-tab-${index}`, "aria-controls": `army-panel-${index}`, text: label });
      button.addEventListener("click", () => select(index));
      button.addEventListener("keydown", event => {
        const offset = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
        const next = event.key === "Home" ? 0 : event.key === "End" ? 2 : (index + offset + 3) % 3;
        if (!offset && event.key !== "Home" && event.key !== "End") return;
        event.preventDefault(); select(next); buttons[next].focus();
      });
      tabs.append(button);
      panels[index].setAttribute("role", "tabpanel");
      panels[index].id = `army-panel-${index}`;
      panels[index].setAttribute("aria-labelledby", button.id);
      return button;
    });
    function select(index: number) {
      buttons.forEach((button, i) => { button.setAttribute("aria-selected", String(i === index)); button.tabIndex = i === index ? 0 : -1; panels[i].hidden = i !== index; });
    }
    select(0);
    this.element.append(tabs, ...panels);
  }

  private buildSharing() {
    const sharing = el("section", { class: "army-sharing card" });
    const share = el("textarea", { rows: "5", placeholder: "粘贴 MC2: 分享码", "aria-label": "卡组分享码", spellcheck: "false" });
    const message = el("p", { class: "army-hint", role: "status" });
    const exportButton = el("md-filled-button", { text: "生成分享码" });
    exportButton.addEventListener("click", () => {
      try { share.value = encodeArmy(this.settings); share.focus(); share.select(); message.textContent = "分享码已选中，可复制给其他玩家。"; }
      catch (err) { message.textContent = String(err); }
    });
    this.importButton.addEventListener("click", async () => {
      if (this.busy || this.saving) return;
      try {
        const next = decodeArmy(share.value, this.cards);
        message.textContent = await this.save(next) ? "已载入卡组和战术。到“选卡”检查后，再应用到手机。" : this.error.textContent;
      } catch (err) { message.textContent = String(err); }
    });
    const actions = el("div", { class: "army-toolbar" });
    actions.append(exportButton, this.importButton);
    sharing.append(el("h2", { text: "分享卡组与战术" }), el("p", { class: "muted", text: "分享码包含卡组、投放策略和自定义战术。导入后先在本地查看，再应用到手机。" }), share, actions, message);
    return sharing;
  }

  render(state: { settings: Settings; cards: ArmyCard[]; status: Status }) {
    this.settings = state.settings; this.cards = state.cards; this.status = state.status;
    this.busy = state.status.phase === "running";
    this.tactics.render(this.settings, this.cards, this.busy || this.saving);
    this.updateCatalog();
    this.updateSelection();
    this.updateControls();
  }

  private count(id: string) { return this.settings.army.entries.find(entry => entry.id === id)?.count ?? 0; }

  private quantity(card: ArmyCard, host: HTMLElement, label: string): QuantityControl {
    const wrap = el("div", { class: "quantity-control" });
    const minus = el("button", { type: "button", text: "−", "aria-label": `${label}${card.name} 减少` });
    const plus = el("button", { type: "button", text: "+", "aria-label": `${label}${card.name} 增加` });
    const input = el("input", { type: "number", min: "0", max: "400", step: "1", "aria-label": `${label}${card.name} 数量` });
    minus.addEventListener("click", () => void this.setCount(card.id, this.count(card.id) - 1));
    plus.addEventListener("click", () => void this.setCount(card.id, this.count(card.id) + 1));
    input.addEventListener("change", () => {
      const count = input.valueAsNumber;
      if (!Number.isInteger(count) || count < 0 || count > 400) { input.value = String(this.count(card.id)); return; }
      void this.setCount(card.id, count);
    });
    wrap.append(minus, input, plus); host.append(wrap);
    return { input, minus, plus, host };
  }

  private async setCount(id: string, count: number) {
    if (this.busy || this.saving) return;
    const entries = this.settings.army.entries.filter(entry => entry.id !== id);
    if (count > 0) entries.push({ id, count });
    await this.save({ army: { entries } });
  }

  private async save(next: Partial<Settings>): Promise<boolean> {
    if (this.busy || this.saving) return false;
    this.saving = true; this.updateControls();
    try { await this.persist(next); this.error.textContent = ""; return true; }
    catch (err) { this.error.textContent = String(err); return false; }
    finally { this.saving = false; this.render({ settings: this.settings, cards: this.cards, status: this.status }); }
  }

  private updateCatalog() {
    if (!this.settings) return;
    for (const [group, button] of this.filters) button.setAttribute("aria-pressed", String(group === this.group));
    const key = JSON.stringify([this.group, this.search.value]);
    if (key === this.catalogKey && this.catalogCards === this.cards) return;
    this.catalogKey = key; this.catalogCards = this.cards; this.controls.clear();
    const cards = this.cards.filter(card => card.group === this.group && card.name.toLowerCase().includes(this.search.value.trim().toLowerCase()));
    this.results.textContent = `${cards.length} 张卡牌`;
    this.grid.replaceChildren(...cards.map(card => {
      const tile = el("div", { class: "army-card" });
      tile.append(el("img", { src: card.preview, alt: "", loading: "lazy" }), el("span", { class: "army-card-name", text: card.name }));
      this.controls.set(card.id, this.quantity(card, tile, ""));
      return tile;
    }));
    if (!cards.length) this.grid.append(el("div", { class: "army-empty", text: this.cards.length ? "没有找到卡牌，试试其他名称或类别。" : "正在加载卡牌…" }));
    this.updateControls();
  }

  private updateSelection() {
    const entries = this.settings.army.entries;
    this.summary.textContent = entries.length ? `${entries.length} 种 · 共 ${entries.reduce((sum, entry) => sum + entry.count, 0)} 张` : "从左侧添加卡牌";
    this.totals.replaceChildren(...GROUPS.map(([group, name, max]) => {
      const amount = entries.filter(entry => this.cards.find(card => card.id === entry.id)?.group === group).length;
      return el("span", { class: amount > max ? "over-limit" : "", text: `${name} ${amount}/${max} 种` });
    }));
    // Quantity changes must not replace a focused input or move a row.
    const key = JSON.stringify(entries.map(entry => entry.id).sort());
    if (key === this.selectedKey && this.selectedCards === this.cards) return;
    this.selectedKey = key; this.selectedCards = this.cards; this.selectedControls.clear();
    const cards = this.cards.filter(card => entries.some(entry => entry.id === card.id));
    this.selected.replaceChildren(...cards.map(card => {
      const row = el("div", { class: "army-selected-row" });
      row.append(el("img", { src: card.preview, alt: "" }), el("span", { text: card.name }));
      this.selectedControls.set(card.id, this.quantity(card, row, "已选 "));
      return row;
    }));
    if (!entries.length) {
      const empty = el("div", { class: "army-empty" });
      empty.append(el("md-icon", { text: "style" }), el("strong", { text: "组建你的军队" }), el("span", { text: "点击卡牌下方的 +，或读取手机上的现有配置。" }));
      this.selected.append(empty);
    }
  }

  private updateControls() {
    if (!this.settings) return;
    const locked = this.busy || this.saving;
    for (const controls of [this.controls, this.selectedControls]) for (const [id, control] of controls) {
      const count = this.count(id);
      if (document.activeElement !== control.input) control.input.value = String(count);
      control.input.disabled = locked;
      control.minus.disabled = locked || count === 0;
      control.plus.disabled = locked || count >= 400;
      control.host.classList.toggle("selected", count > 0);
    }
    const overLimit = GROUPS.some(([group, , max]) => this.settings.army.entries.filter(entry => this.cards.find(card => card.id === entry.id)?.group === group).length > max);
    this.apply.toggleAttribute("disabled", locked || this.status?.phase !== "ready" || !this.settings.army.entries.length || overLimit);
    this.read.toggleAttribute("disabled", locked || this.status?.phase !== "ready");
    this.clear.toggleAttribute("disabled", locked || !this.settings.army.entries.length);
    this.importButton.toggleAttribute("disabled", locked);
    this.stop.hidden = !this.busy;
    this.hint.textContent = this.busy ? "流程运行中，停止后可编辑卡组。" : overLimit ? "卡牌种类超出上方限制，请先减少种类。" : this.status?.phase !== "ready" ? "连接设备后可读取或应用卡组。" : "应用时检查可用卡牌和容量，然后替换部队、法术与战车。英雄、宠物和装备保留。";
  }
}
