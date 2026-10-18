import { useMemo, useRef, useState } from "react";
import { api, errText, type CharacterDto, type WorldDto } from "../api";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";
import { MomentSelect } from "./StoryPicker";
import type { Toast } from "../App";
import { t } from "../i18n";

interface Props {
  world: WorldDto;
  onClose: () => void;
  onCreated: (id: string) => void;
  toast: (t: string, tone?: Toast["tone"]) => void;
}

/** Lowercase without accents, to search "kaltsit" or "teresa" loosely. */
const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/['’]/g, "")
    .toLowerCase();

/**
 * New chat. Made for worlds with many characters: a search box (name or description,
 * ↑/↓ and Enter), the people already in the scene on top in speaking order, and a compact
 * list that hides whoever does not belong to the chosen moment of the story.
 */
export function NewChatDialog({ world, onClose, onCreated, toast }: Props) {
  const [picked, setPicked] = useState<string[]>([]);
  const [title, setTitle] = useState("");
  const [userName, setUserName] = useState(() => localStorage.getItem("chats.userName") ?? "");
  const [saving, setSaving] = useState(false);
  const [remember, setRemember] = useState(true);
  const [moment, setMoment] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  // A character fits a moment if they list it, or if they list none.
  const momentId = moment ?? world.timeline[world.timeline.length - 1]?.id ?? null;
  const fits = (c: CharacterDto) => momentId === null || c.moments.length === 0 || c.moments.some((m) => m.moment === momentId);
  const byId = (id: string) => world.characters.find((c) => c.id === id);

  const inMoment = useMemo(() => world.characters.filter(fits), [world, momentId]); // eslint-disable-line react-hooks/exhaustive-deps
  const shown = useMemo(() => {
    const q = fold(query.trim());
    if (!q) return inMoment;
    // Name matches first, then description matches.
    const byName = inMoment.filter((c) => fold(c.name).includes(q));
    const byText = inMoment.filter((c) => !byName.includes(c) && (fold(c.tagline).includes(q) || fold(c.worldName ?? "").includes(q)));
    return [...byName, ...byText];
  }, [inMoment, query]);
  const hidden = world.characters.length - inMoment.length;

  const toggle = (id: string) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
  const names = picked.map((id) => byId(id)?.name ?? "");
  const group = picked.length > 1;

  function pickMoment(m: number | null) {
    setMoment(m);
    const id = m ?? world.timeline[world.timeline.length - 1]?.id ?? null;
    // Whoever does not fit the new moment leaves the scene.
    setPicked((p) => p.filter((x) => {
      const c = byId(x);
      return !c || id === null || c.moments.length === 0 || c.moments.some((cm) => cm.moment === id);
    }));
  }

  function onSearchKey(e: React.KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.max(0, Math.min(shown.length - 1, cursor + (e.key === "ArrowDown" ? 1 : -1)));
      setCursor(next);
      list.current?.querySelector<HTMLElement>(`[data-row="${next}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && shown[cursor]) {
      e.preventDefault();
      toggle(shown[cursor].id);
      setQuery("");
      setCursor(0);
    }
  }

  async function create() {
    setSaving(true);
    try {
      localStorage.setItem("chats.userName", userName.trim());
      const chat = await api.createChat(world.key, picked, title.trim(), userName.trim() || t("common.user"), remember, moment);
      onCreated(chat.id);
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6" onMouseDown={onClose}>
      <div
        className="flex max-h-full w-[620px] flex-col rounded-lg border border-[var(--c-rule)] bg-[var(--c-bg)] text-[var(--c-ink)] shadow-[0_24px_64px_rgba(0,0,0,0.6)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="flex items-start justify-between px-6 pt-5 pb-3">
          <div>
            <h2 className="text-[19px] font-semibold">{t("ui.new_chat.title")}</h2>
            <p className="text-[13px] text-[var(--c-ink-3)]">{t("ui.new_chat.help")}</p>
          </div>
          <button className="c-link" onClick={onClose}>
            <Icon name="close" size={16} />
          </button>
        </header>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 pb-4">
          {world.timeline.length > 0 && (
            <div className="flex items-baseline gap-3">
              <span className="shrink-0 text-[13px] text-[var(--c-ink-3)]">{t("ui.new_chat.moment")}</span>
              <MomentSelect bare value={moment} timeline={world.timeline} onChange={pickMoment} className="c-select min-w-0 flex-1" />
            </div>
          )}

          {/* Who is in the scene, in speaking order */}
          <div>
            <div className="mb-1.5 text-[13px] text-[var(--c-ink-3)]">{t("ui.new_chat.in_scene")}</div>
            {picked.length === 0 ? (
              <p className="text-[14px] text-[var(--c-ink-3)] italic">{t("ui.new_chat.none_picked")}</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {picked.map((id, i) => {
                  const c = byId(id);
                  if (!c) return null;
                  return (
                    <button
                      key={id}
                      onClick={() => toggle(id)}
                      title={t("ui.new_chat.remove")}
                      className="group flex items-center gap-1.5 rounded-full bg-[var(--c-accent)]/15 py-0.5 pr-2.5 pl-0.5 text-[13.5px] text-[var(--c-ink)] transition-colors hover:bg-[var(--c-accent)]/25"
                    >
                      <Avatar name={c.name} src={c.avatar} size={22} />
                      {group && <span className="text-[11.5px] text-[var(--c-accent)] tabular-nums">{i + 1}</span>}
                      {c.name}
                      <Icon name="close" size={11} className="text-[var(--c-ink-3)] group-hover:text-[var(--c-ink)]" />
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          {/* Search and list */}
          <div>
            <div className="relative">
              <Icon name="search" size={14} className="absolute top-2 left-0 text-[var(--c-ink-3)]" />
              <input
                autoFocus
                className="c-field pl-6"
                placeholder={t("ui.new_chat.search")}
                value={query}
                onChange={(e) => (setQuery(e.target.value), setCursor(0))}
                onKeyDown={onSearchKey}
              />
            </div>
            <div ref={list} className="mt-2 max-h-[300px] overflow-y-auto">
              {shown.length === 0 && <p className="py-6 text-center text-[14px] text-[var(--c-ink-3)] italic">{t("ui.new_chat.no_match")}</p>}
              {shown.map((c, i) => {
                const on = picked.includes(c.id);
                return (
                  <button
                    key={c.id}
                    data-row={i}
                    onClick={() => toggle(c.id)}
                    onMouseEnter={() => setCursor(i)}
                    className={`flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left transition-colors ${
                      i === cursor ? "bg-[var(--c-ink)]/[0.06]" : ""
                    }`}
                  >
                    <Avatar name={c.name} src={c.avatar} size={28} />
                    <div className="min-w-0 flex-1">
                      <div className={`text-[15px] ${on ? "font-semibold text-[var(--c-accent)]" : "text-[var(--c-ink)]"}`}>
                        {c.name}
                        {c.worldName && <span className="ml-2 text-[11.5px] font-normal tracking-wide text-[var(--c-ink-3)] uppercase">{c.worldName}</span>}
                      </div>
                      <div className="truncate text-[12.5px] text-[var(--c-ink-3)]">{c.tagline}</div>
                    </div>
                    <span
                      className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full border text-[var(--c-accent-ink)] ${
                        on ? "border-transparent bg-[var(--c-accent)]" : "border-[var(--c-rule)]"
                      }`}
                    >
                      {on && <Icon name="plus" size={11} className="rotate-45" />}
                    </span>
                  </button>
                );
              })}
            </div>
            {hidden > 0 && <p className="mt-1 text-[12px] text-[var(--c-ink-3)]">{t("ui.new_chat.out_of_moment", { n: hidden })}</p>}
          </div>

          <div className="grid grid-cols-2 gap-4">
            <label className="block">
              <span className="text-[13px] text-[var(--c-ink-3)]">{t("ui.new_chat.chat_title")}</span>
              <input className="c-field" placeholder={names.join(", ") || t("ui.new_chat.title_placeholder")} value={title} onChange={(e) => setTitle(e.target.value)} />
            </label>
            <label className="block">
              <span className="text-[13px] text-[var(--c-ink-3)]">{t("ui.new_chat.your_name")}</span>
              <input className="c-field" placeholder={t("common.user")} value={userName} onChange={(e) => setUserName(e.target.value)} />
            </label>
          </div>
          {group && <p className="text-[13px] text-[var(--c-ink-2)]">{t("ui.new_chat.group_help")}</p>}
          <label className="flex items-start gap-2 text-[13px] text-[var(--c-ink-2)]">
            <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} className="mt-0.5 accent-[var(--c-accent)]" />
            <span>
              {t("ui.new_chat.remember")}
              <span className="block text-[12px] text-[var(--c-ink-3)]">{world.links ? t("ui.collab.remember_help") : t("ui.chat.remember_help")}</span>
            </span>
          </label>
        </div>

        <footer className="flex items-center justify-end gap-4 border-t border-[var(--c-rule)] px-6 py-3.5">
          <button className="c-link !text-[13.5px]" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="c-primary" disabled={picked.length === 0 || saving} onClick={create}>
            <Icon name={group ? "users" : "chat"} size={15} />
            {group ? t("ui.new_chat.create_group", { n: picked.length }) : t("ui.new_chat.start")}
          </button>
        </footer>
      </div>
    </div>
  );
}
