import type { ArmyCard, Settings, Status } from "../api";
import { encodeArmy, decodeArmy } from "../army-share";
import { el } from "../dom";
import { TacticsEditor } from "./tactics-editor";

/** Keep the editable list stable while the engine polls status. */
export class ArmyPage {
  readonly element = el("section", { class: "army-page" });
  private grid = el("div", { class: "army-grid" });
  private summary = el("p");
  private error = el("p", { role: "alert" });
  private apply = el("md-filled-button", { text: "应用到手机（替换当前配置）" });
  private read = el("md-outlined-button", { text: "读取手机当前卡组" });
  private stop = el("md-outlined-button", { text: "停止" });
  private search = el("input", { type: "search", placeholder: "搜索英文名称", "aria-label": "搜索卡牌" });
  private busy = false;
  private clear = el("md-text-button", { text: "清空配置" });
  private group = "Unit";
  private key = "";
  private settings!: Settings;
  private cards: ArmyCard[] = [];
  private tactics: TacticsEditor;

  constructor(private persist: (next: Partial<Settings>) => Promise<void>, apply: () => Promise<void>, stop: () => Promise<void>, read: () => Promise<void>) {
    this.tactics = new TacticsEditor(persist);
    const tabs = el("div", { class: "army-toolbar" });
    for (const [group, label] of [["Unit", "兵种"], ["Spell", "法术"], ["Siege", "战车"]]) {
      const tab = el("md-outlined-button", { text: label });
      tab.addEventListener("click", () => { this.group = group; this.key = ""; this.updateGrid(); });
      tabs.append(tab);
    }
    this.search.addEventListener("input", () => { this.key = ""; this.updateGrid(); });
    tabs.append(this.search);
    const clear = this.clear;
    clear.addEventListener("click", () => void this.save({ army: { entries: [] } }));
    this.apply.addEventListener("click", () => void apply());
    this.read.addEventListener("click", () => void read());
    this.stop.addEventListener("click", () => void stop());
    const actions = el("div", { class: "army-toolbar" });
    actions.append(this.read, this.apply, this.stop, clear);
    const share = el("textarea", { rows: "3", placeholder: "粘贴 MC2: 分享码", "aria-label": "卡组分享码" });
    const message = el("p", { role: "status" });
    const exportButton = el("md-outlined-button", { text: "生成分享码" });
    exportButton.addEventListener("click", () => {
      if (!this.settings.army.entries.length) { message.textContent = "请先配置卡组"; return; }
      try { share.value = encodeArmy(this.settings); share.select(); }
      catch (err) { message.textContent = String(err); return; }
      message.textContent = "分享码已生成，包含卡组、当前策略和自定义战术；可复制给其他玩家。";
    });
    const importButton = el("md-outlined-button", { text: "导入分享码" });
    importButton.addEventListener("click", async () => {
      if (this.busy) { message.textContent = "请先停止正在运行的流程"; return; }
      try { await this.persist(decodeArmy(share.value, this.cards)); message.textContent = "已载入卡组和策略，检查后点击应用到手机。"; }
      catch (err) { message.textContent = String(err); }
    });
    const sharing = el("div", { class: "army-sharing" });
    const shareActions = el("div", { class: "army-toolbar" });
    shareActions.append(exportButton, importButton);
    sharing.append(el("h2", { text: "卡组与策略分享" }), share, shareActions, message);
    this.element.append(el("p", { text: "先检查卡牌是否可用及容量，再替换部队、法术和战车。英雄、宠物和装备保留。" }),
      el("p", { text: "最多配置 6 种兵、6 种法术和 3 种战车；数量 0 表示不携带。活动卡以游戏当前开放情况为准。" }), tabs, this.summary, this.error, actions, this.tactics.element, sharing, this.grid);
  }

  render(state: { settings: Settings; cards: ArmyCard[]; status: Status }) {
    this.settings = state.settings;
    this.cards = state.cards;
    const running = state.status.phase === "running";
    this.busy = running;
    this.tactics.render(this.settings,this.cards,running);
    this.clear.toggleAttribute("disabled", running);
    this.apply.toggleAttribute("disabled", state.status.phase !== "ready" || !this.settings.army.entries.length);
    this.read.toggleAttribute("disabled", state.status.phase !== "ready");
    this.stop.toggleAttribute("disabled", !running);
    this.grid.querySelectorAll<HTMLInputElement>("input").forEach(input => { input.disabled = running; });
    this.summary.textContent = this.settings.army.entries.map(entry => `${this.cards.find(c => c.id === entry.id)?.name || entry.id} ×${entry.count}`).join(" · ") || "尚未选择卡牌";
    this.updateGrid();
  }

  private async save(next: Partial<Settings>) {
    try { await this.persist(next); this.error.textContent = ""; }
    catch (err) { this.error.textContent = String(err); this.key = ""; this.updateGrid(); }
  }

  private updateGrid() {
    if (!this.settings) return;
    const key = JSON.stringify([this.group, this.search.value, this.settings.army]);
    if (key === this.key) return;
    this.key = key;
    this.grid.replaceChildren();
    for (const card of this.cards.filter(c => c.group === this.group && c.name.toLowerCase().includes(this.search.value.toLowerCase()))) {
      const count = this.settings.army.entries.find(e => e.id === card.id)?.count || 0;
      const tile = el("label", { class: `army-card${count ? " selected" : ""}` });
      const input = el("input", { type: "number", min: "0", max: "400", step: "1", value: String(count), "aria-label": `${card.name} 数量` });
      input.disabled = this.busy;
      input.addEventListener("change", () => {
        const quantity = Number(input.value);
        if (!Number.isInteger(quantity) || quantity < 0 || quantity > 400) { input.value = String(count); return; }
        const entries = this.settings.army.entries.filter(e => e.id !== card.id);
        if (quantity) entries.push({ id: card.id, count: quantity });
        void this.save({ army: { entries } });
      });
      tile.append(el("img", { src: card.preview, alt: card.name, loading: "lazy" }), el("span", { text: card.name }), input);
      this.grid.append(tile);
    }
  }
}
