import type { NodeEvent, Status } from "../api";
import { el } from "../dom";

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

export class StatsPage {
  readonly element = buildStatsPage();

  render(state: { events: NodeEvent[]; status: Status }) {
    const hits = state.events.filter((e) => e.kind === "recognition" && e.hit).length;
    const misses = state.events.filter((e) => e.kind === "recognition" && !e.hit).length;
    const set = (id: string, value: string) => {
      const node = this.element.querySelector<HTMLElement>(`#${id}`);
      if (node) node.textContent = value;
    };
    set("stat-battles", String(state.status.battles));
    set("stat-hits", String(hits));
    set("stat-misses", String(misses));
    const rank = this.element.querySelector("#miss-rank");
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
}
