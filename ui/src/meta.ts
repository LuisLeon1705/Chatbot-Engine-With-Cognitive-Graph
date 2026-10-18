// Labels, colours and presentation helpers.
import type { EdgeKind, NodeKind } from "./api";
import { t } from "./i18n";

type KindGroup = "persona" | "memory" | "world" | "root" | "shared";

const KINDS: { kind: NodeKind; color: string; group: KindGroup }[] = [
  { kind: "personality_root", color: "#f1f5f9", group: "root" },
  { kind: "trait", color: "#fbbf24", group: "persona" },
  { kind: "emotion", color: "#f87171", group: "persona" },
  { kind: "value", color: "#c084fc", group: "persona" },
  { kind: "goal", color: "#4ade80", group: "persona" },
  { kind: "episodic_memory", color: "#60a5fa", group: "memory" },
  { kind: "semantic_fact", color: "#22d3ee", group: "memory" },
  { kind: "entity", color: "#94a3b8", group: "memory" },
  { kind: "place", color: "#2dd4bf", group: "memory" },
  { kind: "topic", color: "#78716c", group: "memory" },
  { kind: "character", color: "#f472b6", group: "memory" },
  { kind: "world_root", color: "#f1f5f9", group: "root" },
  { kind: "faction", color: "#fb923c", group: "world" },
  { kind: "lore", color: "#a78bfa", group: "world" },
  { kind: "event", color: "#818cf8", group: "world" },
  { kind: "appearance", color: "#e879f9", group: "memory" },
  { kind: "chat_root", color: "#f1f5f9", group: "root" },
  { kind: "concept", color: "#a3e635", group: "shared" },
  { kind: "library_root", color: "#f1f5f9", group: "root" },
];

/** Node kinds with their label in the current language. */
export const nodeKinds = () => KINDS.map((k) => ({ ...k, label: t(`kind.${k.kind}`) }));

/** By u8 code (the order of the Rust enum). */
const BY_CODE: NodeKind[] = [
  "personality_root",
  "trait",
  "emotion",
  "value",
  "goal",
  "episodic_memory",
  "semantic_fact",
  "entity",
  "place",
  "topic",
  "world_root",
  "character",
  "faction",
  "lore",
  "event",
  "appearance",
  "chat_root",
  "concept",
  "library_root",
];

export const kindOf = (code: number): NodeKind => BY_CODE[code] ?? "topic";
export const kindMeta = (k: NodeKind) => {
  const m = KINDS.find((x) => x.kind === k) ?? KINDS[9];
  return { ...m, label: t(`kind.${m.kind}`) };
};
export const isRootKind = (k: NodeKind) => k === "personality_root" || k === "world_root" || k === "chat_root" || k === "library_root";

const EDGES: { kind: EdgeKind; color: string }[] = [
  { kind: "composes", color: "#fbbf24" },
  { kind: "involves", color: "#60a5fa" },
  { kind: "related_to", color: "#64748b" },
  { kind: "causes", color: "#fb923c" },
  { kind: "precedes", color: "#475569" },
  { kind: "evokes", color: "#f87171" },
  { kind: "knows", color: "#2dd4bf" },
  { kind: "contradicts", color: "#ef4444" },
];
export const edgeKinds = () => EDGES.map((e) => ({ ...e, label: t(`edge.${e.kind}`) }));
const EDGE_BY_CODE: EdgeKind[] = ["composes", "involves", "related_to", "causes", "precedes", "evokes", "knows", "contradicts"];
export const edgeKindOf = (code: number): EdgeKind => EDGE_BY_CODE[code] ?? "related_to";
export const edgeMeta = (k: EdgeKind) => {
  const m = EDGES.find((e) => e.kind === k) ?? EDGES[2];
  return { ...m, label: t(`edge.${m.kind}`) };
};

/** Stable colour per name (avatars without an image). */
export function nameColor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 55% 58%)`;
}

export function initials(name: string): string {
  const parts = name.replace(/[^\p{L}\p{N}' ]/gu, "").split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

/** Short relative time ("3 days", "now"…). */
export function relTime(unix: number): string {
  const d = Math.max(0, Date.now() / 1000 - unix);
  if (d < 60) return t("time.short.now");
  if (d < 3600) return t("time.short.minutes", { n: Math.floor(d / 60) });
  if (d < 86400) return t("time.short.hours", { n: Math.floor(d / 3600) });
  if (d < 86400 * 2) return t("time.short.yesterday");
  if (d < 86400 * 30) return t("time.short.days", { n: Math.floor(d / 86400) });
  if (d < 86400 * 365) return t("time.short.months", { n: Math.floor(d / (86400 * 30)) });
  return t("time.short.years", { n: Math.floor(d / (86400 * 365)) });
}

/** Text pieces: *actions* in italics, **emphasis** in bold. */
export type Segment = { text: string; style: "plain" | "action" | "bold" };

export function segments(text: string): Segment[] {
  const out: Segment[] = [];
  const re = /\*\*([^*]+)\*\*|\*([^*]+)\*/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push({ text: text.slice(last, m.index), style: "plain" });
    out.push(m[1] !== undefined ? { text: m[1], style: "bold" } : { text: m[2], style: "action" });
    last = re.lastIndex;
  }
  if (last < text.length) out.push({ text: text.slice(last), style: "plain" });
  return out;
}
