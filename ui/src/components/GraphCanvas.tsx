import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph2D from "react-force-graph-2d";
import type { GraphDto } from "../api";
import { PAST_COLOR, applyForces, faintLink, linkClassOf } from "../graphPhysics";
import { edgeKindOf, edgeMeta, kindMeta, kindOf } from "../meta";

interface Props {
  data: GraphDto;
  selected: number | null;
  visible: (id: number) => boolean;
  heat: Record<string, number>;
  onSelect: (id: number | null) => void;
  /** Draw weak (associative) links normally instead of faint. */
  showWeak: boolean;
}

type GNode = { id: number; name: string; kind: number; val: number; valence: number; x?: number; y?: number; fx?: number; fy?: number };
type GLink = { source: number | GNode; target: number | GNode; kind: number; weight: number; closed?: boolean };

/** Positions remembered between reloads, so the graph does not "jump" while editing. */
const positions = new Map<string, { x: number; y: number }>();

/** Hex alpha suffix for `#rrggbb` colours. */
const alpha = (a: number) =>
  Math.round(a * 255)
    .toString(16)
    .padStart(2, "0");

export function GraphCanvas({ data, selected, visible, heat, onSelect, showWeak }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const fg = useRef<any>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [hover, setHover] = useState<number | null>(null);
  /** Graphs already framed (they are framed the first time only, not on every edit). */
  const fitted = useRef(new Set<string>());
  const key = data.name + data.scope;

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const graphData = useMemo(() => {
    const nodes: GNode[] = data.view.nodes.map((n) => {
      const p = positions.get(`${key}:${n.id}`);
      return { ...n, ...(p ? { x: p.x, y: p.y } : {}) };
    });
    const links: GLink[] = data.view.links.map((l) => ({ ...l }));
    return { nodes, links };
  }, [data, key]);

  // Strong links cluster, weak ones barely pull, cores repel each other (graphPhysics.ts).
  useEffect(() => {
    applyForces(fg.current, data.view.links, "2d");
  }, [graphData]); // eslint-disable-line react-hooks/exhaustive-deps

  // Neighbours of the selected node (to highlight them).
  const neighbors = useMemo(() => {
    const s = new Set<number>();
    if (selected == null) return s;
    for (const l of data.view.links) {
      if (l.source === selected) s.add(l.target);
      if (l.target === selected) s.add(l.source);
    }
    return s;
  }, [data, selected]);

  const idOf = (x: number | GNode) => (typeof x === "object" ? x.id : x);
  const touches = (l: GLink, id: number | null) => id != null && (idOf(l.source) === id || idOf(l.target) === id);
  const isFaint = (l: GLink) => faintLink(l, touches(l, selected) || touches(l, hover), showWeak);

  return (
    <div ref={box} className="absolute inset-0">
      <ForceGraph2D
        ref={fg}
        width={size.w}
        height={size.h}
        graphData={graphData}
        // Transparent: the board texture (".substrate", fixed to the screen) shows through.
        backgroundColor="rgba(0,0,0,0)"
        nodeId="id"
        cooldownTicks={180}
        onEngineStop={() => {
          graphData.nodes.forEach((n) => n.x != null && positions.set(`${key}:${n.id}`, { x: n.x, y: n.y! }));
          if (!fitted.current.has(key)) {
            fitted.current.add(key);
            // A fit would blow a new, near-empty graph up to fill the screen.
            if (graphData.nodes.length > 3) fg.current?.zoomToFit(500, 60);
            else (fg.current?.centerAt(0, 0, 500), fg.current?.zoom(2.5, 500));
          }
        }}
        onNodeClick={(n: GNode) => onSelect(n.id)}
        onBackgroundClick={() => onSelect(null)}
        onNodeHover={(n: GNode | null) => setHover(n?.id ?? null)}
        onNodeDragEnd={(n: GNode) => positions.set(`${key}:${n.id}`, { x: n.x!, y: n.y! })}
        linkColor={(l: GLink) => {
          const a = idOf(l.source);
          const b = idOf(l.target);
          const c = l.closed ? PAST_COLOR : edgeMeta(edgeKindOf(l.kind)).color;
          if (touches(l, selected) || touches(l, hover)) return c;
          if (!visible(a) || !visible(b) || selected != null) return "#ffffff0a";
          if (isFaint(l)) return c + alpha(0.06);
          const cls = linkClassOf(l);
          return c + alpha(cls === "structural" ? 0.65 : cls === "medium" ? 0.42 : 0.25);
        }}
        linkWidth={(l: GLink) => (isFaint(l) ? 0.4 : linkClassOf(l) === "structural" ? 1.2 + l.weight * 1.4 : 0.5 + l.weight * 1.2)}
        linkDirectionalArrowLength={(l: GLink) => (isFaint(l) ? 0 : 3)}
        linkDirectionalArrowRelPos={0.9}
        linkLineDash={(l: GLink) => (l.closed ? [1.5, 2.5] : l.kind === 7 ? [3, 2] : null)}
        nodeRelSize={4}
        nodeVal={(n: GNode) => n.val}
        nodeCanvasObjectMode={() => "replace"}
        nodeCanvasObject={(n: GNode, ctx: CanvasRenderingContext2D, scale: number) => {
          const kind = kindOf(n.kind);
          const color = kindMeta(kind).color;
          const r = 4 * Math.sqrt(n.val);
          const isSel = n.id === selected;
          const isRoot = n.id === data.view.root;
          const near = neighbors.has(n.id);
          const dim = !visible(n.id) || (selected != null && !isSel && !near);
          const h = heat[String(n.id)] ?? 0;
          ctx.globalAlpha = dim ? 0.16 : 1;

          // Signal activation halo (probe heat)
          if (h > 0) {
            ctx.beginPath();
            ctx.arc(n.x!, n.y!, r + 4 + 10 * h, 0, 2 * Math.PI);
            ctx.fillStyle = `rgba(229, 169, 60, ${0.14 + 0.34 * h})`;
            ctx.fill();
          }

          // Outer microchip pad ring for root, selected, or hovered nodes
          if (isRoot || isSel || n.id === hover) {
            ctx.beginPath();
            ctx.arc(n.x!, n.y!, r + 2.8 / scale, 0, 2 * Math.PI);
            ctx.lineWidth = (isSel ? 1.8 : 1.1) / scale;
            ctx.strokeStyle = isSel ? "#e5a93c" : "rgba(229, 169, 60, 0.55)";
            ctx.stroke();
          }

          // A dark keep-out ring: it lifts the node off the board texture.
          ctx.beginPath();
          ctx.arc(n.x!, n.y!, r + 1.6 / scale, 0, 2 * Math.PI);
          ctx.fillStyle = "rgba(7, 10, 16, 0.92)";
          ctx.fill();

          // Core pad
          ctx.beginPath();
          ctx.arc(n.x!, n.y!, r, 0, 2 * Math.PI);
          ctx.fillStyle = color;
          ctx.fill();
          ctx.lineWidth = 1 / scale;
          ctx.strokeStyle = isSel ? "#ffffff" : "rgba(6, 8, 13, 0.85)";
          ctx.stroke();

          // Zoomed out, only the most important nodes (and the selection) get a label.
          const showLabel = scale > 1.6 || isSel || near || n.id === hover || n.val >= (scale > 0.9 ? 4.2 : 4.7) || h > 0.2;
          if (showLabel) {
            const fontSize = Math.max(10.5 / scale, 2.2);
            ctx.font = `${isSel ? 600 : 500} ${fontSize}px "Inter", "Segoe UI", system-ui, sans-serif`;
            ctx.textAlign = "center";
            ctx.textBaseline = "bottom";
            const label = n.name.length > 28 ? n.name.slice(0, 27) + "…" : n.name;
            const y = n.y! - r - 3 / scale;
            ctx.lineWidth = 3.2 / scale;
            ctx.strokeStyle = "#06080d";
            ctx.strokeText(label, n.x!, y);
            ctx.fillStyle = isSel ? "#ffffff" : h > 0 ? "#fde68a" : "#d0d8e4";
            ctx.fillText(label, n.x!, y);
          }
          ctx.globalAlpha = 1;
        }}
        nodePointerAreaPaint={(n: GNode, color: string, ctx: CanvasRenderingContext2D) => {
          ctx.beginPath();
          ctx.arc(n.x!, n.y!, 4 * Math.sqrt(n.val) + 3, 0, 2 * Math.PI);
          ctx.fillStyle = color;
          ctx.fill();
        }}
      />
    </div>
  );
}
