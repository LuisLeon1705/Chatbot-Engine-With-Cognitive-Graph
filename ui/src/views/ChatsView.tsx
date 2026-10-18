import { useCallback, useEffect, useState } from "react";
import { api, errText, type ChatSummary, type WorldDto } from "../api";
import { chatTheme } from "../chatTheme";
import { ChatPanel } from "../components/ChatPanel";
import { Icon } from "../components/Icon";
import { NewChatDialog } from "../components/NewChatDialog";
import { WorldPicker } from "../components/WorldPicker";
import { t } from "../i18n";
import { relTime } from "../meta";
import type { Toast } from "../App";

interface Props {
  worlds: WorldDto[];
  world: WorldDto | null;
  setWorldKey: (k: string) => void;
  toast: (t: string, tone?: Toast["tone"]) => void;
  busy: boolean;
  /** Opens the Memory view on a graph ("chat:<id>"). */
  onOpenMemory: (graphKey: string) => void;
}

/** The world's conversations. The whole view takes the world's look (its style.toml). */
export function ChatsView({ worlds, world, setWorldKey, toast, busy, onOpenMemory }: Props) {
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [filter, setFilter] = useState("");

  const reload = useCallback(async () => {
    if (!world) return;
    try {
      const list = await api.listChats(world.key);
      setChats(list);
      setSelected((s) => (s && list.some((c) => c.id === s) ? s : (list[0]?.id ?? null)));
    } catch (e) {
      toast(errText(e), "error");
    }
  }, [world, toast]);

  useEffect(() => {
    setSelected(null);
    reload();
  }, [world?.key]); // eslint-disable-line react-hooks/exhaustive-deps

  const shown = chats.filter((c) => c.title.toLowerCase().includes(filter.toLowerCase()));

  return (
    <div className="flex h-full font-[family-name:var(--c-font)]" style={chatTheme(world?.style)}>
      <aside className="flex w-[300px] shrink-0 flex-col border-r border-[var(--c-rule)] bg-[var(--c-surface)]/60">
        <div className="space-y-4 px-5 pt-4 pb-3">
          <WorldPicker worlds={worlds} value={world?.key ?? ""} onChange={setWorldKey} chat />
          <div className="flex items-end gap-3">
            <div className="relative flex-1">
              <Icon name="search" size={13} className="absolute top-2 left-0 text-[var(--c-ink-3)]" />
              <input className="c-field pl-5" placeholder={t("ui.chats.search")} value={filter} onChange={(e) => setFilter(e.target.value)} />
            </div>
            <button className="c-send !h-8 !w-8" onClick={() => setCreating(true)} disabled={!world} title={t("ui.chats.new")}>
              <Icon name="plus" size={15} />
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto pb-4">
          {shown.length === 0 && (
            <p className="px-6 py-10 text-center text-[14px] text-[var(--c-ink-3)] italic">
              {chats.length > 0 ? t("ui.chats.no_match") : world?.links ? t("ui.collab.empty_chats") : t("ui.chats.empty_world")}
            </p>
          )}
          {shown.map((c) => {
            const active = selected === c.id;
            return (
              <button
                key={c.id}
                onClick={() => setSelected(c.id)}
                className={`relative block w-full py-3 pr-5 pl-5 text-left transition-colors duration-150 ${
                  active ? "bg-[var(--c-ink)]/[0.06]" : "hover:bg-[var(--c-ink)]/[0.03]"
                }`}
              >
                {active && <span className="absolute top-3 bottom-3 left-0 w-[2px] bg-[var(--c-accent)]" />}
                <div className="flex items-baseline gap-2">
                  <span className={`truncate text-[15px] ${active ? "font-semibold text-[var(--c-ink)]" : "text-[var(--c-ink-2)]"}`}>{c.title}</span>
                  {c.participants.length > 1 && <span className="shrink-0 text-[12px] text-[var(--c-ink-3)]">· {t("ui.chats.group")}</span>}
                  <span className="ml-auto shrink-0 text-[12px] text-[var(--c-ink-3)] tabular-nums">{relTime(c.updatedAt)}</span>
                </div>
                <div className="mt-0.5 truncate text-[13px] text-[var(--c-ink-3)]">
                  {c.lastText ? (
                    <>
                      <span className="text-[var(--c-ink-2)]">{c.lastSpeaker}: </span>
                      {c.lastText.replace(/\*/g, "")}
                    </>
                  ) : (
                    t("ui.chats.no_messages")
                  )}
                </div>
              </button>
            );
          })}
        </div>
      </aside>

      <section className="min-w-0 flex-1">
        {world && selected ? (
          <ChatPanel key={selected} world={world} chatId={selected} toast={toast} busy={busy} onChanged={reload} onOpenMemory={onOpenMemory} />
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-3">
            <p className="text-[17px] text-[var(--c-ink-3)] italic">{t("ui.chats.pick")}</p>
            <button className="c-link" onClick={() => setCreating(true)} disabled={!world}>
              <Icon name="plus" size={14} /> {t("ui.chats.new")}
            </button>
          </div>
        )}
      </section>

      {creating && world && (
        <NewChatDialog
          world={world}
          onClose={() => setCreating(false)}
          onCreated={(id) => {
            setCreating(false);
            reload().then(() => setSelected(id));
          }}
          toast={toast}
        />
      )}
    </div>
  );
}
