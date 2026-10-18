import { useCallback, useEffect, useState } from "react";
import { api, errText, type CollabLink, type WorldDto } from "../api";
import { CollabBoard } from "../components/CollabBoard";
import { Icon } from "../components/Icon";
import { t } from "../i18n";
import { ChatsView } from "./ChatsView";
import { MemoryView } from "./MemoryView";
import type { Toast } from "../App";

type Tab = "chats" | "memory" | "links";

interface Props {
  /** The real worlds (to link). */
  worlds: WorldDto[];
  toast: (t: string, tone?: Toast["tone"]) => void;
  busy: boolean;
  /** Changes when a curator modifies some memory. */
  tick: number;
  onWorldsChanged: () => void;
}

/**
 * Collabs: places where characters of several worlds meet. Their chats and memory reuse
 * the world views (a Collab travels shaped like a world), plus a board to link worlds.
 */
export function CollabView({ worlds, toast, busy, tick, onWorldsChanged }: Props) {
  const [collabs, setCollabs] = useState<WorldDto[] | null>(null);
  const [key, setKey] = useState(() => {
    try {
      return localStorage.getItem("chats.collab") ?? "";
    } catch {
      return "";
    }
  });
  const [tab, setTab] = useState<Tab>("chats");
  const [creating, setCreating] = useState(false);
  const [requestedGraph, setRequestedGraph] = useState<{ key: string; n: number } | null>(null);

  const reload = useCallback(async () => {
    try {
      const cs = await api.listCollabs();
      setCollabs(cs);
      setKey((k) => (cs.some((c) => c.key === k) ? k : (cs[0]?.key ?? "")));
    } catch (e) {
      toast(errText(e), "error");
    }
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, tick]);

  useEffect(() => {
    try {
      if (key) localStorage.setItem("chats.collab", key);
    } catch {
      // Only a convenience: it is fine if it is not remembered.
    }
  }, [key]);

  const collab = collabs?.find((c) => c.key === key) ?? null;
  const replace = (c: WorldDto) => setCollabs((cs) => (cs ?? []).map((x) => (x.key === c.key ? c : x)));

  if (collabs === null) return <div className="pcb-surface h-full" />;

  if (collabs.length === 0 || !collab) {
    return (
      <div className="pcb-surface flex h-full items-center justify-center p-6">
        <div className="pcb-card w-full max-w-[520px] p-6">
          <div className="silk mb-2 flex items-center gap-2">
            <Icon name="link" size={13} className="text-accent" />
            {t("ui.nav.collab")}
          </div>
          <h2 className="text-[17px] font-semibold">{t("ui.collab.empty")}</h2>
          <p className="mt-1 mb-5 text-[12.5px] leading-relaxed text-ink-3">{t("ui.collab.empty_help")}</p>
          <NewCollabForm
            worlds={worlds}
            toast={toast}
            onCreated={(c) => {
              setCollabs([c]);
              setKey(c.key);
              setTab("links");
            }}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-white/10 bg-[#07090f] px-4 py-2">
        <Icon name="link" size={15} className="text-accent" />
        <span className="max-w-[260px] truncate text-[13.5px] font-semibold">{collab.name}</span>
        <span className="font-mono text-[11px] text-ink-3 tabular-nums">{t("ui.collab.worlds_n", { n: collab.links?.length ?? 0 })}</span>
        <div className="dip ml-3">
          {(["chats", "memory", "links"] as const).map((x) => (
            <button key={x} onClick={() => setTab(x)} className={tab === x ? "on" : ""}>
              {t(`ui.collab.tab_${x}`)}
            </button>
          ))}
        </div>
        <button className="btn-ghost ml-auto" onClick={() => setCreating(true)}>
          <Icon name="plus" size={14} /> {t("ui.collab.new")}
        </button>
      </div>

      <div className="relative min-h-0 flex-1">
        {tab === "chats" ? (
          <ChatsView
            worlds={collabs}
            world={collab}
            setWorldKey={setKey}
            toast={toast}
            busy={busy}
            onOpenMemory={(graph) => {
              setRequestedGraph((r) => ({ key: graph, n: (r?.n ?? 0) + 1 }));
              setTab("memory");
            }}
          />
        ) : tab === "memory" ? (
          <MemoryView
            worlds={collabs}
            world={collab}
            setWorldKey={setKey}
            toast={toast}
            tick={tick}
            onWorldsChanged={reload}
            requestedGraph={requestedGraph}
          />
        ) : (
          <CollabBoard
            collab={collab}
            worlds={worlds}
            toast={toast}
            onChanged={(c) => {
              replace(c);
              onWorldsChanged();
            }}
          />
        )}
      </div>

      {creating && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6" onMouseDown={() => setCreating(false)}>
          <div className="pcb-card w-full max-w-[520px] p-6" onMouseDown={(e) => e.stopPropagation()}>
            <div className="mb-4 flex items-start justify-between">
              <div>
                <h2 className="text-[16px] font-semibold">{t("ui.collab.new")}</h2>
                <p className="mt-1 text-[12px] leading-relaxed text-ink-3">{t("ui.collab.empty_help")}</p>
              </div>
              <button className="btn-ghost" onClick={() => setCreating(false)} title={t("common.cancel")}>
                <Icon name="close" size={15} />
              </button>
            </div>
            <NewCollabForm
              worlds={worlds}
              toast={toast}
              onCreated={(c) => {
                setCreating(false);
                setCollabs((cs) => [...(cs ?? []), c].sort((a, b) => a.name.localeCompare(b.name)));
                setKey(c.key);
                setTab("links");
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}

/** Name, premise and the worlds it starts linking (each at its latest moment). */
function NewCollabForm({ worlds, toast, onCreated }: { worlds: WorldDto[]; toast: Props["toast"]; onCreated: (c: WorldDto) => void }) {
  const [name, setName] = useState("");
  const [premise, setPremise] = useState("");
  const [picked, setPicked] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);

  async function create() {
    setSaving(true);
    try {
      const links: CollabLink[] = picked.map((world) => ({ world, moment: null }));
      onCreated(await api.createCollab(name.trim(), premise.trim(), links));
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-4">
      <div>
        <label className="label">{t("ui.collab.name")}</label>
        <input className="field" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </div>
      <div>
        <label className="label">{t("ui.collab.premise")}</label>
        <textarea className="field min-h-[72px] resize-y" placeholder={t("ui.collab.premise_placeholder")} value={premise} onChange={(e) => setPremise(e.target.value)} />
      </div>
      <div>
        <label className="label">{t("ui.collab.links_title")}</label>
        <div className="flex flex-wrap gap-1.5">
          {worlds.map((w) => {
            const on = picked.includes(w.key);
            return (
              <button
                key={w.key}
                className={`smd ${on ? "" : "off"}`}
                onClick={() => setPicked((p) => (on ? p.filter((x) => x !== w.key) : [...p, w.key]))}
              >
                <span className="led" style={{ color: w.style.accent }} />
                {w.name}
              </button>
            );
          })}
        </div>
      </div>
      <button className="btn-primary w-full" disabled={!name.trim() || saving} onClick={create}>
        {t("ui.collab.create")}
      </button>
    </div>
  );
}
