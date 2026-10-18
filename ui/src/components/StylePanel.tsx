import { useEffect, useState } from "react";
import { api, errText, type WorldDto, type WorldStyle } from "../api";
import { DEFAULT_STYLE, chatTheme } from "../chatTheme";
import { t } from "../i18n";
import { Passage } from "./ChatPanel";
import type { Toast } from "../App";

interface Props {
  world: WorldDto;
  toast: (t: string, tone?: Toast["tone"]) => void;
  onChanged: () => void;
}

const HEX = /^#[0-9a-fA-F]{6}$/;

/** Free editor of how a world's chats look, with a live preview of a passage. */
export function StylePanel({ world, toast, onChanged }: Props) {
  const [form, setForm] = useState<WorldStyle>(world.style);
  const [hex, setHex] = useState(world.style.accent);
  const [saving, setSaving] = useState(false);
  useEffect(() => (setForm(world.style), setHex(world.style.accent)), [world]);
  const dirty = JSON.stringify(form) !== JSON.stringify(world.style);

  const set = (patch: Partial<WorldStyle>) => setForm((f) => ({ ...f, ...patch }));
  const accent = (v: string) => {
    setHex(v);
    if (HEX.test(v)) set({ accent: v.toLowerCase() });
  };

  async function save() {
    setSaving(true);
    try {
      const s = await api.setWorldStyle(world.key, form);
      setForm(s);
      setHex(s.accent);
      toast(t("ui.style.saved"), "ok");
      onChanged();
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="absolute inset-0 overflow-y-auto">
      <div className="mx-auto grid max-w-5xl gap-8 p-6 lg:grid-cols-[320px_1fr]">
        <div className="space-y-5">
          <div>
            <h3 className="text-[15px] font-semibold">{t("ui.style.title")}</h3>
            <p className="mt-1 text-[12px] leading-relaxed text-ink-3">{t("ui.style.help")}</p>
          </div>

          <Row label={t("ui.style.accent")} help={t("ui.style.accent_help")}>
            <div className="flex items-center gap-2">
              <input type="color" value={form.accent} onChange={(e) => accent(e.target.value)} className="h-8 w-10 cursor-pointer rounded border border-white/10 bg-transparent" />
              <input className={`field w-28 font-mono ${HEX.test(hex) ? "" : "!border-danger/70"}`} value={hex} onChange={(e) => accent(e.target.value.trim())} spellCheck={false} />
            </div>
          </Row>
          <Row label={t("ui.style.mode")}>
            <Switches value={form.mode} options={["dark", "light"]} label={(v) => t(`ui.style.${v}`)} onChange={(mode) => set({ mode })} />
          </Row>
          <Row label={t("ui.style.font")}>
            <Switches value={form.font} options={["serif", "sans"]} label={(v) => t(`ui.style.${v}`)} onChange={(font) => set({ font })} />
          </Row>
          <Row label={t("ui.style.texture")}>
            <Switches value={form.texture} options={["none", "grain", "paper"]} label={(v) => t(`ui.style.${v}`)} onChange={(texture) => set({ texture })} />
          </Row>

          <div className="flex gap-2 pt-1">
            <button className="btn-primary flex-1" disabled={!dirty || saving || !HEX.test(hex)} onClick={save}>
              {t("ui.node.save_changes")}
            </button>
            <button className="btn" disabled={!dirty} onClick={() => (setForm(world.style), setHex(world.style.accent))}>
              {t("common.undo")}
            </button>
            <button className="btn-ghost" onClick={() => (setForm(DEFAULT_STYLE), setHex(DEFAULT_STYLE.accent))}>
              {t("ui.style.reset")}
            </button>
          </div>
        </div>

        {/* Preview: the chat's own components with this style */}
        <div>
          <div className="silk mb-2">{t("ui.style.preview")}</div>
          <div className="overflow-hidden rounded-md shadow-[inset_0_0_0_1px_rgba(148,163,184,0.14)]" style={chatTheme(form)}>
            <div className="space-y-8 px-8 py-9 font-[family-name:var(--c-font)]">
              <Passage m={{ speaker: t("ui.style.sample_name"), speakerId: "x", text: t("ui.style.sample_text"), at: 0 }} mine={false} />
              <Passage m={{ speaker: "", speakerId: null, text: t("ui.style.sample_user"), at: 0 }} mine />
              <div className="flex items-end gap-3 border-t border-[var(--c-rule)] pt-3">
                <span className="flex-1 py-1 text-[17px] text-[var(--c-ink-3)]">{t("ui.chat.write_as", { name: t("common.user") })}</span>
                <span className="c-send">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinejoin="round">
                    <path d="M4 12l16-8-6 16-2-7-8-1z" />
                  </svg>
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function Row({ label, help, children }: { label: string; help?: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="silk mb-1.5">{label}</div>
      {children}
      {help && <p className="mt-1 text-[11px] text-ink-3">{help}</p>}
    </div>
  );
}

function Switches<T extends string>({ value, options, label, onChange }: { value: T; options: T[]; label: (v: T) => string; onChange: (v: T) => void }) {
  return (
    <div className="dip">
      {options.map((o) => (
        <button key={o} className={value === o ? "on" : ""} onClick={() => onChange(o)}>
          {label(o)}
        </button>
      ))}
    </div>
  );
}
