import { useEffect, useMemo, useState } from "react";
import { api, errText, type EdgeDetail, type EdgeKind, type MomentDto, type NodeDto, type NodeKind, type Scope, type Story } from "../api";
import { t } from "../i18n";
import { edgeKinds, edgeMeta, isRootKind, kindMeta, nodeKinds, relTime } from "../meta";
import { Icon } from "./Icon";
import { StoryPicker, storyLabel } from "./StoryPicker";
import type { Toast } from "../App";

interface Common {
  world: string;
  graph: string;
  scope: Scope;
  nodes: NodeDto[];
  /** The world's timeline (to date nodes and links). */
  timeline: MomentDto[];
  toast: (t: string, tone?: Toast["tone"]) => void;
  onChanged: (select?: number | null) => void;
  onSelect: (id: number) => void;
  onClose: () => void;
}

/** Personality belongs to characters: worlds and chats do not offer it. */
const selectableKinds = (scope: Scope) => nodeKinds().filter((k) => !isRootKind(k.kind) && (scope === "character" || k.group !== "persona"));

/** Editor of an existing node: data, links and deletion. */
export function NodeEditor({ node, ...p }: Common & { node: NodeDto }) {
  const [form, setForm] = useState(node);
  const [edges, setEdges] = useState<EdgeDetail[]>([]);
  const [saving, setSaving] = useState(false);
  const dirty =
    form.label !== node.label ||
    form.content !== node.content ||
    form.kind !== node.kind ||
    form.weight !== node.weight ||
    form.valence !== node.valence ||
    form.since !== node.since;

  const loadEdges = () => api.edges(p.world, p.graph, node.id).then(setEdges).catch((e) => p.toast(errText(e), "error"));
  useEffect(() => {
    setForm(node);
    loadEdges();
  }, [node]); // eslint-disable-line react-hooks/exhaustive-deps

  async function save() {
    setSaving(true);
    try {
      await api.updateNode(p.world, p.graph, node.id, {
        kind: node.isRoot ? undefined : form.kind,
        label: form.label,
        content: form.content,
        weight: form.weight,
        valence: form.valence,
        since: node.isRoot ? undefined : form.since,
      });
      p.toast(t("ui.node.saved"), "ok");
      p.onChanged(node.id);
    } catch (e) {
      p.toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    if (!confirm(t("ui.node.delete_confirm", { label: node.label, n: edges.length }))) return;
    try {
      await api.deleteNode(p.world, p.graph, node.id);
      p.toast(t("ui.node.deleted"), "ok");
      p.onChanged(null);
    } catch (e) {
      p.toast(errText(e), "error");
    }
  }

  async function setEdge(e: EdgeDetail, kind: EdgeKind, weight: number) {
    const [from, to] = e.outgoing ? [node.id, e.other] : [e.other, node.id];
    try {
      await api.setEdge(p.world, p.graph, from, to, kind, weight);
      await loadEdges();
      p.onChanged(node.id);
    } catch (err) {
      p.toast(errText(err), "error");
    }
  }

  /** When a link holds; closing it keeps it as the past. */
  async function setValidity(e: EdgeDetail, validFrom: Story, validUntil: Story) {
    const [from, to] = e.outgoing ? [node.id, e.other] : [e.other, node.id];
    try {
      await api.setEdgeValidity(p.world, p.graph, from, to, e.validUntil, validFrom, validUntil);
      await loadEdges();
      p.onChanged(node.id);
    } catch (err) {
      p.toast(errText(err), "error");
    }
  }

  async function removeEdge(e: EdgeDetail) {
    const [from, to] = e.outgoing ? [node.id, e.other] : [e.other, node.id];
    try {
      await api.deleteEdge(p.world, p.graph, from, to, e.validUntil);
      await loadEdges();
      p.onChanged(node.id);
    } catch (err) {
      p.toast(errText(err), "error");
    }
  }

  const meta = kindMeta(node.kind);

  return (
    <Panel
      title={node.label}
      subtitle={
        <span className="flex items-center gap-1.5">
          <span className="h-2 w-2 rounded-full" style={{ background: meta.color }} />
          {meta.label} · #{node.id}
          {node.linked && (
            <span className="chip border-pink-400/40 text-pink-300">
              <Icon name="link" size={11} /> {t("ui.node.own_sheet")}
            </span>
          )}
        </span>
      }
      onClose={p.onClose}
    >
      <NodeFields form={form} setForm={setForm} scope={p.scope} lockKind={node.isRoot} timeline={p.timeline} />

      <div className="pcb-card grid grid-cols-2 gap-x-3 gap-y-1.5 px-3 py-2.5 font-mono text-[11px] text-ink-3 tabular-nums">
        <span>{t("ui.node.created_at", { when: relTime(node.createdAt) })}</span>
        <span>{t("ui.node.last_recall", { when: relTime(node.lastAccessed) })}</span>
        <span>{t("ui.node.times_recalled", { n: node.accessCount })}</span>
        <span>{t("ui.node.link_count", { n: node.degree })}</span>
      </div>

      <div className="flex gap-2">
        <button className="btn-primary flex-1" disabled={!dirty || saving} onClick={save}>
          {t("ui.node.save_changes")}
        </button>
        <button className="btn" disabled={!dirty} onClick={() => setForm(node)}>
          {t("common.undo")}
        </button>
        {!node.isRoot && (
          <button className="btn-danger" onClick={remove} title={t("ui.node.delete")}>
            <Icon name="trash" size={14} />
          </button>
        )}
      </div>

      <section>
        <h4 className="label mt-2">{t("ui.node.links", { n: edges.length })}</h4>
        <div className="space-y-1.5">
          {edges.map((e) => {
            const closed = e.validUntil !== "open";
            const dated = p.timeline.length > 0 || closed || e.validFrom !== "always";
            return (
              <div
                key={`${e.other}-${e.outgoing}-${e.validUntil}`}
                className={`rounded-md border px-2.5 py-2 ${closed ? "border-dashed border-white/15 bg-bg/40" : "border-white/10 bg-bg/80"}`}
              >
                <div className="flex items-center gap-2 text-[12px]">
                  <span className="font-mono text-accent">{e.outgoing ? "→" : "←"}</span>
                  <button
                    className={`min-w-0 flex-1 truncate text-left hover:text-accent ${closed ? "text-ink-3 line-through" : "text-ink"}`}
                    onClick={() => p.onSelect(e.other)}
                  >
                    <span className="mr-1 font-mono text-[10.5px] text-ink-3">#{e.other}</span>
                    {e.otherLabel}
                  </button>
                  {closed && (
                    <span className="chip shrink-0 border-amber-400/40 text-amber-300" title={t("ui.node.past_help")}>
                      {t("ui.node.past", { when: storyLabel(e.validUntil, p.timeline) })}
                    </span>
                  )}
                  <button className="text-ink-3 hover:text-danger" onClick={() => removeEdge(e)} title={t("ui.node.remove_link")}>
                    <Icon name="close" size={13} />
                  </button>
                </div>
                {dated && (
                  <div className="mt-1.5 flex items-center gap-1.5 font-mono text-[10.5px] text-ink-3" title={t("ui.node.holds_help")}>
                    <span>{t("ui.node.holds")}</span>
                    <StoryPicker value={e.validFrom} timeline={p.timeline} open="always" onChange={(s) => setValidity(e, s, e.validUntil)} />
                    <span>→</span>
                    <StoryPicker value={e.validUntil} timeline={p.timeline} open="open" onChange={(s) => setValidity(e, e.validFrom, s)} />
                  </div>
                )}
                {closed ? (
                  <div className="mt-1 font-mono text-[11px] text-ink-3 tabular-nums">
                    {edgeMeta(e.kind).label} · {e.weight.toFixed(2)}
                  </div>
                ) : (
                  <div className="mt-1.5 flex items-center gap-2">
                    <select
                      className="field w-auto py-1 font-mono text-[11.5px]"
                      value={e.kind}
                      onChange={(ev) => setEdge(e, ev.target.value as EdgeKind, e.weight)}
                      style={{ color: edgeMeta(e.kind).color }}
                    >
                      {edgeKinds().map((k) => (
                        <option key={k.kind} value={k.kind}>
                          {k.label}
                        </option>
                      ))}
                    </select>
                    <input
                      type="range"
                      min={0}
                      max={1}
                      step={0.05}
                      defaultValue={e.weight}
                      className="flex-1 accent-[#e5a93c]"
                      onMouseUp={(ev) => setEdge(e, e.kind, Number((ev.target as HTMLInputElement).value))}
                      onKeyUp={(ev) => setEdge(e, e.kind, Number((ev.target as HTMLInputElement).value))}
                    />
                    <span className="w-8 text-right font-mono text-[11px] text-ink-3 tabular-nums">{e.weight.toFixed(2)}</span>
                  </div>
                )}
              </div>
            );
          })}
        </div>
        <AddEdge node={node} nodes={p.nodes} onAdd={async (to, kind, weight, outgoing) => {
          try {
            await api.setEdge(p.world, p.graph, outgoing ? node.id : to, outgoing ? to : node.id, kind, weight);
            await loadEdges();
            p.onChanged(node.id);
          } catch (err) {
            p.toast(errText(err), "error");
          }
        }} />
      </section>
    </Panel>
  );
}

function AddEdge({ node, nodes, onAdd }: { node: NodeDto; nodes: NodeDto[]; onAdd: (to: number, kind: EdgeKind, w: number, outgoing: boolean) => void }) {
  const [q, setQ] = useState("");
  const [kind, setKind] = useState<EdgeKind>("related_to");
  const [outgoing, setOutgoing] = useState(true);
  const options = useMemo(() => nodes.filter((n) => n.id !== node.id).sort((a, b) => a.label.localeCompare(b.label)), [nodes, node.id]);
  const target = options.find((n) => `${n.label} (#${n.id})` === q);
  return (
    <div className="mt-2 space-y-1.5 rounded-lg border border-dashed border-line-2 p-2.5">
      <div className="text-[11px] font-semibold text-ink-3">{t("ui.node.new_link")}</div>
      <input className="field py-1.5" list="node-options" placeholder={t("ui.node.find")} value={q} onChange={(e) => setQ(e.target.value)} />
      <datalist id="node-options">
        {options.map((n) => (
          <option key={n.id} value={`${n.label} (#${n.id})`} />
        ))}
      </datalist>
      <div className="flex gap-1.5">
        <button className="btn px-2" onClick={() => setOutgoing((o) => !o)} title={t("ui.node.direction")}>
          {outgoing ? t("ui.node.outgoing") : t("ui.node.incoming")}
        </button>
        <select className="field py-1 text-[12px]" value={kind} onChange={(e) => setKind(e.target.value as EdgeKind)}>
          {edgeKinds().map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.label}
            </option>
          ))}
        </select>
        <button
          className="btn-primary px-2.5"
          disabled={!target}
          onClick={() => {
            if (target) onAdd(target.id, kind, 0.6, outgoing);
            setQ("");
          }}
        >
          <Icon name="plus" size={14} />
        </button>
      </div>
    </div>
  );
}

/** Form to create a new node. */
export function NodeCreator({ rootId, ...p }: Common & { rootId: number }) {
  const [form, setForm] = useState<NodeDto>({
    id: -1,
    kind: p.scope === "world" ? "lore" : p.scope === "chat" ? "semantic_fact" : p.scope === "library" ? "concept" : "episodic_memory",
    kindCode: 0,
    label: "",
    content: "",
    weight: 0.6,
    valence: 0,
    createdAt: 0,
    lastAccessed: 0,
    accessCount: 0,
    degree: 0,
    isRoot: false,
    linked: false,
    since: "always",
  });
  const [linkFrom, setLinkFrom] = useState<number | "">(rootId);
  const [saving, setSaving] = useState(false);

  async function create() {
    setSaving(true);
    try {
      const id = await api.createNode(p.world, p.graph, {
        kind: form.kind,
        label: form.label,
        content: form.content,
        weight: form.weight,
        valence: form.valence,
        linkFrom: linkFrom === "" ? null : linkFrom,
        edgeKind: null,
      });
      if (form.since !== "always") await api.updateNode(p.world, p.graph, id, { since: form.since });
      p.toast(t("ui.node.created"), "ok");
      p.onChanged(id);
    } catch (e) {
      p.toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Panel title={t("ui.node.new_title")} subtitle={t("ui.node.new_help")} onClose={p.onClose}>
      <NodeFields form={form} setForm={setForm} scope={p.scope} lockKind={false} timeline={p.timeline} />
      <div>
        <label className="label">{t("ui.node.link_from")}</label>
        <select className="field" value={linkFrom} onChange={(e) => setLinkFrom(e.target.value === "" ? "" : Number(e.target.value))}>
          <option value="">{t("ui.node.no_link")}</option>
          {[...p.nodes]
            .sort((a, b) => Number(b.isRoot) - Number(a.isRoot) || a.label.localeCompare(b.label))
            .map((n) => (
              <option key={n.id} value={n.id}>
                {n.isRoot ? "★ " : ""}
                {n.label}
              </option>
            ))}
        </select>
      </div>
      <button className="btn-primary w-full" disabled={!form.label.trim() || saving} onClick={create}>
        <Icon name="plus" size={14} /> {t("ui.node.create")}
      </button>
    </Panel>
  );
}

function NodeFields({
  form,
  setForm,
  scope,
  lockKind,
  timeline,
}: {
  form: NodeDto;
  setForm: (f: NodeDto) => void;
  scope: Scope;
  lockKind: boolean;
  timeline: MomentDto[];
}) {
  return (
    <div className="space-y-3">
      <div className="grid grid-cols-[1fr_auto] gap-2">
        <div>
          <label className="label">{t("ui.node.name")}</label>
          <input className="field" value={form.label} onChange={(e) => setForm({ ...form, label: e.target.value })} />
        </div>
        <div>
          <label className="label">{t("ui.node.kind")}</label>
          <select className="field" disabled={lockKind} value={form.kind} onChange={(e) => setForm({ ...form, kind: e.target.value as NodeKind })}>
            {lockKind && <option value={form.kind}>{kindMeta(form.kind).label}</option>}
            {selectableKinds(scope).map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.label}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div>
        <label className="label">{t("ui.node.content")}</label>
        <textarea className="field min-h-[110px] resize-y leading-relaxed" value={form.content} onChange={(e) => setForm({ ...form, content: e.target.value })} />
      </div>
      <Slider label={t("ui.node.weight")} value={form.weight} min={0} max={1} onChange={(v) => setForm({ ...form, weight: v })} />
      <Slider label={t("ui.node.valence")} value={form.valence} min={-1} max={1} onChange={(v) => setForm({ ...form, valence: v })} signed />
      {!form.isRoot && (timeline.length > 0 || form.since !== "always") && (
        <div className="flex items-center justify-between gap-2" title={t("ui.node.since_help")}>
          <label className="label mb-0">{t("ui.node.since")}</label>
          <StoryPicker value={form.since} timeline={timeline} open="always" onChange={(s) => setForm({ ...form, since: s })} />
        </div>
      )}
    </div>
  );
}

function Slider({ label, value, min, max, onChange, signed = false }: { label: string; value: number; min: number; max: number; onChange: (v: number) => void; signed?: boolean }) {
  const color = signed ? (value > 0.05 ? "text-emerald-400" : value < -0.05 ? "text-danger" : "text-ink-3") : "text-ink-2";
  return (
    <div>
      <div className="flex justify-between">
        <label className="label">{label}</label>
        <span className={`font-mono text-[11.5px] font-semibold tabular-nums ${color}`}>
          {signed && value > 0 ? "+" : ""}
          {value.toFixed(2)}
        </span>
      </div>
      <input type="range" min={min} max={max} step={0.01} value={value} onChange={(e) => onChange(Number(e.target.value))} className="w-full accent-[#e5a93c]" />
    </div>
  );
}

function Panel({ title, subtitle, onClose, children }: { title: string; subtitle: React.ReactNode; onClose: () => void; children: React.ReactNode }) {
  return (
    <aside className="pcb-sidebar flex w-[380px] shrink-0 flex-col border-l border-white/10">
      <div className="flex items-start justify-between gap-2 border-b border-white/10 px-4 py-3">
        <div className="min-w-0">
          <h3 className="truncate font-semibold">{title || t("ui.node.unnamed")}</h3>
          <div className="font-mono text-[11px] text-ink-3">{subtitle}</div>
        </div>
        <button className="btn-ghost" onClick={onClose}>
          <Icon name="close" size={15} />
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">{children}</div>
    </aside>
  );
}
