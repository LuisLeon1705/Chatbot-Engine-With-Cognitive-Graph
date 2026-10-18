import { useEffect, useRef, useState } from "react";
import { api, errText, type AppearanceDto, type Aspect } from "../api";
import { Icon } from "./Icon";
import type { Toast } from "../App";
import { t } from "../i18n";

interface Props {
  world: string;
  character: string;
  name: string;
  toast: (t: string, tone?: Toast["tone"]) => void;
  onChanged: () => void;
}

/** A character's appearance: references, canonical aspects, how the others see them, and portrait. */
export function AppearancePanel({ world, character, name, toast, onChanged }: Props) {
  const [data, setData] = useState<AppearanceDto | null>(null);
  const [draft, setDraft] = useState<Aspect[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [merge, setMerge] = useState(false);
  /** The draft holds a reader's proposal that has not been saved yet. */
  const [proposal, setProposal] = useState(false);
  const [scene, setScene] = useState("");
  const [preview, setPreview] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const apply = (d: AppearanceDto) => {
    setData(d);
    setDraft(d.canonical.map((a) => ({ ...a })));
    setProposal(false);
    if (d.warning) toast(d.warning, "error");
  };

  useEffect(() => {
    setData(null);
    api.appearance(world, character).then(apply).catch((e) => toast(errText(e), "error"));
  }, [world, character]); // eslint-disable-line react-hooks/exhaustive-deps

  const dirty = data != null && JSON.stringify(draft) !== JSON.stringify(data.canonical);

  async function run<T>(label: string, f: () => Promise<T>) {
    setBusy(label);
    try {
      return await f();
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setBusy(null);
    }
  }

  async function onFiles(files: File[]) {
    const d = await run(t("common.saving"), async () => {
      let last: AppearanceDto | undefined;
      for (const file of files) {
        const b64 = await new Promise<string>((resolve, reject) => {
          const r = new FileReader();
          r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
          r.onerror = () => reject(r.error);
          r.readAsDataURL(file);
        });
        last = await api.addReference(world, character, file.name, b64);
      }
      return last;
    });
    if (d) {
      const keep = draft;
      apply(d);
      if (proposal) {
        setDraft(keep);
        setProposal(true);
      }
    }
  }

  async function describe() {
    const aspects = await run(t("ui.appearance.describing"), () => api.describeReferences(world, character));
    if (!aspects || !data) return;
    setDraft(merge ? [...data.canonical.map((a) => ({ ...a })), ...aspects] : aspects);
    setProposal(true);
    toast(t("ui.appearance.proposal", { n: aspects.length }), "ok");
  }

  async function save() {
    const d = await run(t("common.saving"), () => api.setAppearance(world, character, draft));
    if (d) {
      apply(d);
      onChanged();
      toast(t("ui.appearance.saved"), "ok");
    }
  }

  async function portrait() {
    const d = await run(t("ui.appearance.painting"), () => api.portrait(world, character, scene));
    if (d) {
      apply(d);
      onChanged();
    }
  }

  if (!data) return <div className="flex h-full items-center justify-center text-ink-3">{t("common.loading")}</div>;

  return (
    <div className="absolute inset-0 overflow-y-auto">
      <div className="mx-auto grid max-w-6xl grid-cols-[320px_1fr] gap-6 p-6">
        {/* Left column: portrait and references */}
        <div className="space-y-5">
          <section>
            <h3 className="label">{t("ui.appearance.portrait")}</h3>
            <div className="flex aspect-[3/4] items-center justify-center overflow-hidden rounded-xl border border-line bg-bg">
              {data.avatar ? (
                <img src={data.avatar} alt={name} className="h-full w-full cursor-zoom-in object-cover" onClick={() => setPreview(data.avatar ?? null)} />
              ) : (
                <span className="text-[12px] text-ink-3">{t("ui.appearance.no_portrait")}</span>
              )}
            </div>
            <input className="field mt-2" placeholder={t("ui.appearance.scene")} value={scene} onChange={(e) => setScene(e.target.value)} />
            <button className="btn-primary mt-2 w-full" disabled={!!busy || data.canonical.length === 0} onClick={portrait}>
              <Icon name="image" size={15} /> {busy === t("ui.appearance.painting") ? t("ui.appearance.painting") : t("ui.appearance.generate")}
            </button>
          </section>

          <section>
            <h3 className="label">{t("ui.appearance.references", { n: data.references.length })}</h3>
            <div className="grid grid-cols-3 gap-2">
              {data.references.map((r) => (
                <div key={r.file} className="group relative aspect-square overflow-hidden rounded-lg border border-line">
                  <img src={r.dataUrl} alt={r.file} className="h-full w-full cursor-zoom-in object-cover" onClick={() => setPreview(r.dataUrl)} />
                  <button
                    className="absolute top-1 right-1 rounded bg-black/70 p-0.5 text-ink-2 opacity-0 group-hover:opacity-100 hover:text-danger"
                    title={t("ui.appearance.delete_reference")}
                    onClick={async () => {
                      if (!confirm(t("ui.appearance.delete_reference_confirm"))) return;
                      const d = await run(t("ui.appearance.deleting"), () => api.deleteReference(world, character, r.file));
                      if (d) apply(d);
                    }}
                  >
                    <Icon name="close" size={12} />
                  </button>
                </div>
              ))}
              <button
                className="flex aspect-square flex-col items-center justify-center gap-1 rounded-lg border border-dashed border-line-2 text-[11px] text-ink-3 hover:border-accent hover:text-accent disabled:opacity-40"
                disabled={!!busy}
                onClick={() => fileInput.current?.click()}
              >
                <Icon name="plus" size={18} />
                {t("ui.appearance.add")}
              </button>
            </div>
            <input
              ref={fileInput}
              type="file"
              multiple
              accept="image/png,image/jpeg,image/webp"
              className="hidden"
              onChange={(e) => {
                const files = [...(e.target.files ?? [])];
                if (files.length) onFiles(files);
                e.target.value = "";
              }}
            />
            <button className="btn mt-3 w-full" disabled={!!busy || data.references.length === 0} onClick={describe}>
              <Icon name="eye" size={14} /> {t("ui.appearance.describe", { n: data.references.length })}
            </button>
            <label className="mt-1.5 flex items-center gap-2 text-[12px] text-ink-2">
              <input type="checkbox" checked={merge} onChange={(e) => setMerge(e.target.checked)} className="accent-[#e5a93c]" />
              {t("ui.appearance.merge")}
            </label>
            <p className="mt-1 text-[11px] text-ink-3">{t("ui.appearance.reader_help")}</p>
            {busy && <p className="mt-2 text-[12px] text-accent">{busy}</p>}
          </section>
        </div>

        {/* Right column: canonical aspects and perceptions */}
        <div className="space-y-5">
          <section>
            <div className="mb-2 flex items-end justify-between">
              <div>
                <h3 className="label mb-0">{t("ui.appearance.canonical")}</h3>
                <p className="text-[11px] text-ink-3">{t("ui.appearance.canonical_help")}</p>
                {proposal && <p className="mt-1 text-[11px] text-accent">{t("ui.appearance.proposal_pending")}</p>}
              </div>
              <div className="flex gap-2">
                <button className="btn" onClick={() => setDraft([...draft, { label: "", content: "", weight: 0.6 }])}>
                  <Icon name="plus" size={14} /> {t("ui.appearance.add_aspect")}
                </button>
                <button className="btn-primary" disabled={!dirty || !!busy} onClick={save}>
                  {t("common.save")}
                </button>
              </div>
            </div>
            <div className="space-y-2">
              {draft.length === 0 && <p className="rounded-lg border border-dashed border-line-2 p-4 text-center text-[12px] text-ink-3">{t("ui.appearance.no_aspects")}</p>}
              {[...draft.keys()]
                .sort((a, b) => draft[b].weight - draft[a].weight)
                .map((i) => {
                  const a = draft[i];
                  const set = (patch: Partial<Aspect>) => setDraft(draft.map((x, j) => (j === i ? { ...x, ...patch } : x)));
                  return (
                    <div key={i} className="grid grid-cols-[180px_1fr_110px_auto] items-start gap-2 rounded-lg border border-line bg-bg p-2">
                      <input className="field py-1.5 font-semibold" placeholder={t("ui.appearance.aspect")} value={a.label} onChange={(e) => set({ label: e.target.value })} />
                      <textarea className="field min-h-[38px] resize-y py-1.5 leading-snug" rows={3} placeholder={t("ui.appearance.description")} value={a.content} onChange={(e) => set({ content: e.target.value })} />
                      <div className="pt-1">
                        <input type="range" min={0.05} max={1} step={0.05} value={a.weight} onChange={(e) => set({ weight: Number(e.target.value) })} className="w-full accent-[#e5a93c]" title={t("ui.appearance.importance")} />
                        <div className="text-center text-[10px] text-ink-3">{t("ui.appearance.importance_value", { n: Math.round(a.weight * 100) })}</div>
                      </div>
                      <button className="btn-ghost mt-0.5" onClick={() => setDraft(draft.filter((_, j) => j !== i))} title={t("common.remove")}>
                        <Icon name="trash" size={14} />
                      </button>
                    </div>
                  );
                })}
            </div>
            {data.prompt && (
              <details className="mt-3 rounded-lg border border-line bg-bg p-3 text-[12px]">
                <summary className="cursor-pointer text-ink-3">{t("ui.appearance.prompt")}</summary>
                <p className="mt-2 text-ink-2">{data.prompt}</p>
              </details>
            )}
          </section>

          <section>
            <h3 className="label">{t("ui.appearance.seen_by_title")}</h3>
            {data.seenBy.length === 0 && <p className="text-[12px] text-ink-3">{t("ui.appearance.nobody", { name })}</p>}
            <div className="grid grid-cols-2 gap-3">
              {data.seenBy.map((s) => (
                <div key={s.by} className="rounded-lg border border-line bg-panel p-3">
                  <div className="mb-1.5 text-[12px] font-semibold text-accent">{t("ui.appearance.seen_by", { who: s.by })}</div>
                  <ul className="space-y-1 text-[12px] text-ink-2">
                    {s.aspects.map((a) => (
                      <li key={a.label}>
                        <span className="text-ink">{a.label}:</span> {a.content}
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
            <p className="mt-2 text-[11px] text-ink-3">{t("ui.appearance.seen_help")}</p>
          </section>
        </div>
      </div>

      {preview && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-8" onClick={() => setPreview(null)}>
          <img src={preview} alt="" className="max-h-full max-w-full rounded-lg shadow-2xl" />
        </div>
      )}
    </div>
  );
}
