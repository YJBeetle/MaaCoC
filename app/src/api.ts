/** Contract between the UI and the engine.

The same interface is served by the Tauri shell in the app and by a mock in the
browser, so the UI can be driven and asserted without a device or a window. */

export interface NodeEvent {
  at: number;
  wall: number;
  recoId: number;
  kind: "recognition" | "action";
  node: string;
  focus: string;
  hit: boolean;
  boxRect: [number, number, number, number] | null;
  score: number;
  candidates: number;
  filtered: number;
  error: boolean;
}

export type Phase = "idle" | "connecting" | "ready" | "running" | "error";

export interface Status {
  phase: Phase;
  detail: string;
  battles: number;
  uptimeMs: number;
  currentNode: string;
}

export interface Settings {
  entry: string;
  preferredDevice: string | null;
  autoStart: boolean;
  frameIntervalMs: number;
  showMisses: boolean;
  overlayHits: boolean;
  recordFrames: boolean;
  themeMode: "system" | "light" | "dark";
}

export interface DeviceItem {
  label: string;
  address: string;
}

export interface Frame {
  dataUrl: string;
  width: number;
  height: number;
}

/** Where the app keeps its own files — the address to ask for when reporting a bug. */
export interface Paths {
  logDir: string;
}

export interface EngineApi {
  readonly mode: "tauri" | "mock";
  devices(): Promise<DeviceItem[]>;
  nodes(): Promise<string[]>;
  settings(): Promise<Settings>;
  saveSettings(next: Settings): Promise<Settings>;
  connect(): Promise<Status>;
  disconnect(): Promise<Status>;
  start(): Promise<Status>;
  stop(): Promise<Status>;
  status(): Promise<Status>;
  paths(): Promise<Paths>;
  frame(): Promise<Frame | null>;
  events(): Promise<NodeEvent[]>;
}

export const DEFAULT_SETTINGS: Settings = {
  entry: "Main",
  preferredDevice: null,
  autoStart: false,
  frameIntervalMs: 1000,
  showMisses: false,
  overlayHits: false,
  recordFrames: false,
  themeMode: "system",
};

export function formatUptime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const minutes = Math.floor(total / 60)
    .toString()
    .padStart(2, "0");
  const seconds = (total % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

/** Value shown in a timeline row: score plus how many candidates survived. */
export function eventValue(event: NodeEvent): string {
  if (event.kind === "action") return "";
  if (!event.candidates) return "";
  return `${event.score.toFixed(3)} · ${event.filtered}/${event.candidates}`;
}

export function eventLabel(event: NodeEvent): string {
  return event.focus || event.node;
}
