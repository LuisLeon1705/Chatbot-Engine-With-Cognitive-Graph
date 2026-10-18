import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph3D from "react-force-graph-3d";
import {
  AdditiveBlending,
  BufferGeometry,
  Float32BufferAttribute,
  Group,
  LineBasicMaterial,
  LineSegments,
  Object3D,
  Points,
  PointsMaterial,
} from "three";
import SpriteText from "three-spritetext";
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

type GNode = { id: number; name: string; kind: number; val: number; valence: number; x?: number; y?: number; z?: number };
type GLink = { source: number | GNode; target: number | GNode; kind: number; weight: number; closed?: boolean };

/** Positions remembered between reloads, so the graph does not "jump" while editing. */
const positions = new Map<string, { x: number; y: number; z: number }>();

/** `#rrggbb` + alpha → `rgba(…)` (3d-force-graph turns the alpha into the material's opacity). */
function withAlpha(hex: string, alpha: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/**
 * Builds the deep-space starfield, astronomical constellation wireframes, and celestial
 * microchip astrolabe rings around the 3D neural graph.
 */
function buildConstellationBackdrop(): { group: Group; dispose: () => void } {
  const group = new Group();
  const geometries: BufferGeometry[] = [];
  const materials: (PointsMaterial | LineBasicMaterial)[] = [];

  // Deterministic PRNG so constellations remain stable across renders.
  let seed = 0x6d2b79f5;
  const rand = () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };

  const spherical = (r: number, theta: number, phi: number): [number, number, number] => [
    r * Math.sin(phi) * Math.cos(theta),
    r * Math.cos(phi),
    r * Math.sin(phi) * Math.sin(theta),
  ];

  // 1. Deep-space starfield (1,400 background stars)
  const starCount = 1400;
  const starPos = new Float32Array(starCount * 3);
  const starCol = new Float32Array(starCount * 3);
  const palette: [number, number, number][] = [
    [0.92, 0.96, 1.0],
    [0.65, 0.82, 1.0],
    [0.98, 0.88, 0.68],
    [0.78, 0.84, 0.95],
  ];
  for (let i = 0; i < starCount; i++) {
    const r = 950 + rand() * 1200;
    const theta = rand() * Math.PI * 2;
    const phi = Math.acos(2 * rand() - 1);
    const [x, y, z] = spherical(r, theta, phi);
    starPos[i * 3] = x;
    starPos[i * 3 + 1] = y;
    starPos[i * 3 + 2] = z;
    const c = palette[i % palette.length];
    const dim = 0.45 + rand() * 0.55;
    starCol[i * 3] = c[0] * dim;
    starCol[i * 3 + 1] = c[1] * dim;
    starCol[i * 3 + 2] = c[2] * dim;
  }
  const starGeo = new BufferGeometry();
  starGeo.setAttribute("position", new Float32BufferAttribute(starPos, 3));
  starGeo.setAttribute("color", new Float32BufferAttribute(starCol, 3));
  const starMat = new PointsMaterial({
    size: 2.6,
    vertexColors: true,
    transparent: true,
    opacity: 0.78,
    blending: AdditiveBlending,
    depthWrite: false,
  });
  geometries.push(starGeo);
  materials.push(starMat);
  group.add(new Points(starGeo, starMat));

  // 2. Constellation wireframes + bright alpha/beta anchor stars
  // Local patterns in (dTheta, dPhi) radians, plus edge pairs.
  const patterns: { pts: [number, number][]; edges: [number, number][] }[] = [
    // Cygnus / Northern Cross
    {
      pts: [[0, 0], [0, 0.14], [0, 0.28], [0, -0.15], [-0.16, 0.12], [0.16, 0.12], [0.26, 0.08]],
      edges: [[3, 0], [0, 1], [1, 2], [4, 1], [1, 5], [5, 6]],
    },
    // Cassiopeia W-crown
    {
      pts: [[-0.22, 0.06], [-0.11, -0.05], [0, 0.04], [0.12, -0.06], [0.23, 0.08]],
      edges: [[0, 1], [1, 2], [2, 3], [3, 4]],
    },
    // Orion hourglass + belt
    {
      pts: [[-0.08, 0], [0, 0.02], [0.08, 0.04], [-0.14, 0.18], [0.12, 0.2], [-0.12, -0.18], [0.15, -0.16]],
      edges: [[0, 1], [1, 2], [0, 3], [2, 4], [3, 4], [0, 5], [2, 6], [5, 6]],
    },
    // Ursa Major dipper
    {
      pts: [[-0.24, 0.1], [-0.14, 0.06], [-0.04, 0.01], [0.05, -0.02], [0.18, -0.01], [0.19, -0.12], [0.05, -0.11]],
      edges: [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [5, 6], [6, 3]],
    },
    // Silicon Hex-Bridge constellation
    {
      pts: [[-0.1, 0.1], [0.1, 0.1], [0.16, 0], [0.1, -0.1], [-0.1, -0.1], [-0.16, 0], [0.28, 0.06], [-0.28, -0.06]],
      edges: [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [5, 0], [2, 6], [5, 7]],
    },
    // Lyra / Harp parallelogram
    {
      pts: [[0, 0.16], [-0.06, 0.06], [0.06, 0.04], [-0.08, -0.08], [0.04, -0.1]],
      edges: [[0, 1], [0, 2], [1, 2], [1, 3], [2, 4], [3, 4]],
    },
  ];

  const cLineVerts: number[] = [];
  const cStarVerts: number[] = [];
  const constellationCenters: [number, number][] = [
    [0.4, 1.1],
    [1.35, 0.85],
    [2.3, 1.3],
    [3.2, 0.9],
    [4.15, 1.25],
    [5.1, 0.8],
    [5.85, 1.4],
    [0.9, 2.15],
    [1.95, 2.25],
    [2.95, 2.05],
    [4.0, 2.2],
    [5.05, 2.1],
    [1.6, 0.45],
    [4.6, 2.65],
  ];

  constellationCenters.forEach(([baseTheta, basePhi], idx) => {
    const pat = patterns[idx % patterns.length];
    const radius = 1080 + (idx % 3) * 110;
    const rot = (idx * 0.7) % (Math.PI * 2);
    const cosR = Math.cos(rot);
    const sinR = Math.sin(rot);

    const worldPts = pat.pts.map(([dt, dp]) => {
      const rdt = dt * cosR - dp * sinR;
      const rdp = dt * sinR + dp * cosR;
      return spherical(radius, baseTheta + rdt, Math.max(0.2, Math.min(Math.PI - 0.2, basePhi + rdp)));
    });

    for (const [x, y, z] of worldPts) {
      cStarVerts.push(x, y, z);
    }
    for (const [a, b] of pat.edges) {
      const pa = worldPts[a];
      const pb = worldPts[b];
      cLineVerts.push(pa[0], pa[1], pa[2], pb[0], pb[1], pb[2]);
    }
  });

  const cLineGeo = new BufferGeometry();
  cLineGeo.setAttribute("position", new Float32BufferAttribute(cLineVerts, 3));
  const cLineMat = new LineBasicMaterial({
    color: 0x60a5fa,
    transparent: true,
    opacity: 0.22,
    blending: AdditiveBlending,
    depthWrite: false,
  });
  geometries.push(cLineGeo);
  materials.push(cLineMat);
  group.add(new LineSegments(cLineGeo, cLineMat));

  const cStarGeo = new BufferGeometry();
  cStarGeo.setAttribute("position", new Float32BufferAttribute(cStarVerts, 3));
  const cStarMat = new PointsMaterial({
    color: 0xe5a93c,
    size: 5.2,
    transparent: true,
    opacity: 0.85,
    blending: AdditiveBlending,
    depthWrite: false,
  });
  geometries.push(cStarGeo);
  materials.push(cStarMat);
  group.add(new Points(cStarGeo, cStarMat));

  // 3. Subtle Celestial Astrolabe / Circuit Orbit Rings
  const ringVerts: number[] = [];
  const addRing = (radius: number, tiltX: number, tiltZ: number, segments: number) => {
    const cx = Math.cos(tiltX);
    const sx = Math.sin(tiltX);
    const cz = Math.cos(tiltZ);
    const sz = Math.sin(tiltZ);
    const pt = (a: number): [number, number, number] => {
      const x0 = radius * Math.cos(a);
      const z0 = radius * Math.sin(a);
      const y1 = -z0 * sx;
      const z1 = z0 * cx;
      const x2 = x0 * cz - y1 * sz;
      const y2 = x0 * sz + y1 * cz;
      return [x2, y2, z1];
    };
    for (let i = 0; i < segments; i++) {
      if (i % 2 === 1) continue; // dashed technical orbital ring
      const a0 = (i / segments) * Math.PI * 2;
      const a1 = ((i + 1) / segments) * Math.PI * 2;
      const p0 = pt(a0);
      const p1 = pt(a1);
      ringVerts.push(p0[0], p0[1], p0[2], p1[0], p1[1], p1[2]);
    }
  };
  addRing(920, 0.38, 0.22, 180);
  addRing(1220, -0.52, -0.31, 220);

  const ringGeo = new BufferGeometry();
  ringGeo.setAttribute("position", new Float32BufferAttribute(ringVerts, 3));
  const ringMat = new LineBasicMaterial({
    color: 0xe5a93c,
    transparent: true,
    opacity: 0.11,
    blending: AdditiveBlending,
    depthWrite: false,
  });
  geometries.push(ringGeo);
  materials.push(ringMat);
  group.add(new LineSegments(ringGeo, ringMat));

  return {
    group,
    dispose: () => {
      geometries.forEach((g) => g.dispose());
      materials.forEach((m) => m.dispose());
    },
  };
}

/**
 * The memory graph in 3D: depth spreads the nodes out inside a celestial constellation
 * and microchip astrolabe sphere.
 */
export default function GraphCanvas3D({ data, selected, visible, heat, onSelect, showWeak }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const fg = useRef<any>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [hover, setHover] = useState<number | null>(null);
  /** Graphs already framed: early (while spreading) and once more when the layout settles. */
  const fitted = useRef(new Set<string>());
  const settled = useRef(new Set<string>());
  const key = data.name + data.scope;

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Inject constellation backdrop into the Three.js scene and clamp DPR.
  useEffect(() => {
    const f = fg.current;
    if (!f) return;
    try {
      const renderer = f.renderer?.();
      if (renderer?.setPixelRatio) {
        renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
      }
    } catch {
      // Renderer not ready yet
    }
    const scene = f.scene?.();
    if (!scene) return;
    const { group, dispose } = buildConstellationBackdrop();
    scene.add(group);

    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    let raf = 0;
    const animate = () => {
      if (!reducedMotion) {
        group.rotation.y += 0.00022;
        group.rotation.x += 0.00005;
      }
      raf = requestAnimationFrame(animate);
    };
    raf = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(raf);
      scene.remove(group);
      dispose();
    };
  }, []);

  const graphData = useMemo(() => {
    const nodes: GNode[] = data.view.nodes.map((n) => {
      const p = positions.get(`${key}:${n.id}`);
      return { ...n, ...(p ?? {}) };
    });
    const links: GLink[] = data.view.links.map((l) => ({ ...l }));
    return { nodes, links };
  }, [data, key]);

  useEffect(() => {
    const f = fg.current;
    if (!f) return;
    // Strong links cluster, weak ones barely pull, cores repel each other (graphPhysics.ts).
    applyForces(f, data.view.links, "3d");
    if (fitted.current.has(key)) return;
    let tries = 0;
    const timer = setInterval(() => {
      const xs = graphData.nodes.filter((n) => n.x != null);
      const extent = xs.length
        ? Math.max(...xs.map((n) => Math.max(Math.abs(n.x!), Math.abs(n.y ?? 0), Math.abs(n.z ?? 0))))
        : 0;
      if (extent > 40 || ++tries > 20) {
        clearInterval(timer);
        fitted.current.add(key);
        fg.current?.zoomToFit(500, 30);
      }
    }, 150);
    return () => clearInterval(timer);
  }, [graphData, key]); // eslint-disable-line react-hooks/exhaustive-deps

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
  const dimmed = (id: number) => !visible(id) || (selected != null && id !== selected && !neighbors.has(id));

  // Floating labels. Rebuilt only when the selection, the filters or the heat change.
  const labelFor = useMemo(() => {
    const rootId = data.view.root;
    return (n: GNode) => {
      const h = heat[String(n.id)] ?? 0;
      const important = n.id === rootId || n.val >= 4.2 || h > 0.2;
      const show = n.id === selected || neighbors.has(n.id) || (selected == null && important);
      if (!show || !visible(n.id)) return new Object3D();
      const text = n.name.length > 30 ? n.name.slice(0, 29) + "…" : n.name;
      const sprite = new SpriteText(text, n.id === selected ? 6.8 : 5.2, n.id === selected ? "#ffffff" : h > 0 ? "#e5a93c" : "#d8e2ef");
      sprite.backgroundColor = "rgba(6, 8, 13, 0.82)";
      sprite.padding = 1.4;
      sprite.borderRadius = 2;
      sprite.fontWeight = n.id === selected ? "600" : "500";
      sprite.position.y = 4 * Math.cbrt(n.val) + 7;
      return sprite;
    };
  }, [data, selected, neighbors, heat, visible]);

  function focus(n: GNode) {
    const f = fg.current;
    if (!f || n.x == null) return;
    const dist = 170;
    const r = Math.hypot(n.x, n.y ?? 0, n.z ?? 0) || 1;
    const k = 1 + dist / r;
    f.cameraPosition({ x: n.x * k, y: (n.y ?? 0) * k, z: (n.z ?? 0) * k }, { x: n.x, y: n.y, z: n.z }, 900);
  }

  return (
    <div ref={box} className="absolute inset-0">
      <ForceGraph3D
        ref={fg}
        width={size.w}
        height={size.h}
        graphData={graphData}
        backgroundColor="#06080d"
        controlType="orbit"
        showNavInfo={false}
        nodeId="id"
        warmupTicks={120}
        cooldownTicks={80}
        onEngineStop={() => {
          graphData.nodes.forEach((n) => n.x != null && positions.set(`${key}:${n.id}`, { x: n.x, y: n.y!, z: n.z! }));
          if (!settled.current.has(key)) {
            settled.current.add(key);
            fg.current?.zoomToFit(700, 30);
          }
        }}
        onNodeClick={(n: GNode) => {
          onSelect(n.id);
          focus(n);
        }}
        onBackgroundClick={() => onSelect(null)}
        onNodeHover={(n: GNode | null) => setHover(n?.id ?? null)}
        onNodeDragEnd={(n: GNode) => positions.set(`${key}:${n.id}`, { x: n.x!, y: n.y!, z: n.z! })}
        nodeLabel={(n: GNode) =>
          `<div style="font:11.5px 'JetBrains Mono',Consolas,monospace;padding:4px 8px;background:rgba(10,13,20,0.92);border:1px solid rgba(229,169,60,0.45);border-radius:4px;color:#e8ecf2;box-shadow:0 4px 16px rgba(0,0,0,0.6)">#${n.id} ${n.name.replace(/</g, "&lt;")}<span style="color:#9ba6b8"> · ${kindMeta(kindOf(n.kind)).label}</span></div>`
        }
        nodeRelSize={4}
        nodeVal={(n: GNode) => n.val}
        nodeResolution={16}
        nodeOpacity={0.95}
        nodeColor={(n: GNode) => {
          const c = kindMeta(kindOf(n.kind)).color;
          if (n.id === selected) return "#ffffff";
          return dimmed(n.id) ? withAlpha(c, 0.12) : (heat[String(n.id)] ?? 0) > 0 ? "#e5a93c" : c;
        }}
        nodeThreeObjectExtend
        nodeThreeObject={labelFor}
        linkColor={(l: GLink) => {
          const a = idOf(l.source);
          const b = idOf(l.target);
          const c = l.closed ? PAST_COLOR : edgeMeta(edgeKindOf(l.kind)).color;
          const touched = a === selected || b === selected || a === hover || b === hover;
          if (touched) return c;
          if (!visible(a) || !visible(b) || selected != null) return withAlpha("#ffffff", 0.04);
          if (faintLink(l, false, showWeak)) return withAlpha(c, 0.05);
          const cls = linkClassOf(l);
          return withAlpha(c, cls === "structural" ? 0.78 : cls === "medium" ? 0.45 : 0.3);
        }}
        linkWidth={(l: GLink) => (selected != null && (idOf(l.source) === selected || idOf(l.target) === selected) ? 1.1 : 0)}
        linkDirectionalParticles={(l: GLink) => (selected != null && (idOf(l.source) === selected || idOf(l.target) === selected) ? 3 : 0)}
        linkDirectionalParticleWidth={1.8}
        linkDirectionalParticleSpeed={0.006}
      />
    </div>
  );
}
