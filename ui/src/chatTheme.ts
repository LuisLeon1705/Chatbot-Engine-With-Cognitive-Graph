// How a world's chats look: CSS variables built from its style (style.toml). The chat
// views read only these variables, so a world changes its accent, light or dark mode,
// typeface and texture without touching any component.
import type { CSSProperties } from "react";
import type { WorldStyle } from "./api";

export const DEFAULT_STYLE: WorldStyle = { accent: "#e5a93c", mode: "dark", font: "serif", texture: "none" };

const PALETTES = {
  dark: { bg: "#111113", surface: "#17171a", ink: "#e7e4dd", ink2: "#b3aea5", ink3: "#7a766e", rule: "rgba(231, 228, 221, 0.09)" },
  light: { bg: "#f5f2ea", surface: "#ebe6da", ink: "#1d1b18", ink2: "#4f4a43", ink3: "#8a8378", rule: "rgba(29, 27, 24, 0.12)" },
} as const;

const FONTS = {
  serif: `Charter, "Bitstream Charter", "Sitka Text", Georgia, "Palatino Linotype", serif`,
  sans: `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`,
} as const;

/** Background layers for each texture (the noise images are defined in styles.css). */
const TEXTURES: Record<WorldStyle["mode"], Record<WorldStyle["texture"], { image: string; size: string }>> = {
  dark: {
    none: { image: "none", size: "auto" },
    grain: { image: "var(--tex-noise-dark)", size: "200px 200px" },
    paper: { image: "var(--tex-fibre), var(--tex-noise-dark)", size: "600px 600px, 200px 200px" },
  },
  light: {
    none: { image: "none", size: "auto" },
    grain: { image: "var(--tex-grain)", size: "240px 240px" },
    paper: { image: "var(--tex-stain), var(--tex-grain)", size: "900px 900px, 240px 240px" },
  },
};

/** Black or white, whichever reads better on `hex`. */
function inkOn(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.4 ? "#141210" : "#ffffff";
}

/** The CSS variables of a world's chat (`--c-*`), plus its texture on the background. */
export function chatTheme(style: WorldStyle | undefined): CSSProperties {
  const s = style ?? DEFAULT_STYLE;
  const p = PALETTES[s.mode] ?? PALETTES.dark;
  const tex = TEXTURES[s.mode]?.[s.texture] ?? TEXTURES.dark.none;
  return {
    "--c-bg": p.bg,
    "--c-surface": p.surface,
    "--c-ink": p.ink,
    "--c-ink-2": p.ink2,
    "--c-ink-3": p.ink3,
    "--c-rule": p.rule,
    "--c-accent": s.accent,
    "--c-accent-ink": inkOn(s.accent),
    "--c-font": FONTS[s.font] ?? FONTS.serif,
    // Roleplay text (RichText): actions in a softer italic, emphasis in full ink.
    "--rt-action": p.ink2,
    "--rt-bold": p.ink,
    colorScheme: s.mode,
    backgroundColor: p.bg,
    backgroundImage: tex.image,
    backgroundSize: tex.size,
    color: p.ink,
  } as CSSProperties;
}
