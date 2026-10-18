import { useEffect, useState } from "react";
import { api, errText, type CharacterDto, type CharacterMomentDto, type MomentDto, type WorldDto } from "../api";
import { t } from "../i18n";
import { Icon } from "./Icon";
import type { Toast } from "../App";

interface Props {
  world: WorldDto;
  /** A character's moments; `null` = the world's timeline. */
  character: CharacterDto | null;
  toast: (t: string, tone?: Toast["tone"]) => void;
  onChanged: () => void;
}

/** The world's timeline, or the moments of it a character belongs to. */
export function TimelinePanel(p: Props) {
  return (
    <div className="absolute inset-0 overflow-y-auto">
      <div className="mx-auto max-w-3xl space-y-4 p-6">{p.character ? <CharacterMoments {...p} character={p.character} /> : <WorldTimeline {...p} />}</div>
    </div>
  );
}

function WorldTimeline({ world, toast, onChanged }: Props) {
  const [moments, setMoments] = useState<MomentDto[]>(world.timeline);
  const [saving, setSaving] = useState(false);
  useEffect(() => setMoments(world.timeline), [world]);
  const dirty = JSON.stringify(moments) !== JSON.stringify(world.timeline);

  const edit = (i: number, patch: Partial<MomentDto>) => setMoments((ms) => ms.map((m, j) => (j === i ? { ...m, ...patch } : m)));
  const last = moments[moments.length - 1];

  async function save() {
    setSaving(true);
    try {
      setMoments(await api.setTimeline(world.key, moments));
      toast(t("ui.timeline.saved"), "ok");
      onChanged();
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <div>
        <h3 className="text-[15px] font-semibold">{t("ui.timeline.title")}</h3>
        <p className="mt-1 text-[12px] leading-relaxed text-ink-3">{t("ui.timeline.help")}</p>
      </div>
      {moments.length === 0 && <p className="rounded-lg border border-dashed border-line-2 p-4 text-[12px] text-ink-3">{t("ui.timeline.empty")}</p>}
      {moments.map((m, i) => (
        <div key={m.id || `new-${i}`} className="space-y-2 rounded-xl border border-line bg-panel p-3">
          <div className="grid grid-cols-[1fr_96px_80px_auto] items-end gap-2">
            <div>
              <label className="label">{t("ui.timeline.name")}</label>
              <input className="field" value={m.name} onChange={(e) => edit(i, { name: e.target.value })} />
            </div>
            <div>
              <label className="label">{t("ui.timeline.year")}</label>
              <input className="field" type="number" value={m.year} onChange={(e) => edit(i, { year: Math.trunc(Number(e.target.value)) })} />
            </div>
            <div>
              <label className="label">{t("ui.timeline.day")}</label>
              <input
                className="field"
                type="number"
                min={0}
                max={364}
                value={m.day}
                onChange={(e) => edit(i, { day: Math.min(364, Math.max(0, Math.trunc(Number(e.target.value)))) })}
              />
            </div>
            <button className="btn-danger mb-0.5" title={t("ui.timeline.remove")} onClick={() => setMoments((ms) => ms.filter((_, j) => j !== i))}>
              <Icon name="trash" size={14} />
            </button>
          </div>
          <div>
            <label className="label">{t("ui.timeline.summary")}</label>
            <textarea
              className="field min-h-[60px] resize-y leading-relaxed"
              placeholder={t("ui.timeline.summary_help")}
              value={m.summary}
              onChange={(e) => edit(i, { summary: e.target.value })}
            />
          </div>
        </div>
      ))}
      <div className="flex gap-2">
        <button
          className="btn"
          onClick={() => setMoments((ms) => [...ms, { id: 0, name: "", year: last ? last.year + 1 : 1, day: 0, summary: "", at: 0 }])}
        >
          <Icon name="plus" size={14} /> {t("ui.timeline.add")}
        </button>
        <button className="btn-primary ml-auto" disabled={!dirty || saving || moments.some((m) => !m.name.trim())} onClick={save}>
          {t("ui.node.save_changes")}
        </button>
        <button className="btn" disabled={!dirty} onClick={() => setMoments(world.timeline)}>
          {t("common.undo")}
        </button>
      </div>
    </>
  );
}

function CharacterMoments({ world, character, toast, onChanged }: Props & { character: CharacterDto }) {
  const [moments, setMoments] = useState<CharacterMomentDto[]>(character.moments);
  const [saving, setSaving] = useState(false);
  useEffect(() => setMoments(character.moments), [character]);
  const dirty = JSON.stringify(moments) !== JSON.stringify(character.moments);

  const toggle = (id: number) =>
    setMoments((ms) =>
      ms.some((m) => m.moment === id)
        ? ms.filter((m) => m.moment !== id)
        : world.timeline.filter((x) => x.id === id || ms.some((m) => m.moment === x.id)).map((x) => ms.find((m) => m.moment === x.id) ?? { moment: x.id, scenario: "" }),
    );

  async function save() {
    setSaving(true);
    try {
      await api.setCharacterMoments(world.key, character.id, moments);
      toast(t("ui.timeline.saved"), "ok");
      onChanged();
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  if (world.timeline.length === 0) {
    return <p className="rounded-lg border border-dashed border-line-2 p-4 text-[12px] text-ink-3">{t("ui.timeline.no_timeline")}</p>;
  }
  return (
    <>
      <div>
        <h3 className="text-[15px] font-semibold">{t("ui.timeline.character_title", { name: character.name })}</h3>
        <p className="mt-1 text-[12px] leading-relaxed text-ink-3">{t("ui.timeline.character_help")}</p>
        <p className="mt-1 text-[12px] text-accent/80">{moments.length === 0 ? t("ui.timeline.any_moment") : t("ui.timeline.only_these", { n: moments.length })}</p>
      </div>
      {world.timeline.map((m) => {
        const mine = moments.find((x) => x.moment === m.id);
        return (
          <div key={m.id} className={`space-y-2 rounded-xl border p-3 ${mine ? "border-accent/40 bg-accent/5" : "border-line bg-panel"}`}>
            <label className="flex cursor-pointer items-center gap-2">
              <input type="checkbox" className="accent-[#e5a93c]" checked={!!mine} onChange={() => toggle(m.id)} />
              <span className="font-semibold">{m.name}</span>
              {m.summary && <span className="truncate text-[11px] text-ink-3">· {m.summary}</span>}
            </label>
            {mine && (
              <textarea
                className="field min-h-[60px] resize-y leading-relaxed"
                placeholder={character.scenario || t("ui.timeline.scenario_help")}
                value={mine.scenario}
                onChange={(e) => setMoments((ms) => ms.map((x) => (x.moment === m.id ? { ...x, scenario: e.target.value } : x)))}
              />
            )}
          </div>
        );
      })}
      <div className="flex gap-2">
        <button className="btn-primary ml-auto" disabled={!dirty || saving} onClick={save}>
          {t("ui.node.save_changes")}
        </button>
        <button className="btn" disabled={!dirty} onClick={() => setMoments(character.moments)}>
          {t("common.undo")}
        </button>
      </div>
    </>
  );
}
