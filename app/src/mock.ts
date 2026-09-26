import type { EngineApi, Frame, NodeEvent, Settings, Status } from "./api";
import { DEFAULT_SETTINGS } from "./api";
import frameUrl from "./dev/frame.png?url";

/** Browser-only stand-in so the UI can be driven and asserted without a phone
    or a native window. Loaded only from a dev build. */

const SCRIPT: Array<Partial<NodeEvent> & { at: number; node: string; kind: string }> = [
  { at: 0.0, node: "Main", kind: "recognition", focus: "", hit: true, candidates: 0, filtered: 0, score: 0 },
  { at: 0.2, node: "Main", kind: "action" },
  { at: 2.9, node: "Exception", kind: "recognition", focus: "Exception!", hit: false, score: 0.105, candidates: 1, filtered: 0 },
  { at: 5.2, node: "AttackStart", kind: "recognition", focus: "AttackStart!", hit: true, score: 0.998, candidates: 4, filtered: 1, boxRect: [67, 586, 96, 74] },
  { at: 5.5, node: "AttackStart", kind: "action" },
  { at: 8.1, node: "FindMatch", kind: "recognition", focus: "FindMatch!", hit: true, score: 0.995, candidates: 2, filtered: 1, boxRect: [199, 502, 95, 31] },
  { at: 8.4, node: "FindMatch", kind: "action" },
  { at: 12.0, node: "FindSoldier", kind: "recognition", focus: "FindSoldier!", hit: true, score: 0.996, candidates: 52, filtered: 5, boxRect: [92, 594, 90, 118] },
  { at: 12.3, node: "FindSoldier", kind: "action" },
  { at: 13.1, node: "DeploySoldier", kind: "action" },
  { at: 14.0, node: "FindHero", kind: "recognition", focus: "FindHero!", hit: true, score: 0.962, candidates: 5, filtered: 1, boxRect: [794, 596, 86, 113] },
  { at: 14.4, node: "SkillHero", kind: "action" },
  { at: 41.2, node: "FindSpell", kind: "recognition", focus: "FindSpell!", hit: false, score: 0.794, candidates: 2, filtered: 0 },
  { at: 62.8, node: "WaitingEnd", kind: "recognition", focus: "", hit: true, score: 0.867, candidates: 2, filtered: 2, boxRect: [50, 511, 147, 52] },
  { at: 70.1, node: "ReturnHome", kind: "recognition", focus: "ReturnHome!", hit: true, score: 0.831, candidates: 2, filtered: 1, boxRect: [557, 583, 170, 76] },
  { at: 70.4, node: "ReturnHome", kind: "action" },
];

export function createMockApi(): EngineApi {
  let settings: Settings = { ...DEFAULT_SETTINGS };
  let status: Status = { phase: "idle", detail: "", battles: 0, uptimeMs: 0, currentNode: "" };
  let cursor = 0;
  let clock = 0;

  const frame: Frame = { dataUrl: frameUrl, width: 1280, height: 720, landscape: true };

  return {
    mode: "mock",
    async devices() {
      return [{ label: "K40", address: "f5d66ad2" }];
    },
    async nodes() {
      return ["Main", "AttackLoop", "Launch"];
    },
    async settings() {
      return settings;
    },
    async paths() {
      return { logDir: "~/Library/Logs/com.maacoc.client" };
    },
    async saveSettings(next) {
      settings = next;
      return settings;
    },
    async connect() {
      status = { ...status, phase: settings.autoStart ? "running" : "ready", detail: "K40 @ f5d66ad2" };
      return status;
    },
    async disconnect() {
      status = { phase: "idle", detail: "", battles: 0, uptimeMs: 0, currentNode: "" };
      cursor = 0;
      clock = 0;
      return status;
    },
    async start() {
      status = { ...status, phase: "running" };
      cursor = 0;
      clock = 0;
      return status;
    },
    async stop() {
      status = { ...status, phase: "ready" };
      return status;
    },
    async status() {
      if (status.phase === "running") {
        clock += 0.25;
        status = { ...status, uptimeMs: clock * 1000 };
      }
      return status;
    },
    async frame() {
      return status.phase === "idle" ? null : frame;
    },
    async events() {
      if (status.phase !== "running") return [];
      const batch = SCRIPT.slice(cursor, cursor + 2);
      cursor += batch.length;
      if (cursor >= SCRIPT.length) cursor = 0;
      // Mirror the shell: a new AttackStart action is a new battle, and the
      // status carries the node the run is currently sitting on.
      for (const item of batch) {
        if (item.kind === "action" && item.node === "AttackStart") status = { ...status, battles: status.battles + 1 };
        status = { ...status, currentNode: item.focus || item.node };
      }
      return batch.map((item) => ({
        at: item.at,
        wall: 0,
        recoId: 0,
        kind: item.kind as NodeEvent["kind"],
        node: item.node,
        focus: item.focus ?? "",
        hit: item.hit ?? false,
        boxRect: item.boxRect ?? null,
        score: item.score ?? 0,
        candidates: item.candidates ?? 0,
        filtered: item.filtered ?? 0,
        error: false,
      })) as NodeEvent[];
    },
  };
}
