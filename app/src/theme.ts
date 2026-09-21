/** Material 3 colour roles, generated from one seed with Google's own
    implementation rather than hand-picked values. MWC components read the same
    custom properties, so this is the single place colour is decided. */

import { Hct, SchemeContent, argbFromHex, hexFromArgb } from "@material/material-color-utilities";

export const SEED = "#F5A623";

export type ThemeMode = "system" | "light" | "dark";

const ROLES = [
  "primary",
  "onPrimary",
  "primaryContainer",
  "onPrimaryContainer",
  "secondary",
  "onSecondary",
  "secondaryContainer",
  "onSecondaryContainer",
  "tertiary",
  "error",
  "onError",
  "errorContainer",
  "onErrorContainer",
  "background",
  "onBackground",
  "surface",
  "onSurface",
  "surfaceVariant",
  "onSurfaceVariant",
  "surfaceDim",
  "surfaceBright",
  "surfaceContainerLowest",
  "surfaceContainerLow",
  "surfaceContainer",
  "surfaceContainerHigh",
  "surfaceContainerHighest",
  "outline",
  "outlineVariant",
  "inverseSurface",
  "onInverseSurface",
] as const;

function cssName(role: string): string {
  return `--md-sys-color-${role.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
}

export function prefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyTheme(mode: ThemeMode): "light" | "dark" {
  const dark = mode === "system" ? prefersDark() : mode === "dark";
  const scheme = new SchemeContent(Hct.fromInt(argbFromHex(SEED)), dark, 0);
  const root = document.documentElement;
  for (const role of ROLES) {
    const value = (scheme as unknown as Record<string, number>)[role];
    if (value !== undefined) root.style.setProperty(cssName(role), hexFromArgb(value));
  }
  root.style.setProperty("color-scheme", dark ? "dark" : "light");
  root.dataset.theme = dark ? "dark" : "light";
  return dark ? "dark" : "light";
}
