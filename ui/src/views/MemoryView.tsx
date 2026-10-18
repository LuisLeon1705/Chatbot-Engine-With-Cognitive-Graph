import { Suspense, lazy, useCallback, useEffect, useMemo, useState } from "react";
import { api, errText, type ChatSummary, type GraphDto, type NodeKind, type WorldDto } from "../api";
import { AppearancePanel } from "../components/AppearancePanel";
import { Avatar, AvatarStack } from "../components/Avatar";
import { GraphCanvas } from "../components/GraphCanvas";
import { Icon } from "../components/Icon";
import { NodeCreator, NodeEditor } from "../components/NodeEditor";
import { StylePanel } from "../components/StylePanel";
import { TimelinePanel } from "../components/TimelinePanel";
import { WorldPicker } from "../components/WorldPicker";
import { t } from "../i18n";
import { kindMeta, nodeKinds, relTime } from "../meta";
import type { Toast } from "../App";

// three.js is heavy: the 3D view only loads when it is first used.
const GraphCanvas3D = lazy(() => import("../components/GraphCanvas3D"));

interface Props {
  worlds: WorldDto[];
  world: WorldDto | null;
  setWorldKey: (k: string) => void;
  toast: (t: string, tone?: Toast["tone"]) => void;
  /** Changes when a curator modifies some memory. */
  tick: number;
  onWorldsChanged: () => void;
  /** A graph to open ("chat:<id>"…); `n` changes on every request. */
  requestedGraph: { key: string; n: number } | null;
}

export function MemoryView({ worlds, world, setWorldKey, toast, tick, onWorldsChanged, requestedGraph }: Props) {
  const [graphKey, setGraphKey] = useState<string>(() => new URLSearchParams(location.hash.slice(1)).get("graph") || "world");
  const [data, setData] = useState<GraphDto | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [creating, setCreating] = useState(false);
  const [tab, setTab] = useState<"graph" | "list" | "appearance" | "timeline" | "style">("graph");
  /** Show the graph as it is at this story time (`null` = everything). */
  const [viewAt, setViewAt] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [hidden, setHidden] = useState<Set<NodeKind>>(new Set());
  const [probe, setProbe] = useState("");
  const [heat, setHeat] = useState<Record<string, number>>({});
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [is3d, setIs3d] = useState(() => {
    try {
      return localStorage.getItem("chats.graph3d") === "1";
    } catch {
      return false;
    }
  });
  const [showWeak, setShowWeak] = useState(() => {
    try {
      return localStorage.getItem("chats.weakLinks") === "1";
    } catch {
      return false;
    }
  });
  const isLibrary = graphKey.startsWith("lib:");
  const isCharacter = graphKey !== "world" && !graphKey.startsWith("chat:") && !isLibrary;
  const isChat = graphKey.startsWith("chat:");
  /** A Collab's memory view: no definitions or private memories of its own. */
  const isCollab = !!world?.links;
  const timeline = world?.timeline ?? [];
  const browsing = tab === "graph" || tab === "list";

  function toggleWeak() {
    setShowWeak((on) => {
      try {
        localStorage.setItem("chats.weakLinks", on ? "0" : "1");
      } catch {
        // Only a convenience: it is fine if it is not remembered.
      }
      return !on;
    });
  }

  function set3d(on: boolean) {
    setIs3d(on);
    try {
      localStorage.setItem("chats.graph3d", on ? "1" : "0");
    } catch {
      // Only a convenience: it is fine if it is not remembered.
    }
  }

  // Opening a graph from elsewhere (e.g. a conversation's "Chat memory" button).
  useEffect(() => {
    if (requestedGraph) {
      setGraphKey(requestedGraph.key);
      setTab((x) => (x === "appearance" || x === "timeline" || x === "style" ? "graph" : x));
    }
  }, [requestedGraph?.n]); // eslint-disable-line react-hooks/exhaustive-deps

  // The world's chats, for the "Chat memories" list.
  useEffect(() => {
    if (!world) return;
    api.listChats(world.key).then(setChats).catch(() => setChats([]));
  }, [world?.key, tick]); // eslint-disable-line react-hooks/exhaustive-deps

  const load = useCallback(
    async (select?: number | null) => {
      if (!world) return;
      try {
        const g = await api.graph(world.key, graphKey);
        setData(g);
        if (select !== undefined) setSelected(select);
      } catch (e) {
        toast(errText(e), "error");
      }
    },
    [world, graphKey, toast],
  );

  useEffect(() => {
    setSelected(null);
    setCreating(false);
    setHeat({});
    if (!isCharacter) setTab((x) => (x === "appearance" ? "graph" : x));
    if (isChat) setTab((x) => (x === "timeline" ? "graph" : x));
    if (graphKey !== "world") setTab((x) => (x === "style" ? "graph" : x));
    const pre = new URLSearchParams(location.hash.slice(1)).get("node");
    load(pre ? Number(pre) : undefined);
  }, [world?.key, graphKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // A curator changed something: reload (keeping the selection).
  useEffect(() => {
    if (tick > 0) load();
  }, [tick]); // eslint-disable-line react-hooks/exhaustive-deps

  const q = search.trim().toLowerCase();
  const visibleSet = useMemo(() => {
    const s = new Set<number>();
    for (const n of data?.nodes ?? []) {
      if (hidden.has(n.kind)) continue;
      if (viewAt !== null && typeof n.since === "number" && n.since > viewAt) continue;
      if (q && !n.label.toLowerCase().includes(q) && !n.content.toLowerCase().includes(q)) continue;
      s.add(n.id);
    }
    return s;
  }, [data, hidden, q, viewAt]);

  const presentKinds = nodeKinds().filter((k) => data?.nodes.some((n) => n.kind === k.kind));
  const node = data?.nodes.find((n) => n.id === selected) ?? null;
  const root = data?.nodes.find((n) => n.isRoot);

  async function runProbe() {
    if (!world || !probe.trim()) {
      setHeat({});
      return;
    }
    try {
      const h = await api.probe(world.key, graphKey, probe);
      const max = Math.max(0.0001, ...Object.values(h));
      setHeat(Object.fromEntries(Object.entries(h).map(([k, v]) => [k, v / max])));
    } catch (e) {
      toast(errText(e), "error");
    }
  }

  const editorProps = {
    world: world?.key ?? "",
    graph: graphKey,
    scope: data?.scope ?? ("character" as const),
    nodes: data?.nodes ?? [],
    timeline,
    toast,
    onChanged: (sel?: number | null) => {
      setCreating(false);
      load(sel);
      onWorldsChanged();
    },
    onSelect: (id: number) => {
      setCreating(false);
      setSelected(id);
    },
    onClose: () => {
      setSelected(null);
      setCreating(false);
    },
  };

  return (
    <div className="pcb-surface flex h-full">
      {/* Memory Banks Sidebar (Silicon / IC Bus) */}
      <aside className="pcb-sidebar flex w-[275px] shrink-0 flex-col">
        <div className="border-b border-white/10 p-3">
          <WorldPicker worlds={worlds} value={world?.key ?? ""} onChange={setWorldKey} />
        </div>
        <div className="min-h-0 flex-1 space-y-1 overflow-y-auto p-2.5">
          {!isCollab && (
            <div className="silk flex items-center gap-2 px-2 pt-1 pb-1">
              <span className="text-[#a3e635]">U0</span>
              {t("ui.memory.definitions")}
            </div>
          )}
          {world?.definitions.map((d) => (
            <GraphButton
              key={d.key}
              active={graphKey === d.key}
              onClick={() => (setGraphKey(d.key), d.nodes === 0 && setTimeout(onWorldsChanged, 400))}
              title={d.name}
              subtitle={d.nodes > 0 ? t("ui.memory.definitions_sub", { n: d.nodes }) : t("ui.memory.definitions_empty")}
            >
              <div className="flex h-8 w-8 items-center justify-center rounded-md border border-[#a3e635]/35 bg-[#a3e635]/10 text-[#a3e635]">
                <Icon name="list" size={15} />
              </div>
            </GraphButton>
          ))}

          <div className={`silk flex items-center gap-2 px-2 pb-1 ${isCollab ? "pt-1" : "pt-4"}`}>
            <span className="text-[#38bdf8]">U1</span>
            {t("ui.memory.shared")}
          </div>
          <GraphButton
            active={graphKey === "world"}
            onClick={() => setGraphKey("world")}
            title={world?.name ?? t("ui.memory.world")}
            subtitle={isCollab ? t("ui.memory.collab_sub", { n: world?.nodes ?? 0 }) : t("ui.memory.world_sub", { n: world?.nodes ?? 0 })}
          >
            <div className="flex h-8 w-8 items-center justify-center rounded-md border border-[#38bdf8]/35 bg-[#38bdf8]/10 text-[#38bdf8]">
              <Icon name="graph" size={16} />
            </div>
          </GraphButton>

          {!isCollab && (
            <div className="silk flex items-center gap-2 px-2 pt-4 pb-1">
              <span className="text-accent">U2</span>
              {t("ui.memory.private")}
            </div>
          )}
          {!isCollab && world?.characters.map((c) => (
            <GraphButton
              key={c.id}
              active={graphKey === c.id}
              onClick={() => setGraphKey(c.id)}
              title={c.name}
              subtitle={t("ui.memory.nodes", { n: c.nodes })}
            >
              <Avatar name={c.name} src={c.avatar} size={32} />
            </GraphButton>
          ))}

          <div className="silk flex items-center gap-2 px-2 pt-4 pb-1">
            <span className="text-emerald-400">U3</span>
            {t("ui.memory.chats")}
          </div>
          {chats.length === 0 && <p className="px-2 font-mono text-[11px] text-ink-3">{isCollab ? t("ui.collab.no_chats") : t("ui.memory.no_chats")}</p>}
          {chats.map((c) => {
            const people = c.participants
              .map((id) => world?.characters.find((x) => x.id === id))
              .filter((x) => !!x);
            return (
              <div key={c.id} className="space-y-0.5 pt-0.5">
                <GraphButton
                  active={graphKey === `chat:${c.id}`}
                  onClick={() => setGraphKey(`chat:${c.id}`)}
                  title={c.title}
                  subtitle={people.map((p) => p.name).join(", ")}
                >
                  <AvatarStack people={people.map((x) => ({ name: x.name, avatar: x.avatar }))} size={32} />
                </GraphButton>
                {people.map((p) => (
                  <div key={p.id} className="pcb-trace-branch">
                    <GraphButton
                      active={graphKey === `chat:${c.id}:${p.id}`}
                      onClick={() => setGraphKey(`chat:${c.id}:${p.id}`)}
                      title={p.name}
                      subtitle={t("ui.memory.scope_private")}
                    >
                      <Avatar name={p.name} src={p.avatar} size={24} />
                    </GraphButton>
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        {/* Hardware Telemetry Toolbar */}
        <div className="flex flex-wrap items-center gap-2.5 border-b border-white/10 bg-panel/80 px-4 py-2.5 backdrop-blur-md">
          <div className="mr-2">
            <div className="flex items-center gap-2">
              <span className="h-2 w-2 rounded-[2px] bg-accent shadow-[0_0_8px_rgba(229,169,60,0.7)]" />
              <span className="text-[14.5px] font-semibold tracking-tight">{data?.name ?? "…"}</span>
            </div>
            <div className="font-mono text-[11px] text-ink-3 tabular-nums">
              {data
                ? t("ui.memory.stats", {
                    nodes: data.nodes.length,
                    links: data.view.links.length,
                    scope:
                      data.scope === "world"
                        ? t("ui.memory.scope_world")
                        : data.scope === "chat"
                          ? t("ui.memory.scope_chat")
                          : data.scope === "library"
                            ? t("ui.memory.scope_library")
                            : data.scope === "collab"
                              ? t("ui.memory.scope_collab")
                              : t("ui.memory.scope_private"),
                  })
                : ""}
            </div>
            {data?.scope === "chat" && <div className="max-w-md text-[11px] text-accent/85">{t("ui.memory.chat_help")}</div>}
            {data?.scope === "library" && <div className="max-w-md text-[11px] text-[#a3e635]/85">{t("ui.memory.library_help")}</div>}
            {data?.scope === "collab" && <div className="max-w-md text-[11px] text-[#38bdf8]/85">{t("ui.memory.collab_help")}</div>}
          </div>

          <div className="dip">
            {(["graph", "list", "appearance", "timeline", "style"] as const)
              .filter((x) => (x !== "appearance" || isCharacter) && (x !== "timeline" || (!isChat && !isLibrary && !isCollab)) && (x !== "style" || graphKey === "world"))
              .map((x) => (
                <button key={x} onClick={() => setTab(x)} className={tab === x ? "on" : ""}>
                  {t(`ui.memory.tab_${x}`)}
                </button>
              ))}
          </div>

          {tab === "graph" && (
            <div className="dip" title={t("ui.memory.view_help")}>
              {[false, true].map((three) => (
                <button key={String(three)} onClick={() => set3d(three)} className={is3d === three ? "on" : ""}>
                  {three ? "3D" : "2D"}
                </button>
              ))}
            </div>
          )}

          {tab === "graph" && (
            <div className="dip" title={t("ui.memory.weak_links_help")}>
              <button onClick={toggleWeak} className={showWeak ? "on" : ""}>
                {t("ui.memory.weak_links")}
              </button>
            </div>
          )}

          {browsing && timeline.length > 0 && (
            <select
              className="field w-auto py-1.5 font-mono text-[11.5px]"
              value={viewAt ?? ""}
              onChange={(e) => setViewAt(e.target.value === "" ? null : Number(e.target.value))}
              title={t("ui.timeline.view_at_help")}
            >
              <option value="">{t("ui.timeline.all_times")}</option>
              {timeline.map((m) => (
                <option key={m.id} value={m.at}>
                  {t("ui.timeline.view_at", { name: m.name })}
                </option>
              ))}
            </select>
          )}

          {browsing && (
            <>
              <div className="relative w-52">
                <Icon name="search" size={14} className="absolute top-2.5 left-2.5 text-ink-3" />
                <input className="field pl-8" placeholder={t("ui.memory.search")} value={search} onChange={(e) => setSearch(e.target.value)} />
              </div>
              <div className="relative w-60">
                <Icon name="bolt" size={14} className="absolute top-2.5 left-2.5 text-accent" />
                <input
                  className="field pl-8"
                  placeholder={t("ui.memory.probe")}
                  value={probe}
                  onChange={(e) => setProbe(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && runProbe()}
                  title={t("ui.memory.probe_help")}
                />
              </div>
              {Object.keys(heat).length > 0 && (
                <button className="btn-ghost" onClick={() => (setHeat({}), setProbe(""))}>
                  {t("common.clear")}
                </button>
              )}
              <button className="btn-primary ml-auto" onClick={() => (setSelected(null), setCreating(true))} disabled={!data}>
                <Icon name="plus" size={14} /> {t("ui.memory.new_node")}
              </button>
            </>
          )}
        </div>

        {/* Filters by kind (IC Register Pins) */}
        <div className={`flex flex-wrap gap-2 border-b border-white/10 bg-panel/40 px-4 py-2 ${browsing ? "" : "hidden"}`}>
          {presentKinds.map((k) => {
            const off = hidden.has(k.kind);
            const count = data?.nodes.filter((n) => n.kind === k.kind).length ?? 0;
            return (
              <button
                key={k.kind}
                onClick={() =>
                  setHidden((h) => {
                    const n = new Set(h);
                    if (n.has(k.kind)) n.delete(k.kind);
                    else n.add(k.kind);
                    return n;
                  })
                }
                className={`smd ${off ? "off" : ""}`}
              >
                <span className="led" style={{ color: k.color }} />
                <span>{k.label}</span>
                <span className="text-[#6f7c91]">{count}</span>
              </button>
            );
          })}
        </div>

        <div className={`relative min-h-0 flex-1 ${tab === "graph" && !is3d ? "substrate" : ""}`}>
          {data && tab === "graph" && !is3d && (
            <GraphCanvas
              data={data}
              selected={selected}
              visible={(id) => visibleSet.has(id)}
              heat={heat}
              showWeak={showWeak}
              onSelect={(id) => (setCreating(false), setSelected(id))}
            />
          )}
          {data && tab === "graph" && is3d && (
            <Suspense fallback={<div className="flex h-full items-center justify-center font-mono text-ink-3">{t("common.loading")}</div>}>
              <GraphCanvas3D
                data={data}
                selected={selected}
                visible={(id) => visibleSet.has(id)}
                heat={heat}
                showWeak={showWeak}
                onSelect={(id) => (setCreating(false), setSelected(id))}
              />
            </Suspense>
          )}
          {data && tab === "appearance" && isCharacter && world && (
            <AppearancePanel world={world.key} character={graphKey} name={data.name} toast={toast} onChanged={() => (load(), onWorldsChanged())} />
          )}
          {data && tab === "style" && world && graphKey === "world" && <StylePanel world={world} toast={toast} onChanged={onWorldsChanged} />}
          {data && tab === "timeline" && world && !isChat && (
            <TimelinePanel
              world={world}
              character={isCharacter ? (world.characters.find((c) => c.id === graphKey) ?? null) : null}
              toast={toast}
              onChanged={onWorldsChanged}
            />
          )}
          {data && tab === "list" && (
            <div className="absolute inset-0 overflow-y-auto">
              <table className="w-full border-collapse text-[12.5px]">
                <thead className="sticky top-0 border-b border-white/10 bg-panel/95 text-left font-mono text-[10.5px] tracking-wider text-ink-3 uppercase backdrop-blur">
                  <tr>
                    <th className="px-4 py-2.5 font-semibold">{t("ui.memory.col_kind")}</th>
                    <th className="px-2 py-2.5 font-semibold">{t("ui.memory.col_name")}</th>
                    <th className="px-2 py-2.5 font-semibold">{t("ui.memory.col_content")}</th>
                    <th className="px-2 py-2.5 font-semibold">{t("ui.memory.col_weight")}</th>
                    <th className="px-2 py-2.5 font-semibold">{t("ui.memory.col_valence")}</th>
                    <th className="px-3 py-2.5 font-semibold">{t("ui.memory.col_recalled")}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/[0.06]">
                  {data.nodes
                    .filter((n) => visibleSet.has(n.id))
                    .sort((a, b) => Number(b.isRoot) - Number(a.isRoot) || (heat[String(b.id)] ?? 0) - (heat[String(a.id)] ?? 0) || b.createdAt - a.createdAt)
                    .map((n) => {
                      const m = kindMeta(n.kind);
                      return (
                        <tr
                          key={n.id}
                          onClick={() => (setCreating(false), setSelected(n.id))}
                          className={`cursor-pointer transition-colors ${selected === n.id ? "bg-accent/12" : "hover:bg-white/[0.03]"}`}
                        >
                          <td className="px-4 py-2 whitespace-nowrap">
                            <span className="chip border-white/15 bg-panel-2/60">
                              <span className="h-2 w-2 rounded-[2px]" style={{ background: m.color }} />
                              {m.label}
                            </span>
                          </td>
                          <td className="px-2 py-2 font-medium">
                            <span className="mr-1.5 font-mono text-[11px] text-ink-3">#{n.id}</span>
                            {n.label}
                            {(heat[String(n.id)] ?? 0) > 0 && <span className="ml-1.5 text-accent">●</span>}
                          </td>
                          <td className="max-w-[420px] truncate px-2 py-2 text-ink-2">{n.content}</td>
                          <td className="px-2 py-2">
                            <div className="h-1.5 w-16 overflow-hidden rounded-[2px] bg-white/10">
                              <div className="h-full bg-accent" style={{ width: `${n.weight * 100}%` }} />
                            </div>
                          </td>
                          <td
                            className={`px-2 py-2 font-mono text-[11.5px] tabular-nums ${
                              n.valence > 0.05 ? "text-emerald-400" : n.valence < -0.05 ? "text-danger" : "text-ink-3"
                            }`}
                          >
                            {n.valence > 0 ? "+" : ""}
                            {n.valence.toFixed(1)}
                          </td>
                          <td className="px-3 py-2 font-mono text-[11px] whitespace-nowrap text-ink-3 tabular-nums">{relTime(n.lastAccessed)}</td>
                        </tr>
                      );
                    })}
                </tbody>
              </table>
            </div>
          )}
          {tab === "graph" && <Legend three={is3d} />}
        </div>
      </section>

      {data && creating && root && <NodeCreator {...editorProps} rootId={root.id} />}
      {data && !creating && node && <NodeEditor {...editorProps} node={node} />}
    </div>
  );
}

function GraphButton({
  active,
  onClick,
  title,
  subtitle,
  children,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  subtitle: string;
  children: React.ReactNode;
}) {
  return (
    <button onClick={onClick} className={`ic mx-1 my-1 flex w-[calc(100%-8px)] items-center gap-2.5 py-2 pr-3 pl-4 text-left ${active ? "on" : ""}`}>
      <span className="pin1" />
      {children}
      <div className="min-w-0 flex-1">
        <div className={`truncate text-[13px] font-medium ${active ? "text-[#f6dca6]" : "text-ink"}`}>{title}</div>
        <div className="truncate font-mono text-[10.5px] text-ink-3 tabular-nums">{subtitle}</div>
      </div>
    </button>
  );
}

function Legend({ three }: { three: boolean }) {
  return (
    <div className="pointer-events-none absolute bottom-3 left-3 rounded-[3px] bg-[#0b0f16]/90 px-3 py-1.5 font-mono text-[10.5px] text-[#8b97a8] shadow-[inset_0_0_0_1px_rgba(148,163,184,0.14)]">
      {three ? t("ui.memory.legend_3d") : t("ui.memory.legend")}
    </div>
  );
}
