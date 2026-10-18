// Physics of the graph views (2D and 3D). It separates the *visual* topology from the
// memory's real data: every edge still exists for the model, but only the strong,
// structural ones pull nodes together. Weak associative edges barely pull, so the
// graph stops collapsing into one dense ball as it grows.
import { forceX, forceY, forceZ } from "d3-force-3d";
import type { EdgeKind } from "./api";
import { edgeKindOf, kindOf } from "./meta";

/** How an edge behaves on screen. */
export type LinkClass = "structural" | "medium" | "weak";

/**
 * - **structural**: personality-tree branches and aspects (`composes`) or any edge with
 *   weight ≥ 0.80. They form tight clusters around their core.
 * - **weak**: generic associations (`related_to`), temporal chains (`precedes`),
 *   inhibitions (`contradicts`) or any edge with weight < 0.70. They hardly pull.
 * - **medium**: the rest (`involves`, `evokes`, `causes`, `knows` between 0.70 and 0.80).
 */
export function linkClass(kind: EdgeKind, weight: number): LinkClass {
  if (kind === "composes" || weight >= 0.8) return "structural";
  if (kind === "related_to" || kind === "precedes" || kind === "contradicts" || weight < 0.7) return "weak";
  return "medium";
}

/** A closed link (the past) barely pulls, whatever its kind and weight. */
export const linkClassOf = (l: { kind: number; weight: number; closed?: boolean }) => (l.closed ? "weak" : linkClass(edgeKindOf(l.kind), l.weight));

/** Colour of links that no longer hold (drawn dashed in 2D). */
export const PAST_COLOR = "#f59e0b";

type Mode = "2d" | "3d";

/** Pull (scaled by degree, see `applyForces`) and resting length per class. */
const LINK = {
  "2d": { structural: { strength: 0.9, distance: 28 }, medium: { strength: 0.25, distance: 70 }, weak: { strength: 0.012, distance: 150 } },
  "3d": { structural: { strength: 0.9, distance: 32 }, medium: { strength: 0.25, distance: 80 }, weak: { strength: 0.012, distance: 170 } },
} as const;

/** Repulsion: cores push the hardest so each one keeps its own "galaxy". */
const CHARGE = {
  "2d": { root: -900, core: -420, hub: -260, node: -110, distanceMax: 700 },
  "3d": { root: -650, core: -320, hub: -200, node: -90, distanceMax: 550 },
} as const;

/** Gentle pull of every node towards the centre: nodes held only by weak links settle
 * on an outer orbit instead of drifting away (and shrinking the framed view). */
const GRAVITY = { "2d": 0.05, "3d": 0.04 } as const;

const ROOTS = new Set(["personality_root", "world_root", "chat_root", "library_root"]);

type SimNode = { id: number; kind: number };
type SimLink = { source: number | SimNode; target: number | SimNode; kind: number; weight: number; closed?: boolean };

const idOf = (x: number | SimNode) => (typeof x === "object" ? x.id : x);

/**
 * Configures the link and charge forces of a `ForceGraph2D` / `ForceGraph3D` (through its
 * `ref`) and reheats the simulation so the new layout settles.
 */
export function applyForces(fg: any, links: { source: number; target: number }[], mode: Mode) {
  if (!fg) return;
  const degree = new Map<number, number>();
  for (const l of links) {
    degree.set(l.source, (degree.get(l.source) ?? 0) + 1);
    degree.set(l.target, (degree.get(l.target) ?? 0) + 1);
  }
  const deg = (id: number) => degree.get(id) ?? 0;

  const link = fg.d3Force("link");
  if (link) {
    const cfg = LINK[mode];
    link
      // Around a big hub the resting length grows with its degree: a core with 30
      // branches opens into a wide ring instead of a tight ball.
      .distance((l: SimLink) => cfg[linkClassOf(l)].distance * (1 + 0.12 * Math.sqrt(Math.max(deg(idOf(l.source)), deg(idOf(l.target))))))
      // Dividing by the smaller degree (as d3 does by default) keeps hubs from shaking
      // the layout; the square root keeps structural branches tight around them.
      .strength((l: SimLink) => cfg[linkClassOf(l)].strength / Math.sqrt(Math.max(1, Math.min(deg(idOf(l.source)), deg(idOf(l.target))))));
  }

  const charge = fg.d3Force("charge");
  if (charge) {
    const cfg = CHARGE[mode];
    charge
      .strength((n: SimNode) => {
        const kind = kindOf(n.kind);
        if (ROOTS.has(kind)) return cfg.root;
        if (kind === "character" || kind === "faction") return cfg.core;
        return deg(n.id) >= 6 ? cfg.hub : cfg.node;
      })
      // Far-away clusters stop pushing each other: galaxies stay apart without flying off.
      .distanceMax(cfg.distanceMax);
  }

  const g = GRAVITY[mode];
  fg.d3Force("x", forceX(0).strength(g));
  fg.d3Force("y", forceY(0).strength(g));
  if (mode === "3d") fg.d3Force("z", forceZ(0).strength(g));
  fg.d3ReheatSimulation?.();
}

/**
 * Whether an edge is drawn faint: weak edges, unless the user asked to see them or one
 * of their ends is hovered or selected.
 */
export function faintLink(l: { kind: number; weight: number; closed?: boolean }, touched: boolean, showWeak: boolean): boolean {
  return !touched && !showWeak && linkClassOf(l) === "weak";
}
