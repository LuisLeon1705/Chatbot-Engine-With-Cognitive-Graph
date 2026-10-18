import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, errText, on, type ChatDto, type MessageDto, type WorldDto } from "../api";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";
import { RichText } from "./RichText";
import { MomentSelect } from "./StoryPicker";
import type { Toast } from "../App";
import { t } from "../i18n";
import { relTime } from "../meta";

interface Props {
  world: WorldDto;
  chatId: string;
  toast: (t: string, tone?: Toast["tone"]) => void;
  busy: boolean;
  onChanged: () => void;
  onOpenMemory: (graphKey: string) => void;
}

/**
 * A conversation set for reading: one column, generous type, no boxes. Colours, typeface
 * and texture come from the world's style (the `--c-*` variables set by `ChatsView`).
 */
export function ChatPanel({ world, chatId, toast, busy, onChanged, onOpenMemory }: Props) {
  const [chat, setChat] = useState<ChatDto | null>(null);
  const [draft, setDraft] = useState("");
  const [speaker, setSpeaker] = useState<string>("");
  const [pending, setPending] = useState<{ speaker: string; speakerId: string; text: string } | null>(null);
  const [sending, setSending] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const [context, setContext] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  const character = (id?: string | null) => world.characters.find((c) => c.id === id);
  const participants = chat?.participants.map((id) => character(id)).filter((c) => !!c) ?? [];
  const group = participants.length > 1;

  useEffect(() => {
    api.getChat(world.key, chatId).then(setChat).catch((e) => toast(errText(e), "error"));
    api.lastContext(chatId).then(setContext);
  }, [world.key, chatId, toast]);

  // Streaming tokens of this conversation.
  useEffect(() => {
    const un = on.token((e) => {
      if (e.conversation !== chatId) return;
      setPending((p) => ({ speaker: e.speaker, speakerId: e.speakerId, text: (p?.speakerId === e.speakerId ? p.text : "") + e.token }));
    });
    return () => {
      un.then((f) => f());
    };
  }, [chatId]);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [chat?.messages.length, pending?.text]);

  async function send(text: string) {
    if (!chat || sending) return;
    setSending(true);
    const userMsg: MessageDto | null = text.trim() ? { speaker: chat.userName, speakerId: null, text: text.trim(), at: Date.now() / 1000 } : null;
    if (userMsg) setChat({ ...chat, messages: [...chat.messages, userMsg] });
    setDraft("");
    try {
      const reply = await api.send(world.key, chat.id, text, speaker || null);
      setChat((c) => (c ? { ...c, messages: [...c.messages, reply] } : c));
      api.lastContext(chat.id).then(setContext);
      onChanged();
    } catch (e) {
      toast(errText(e), "error");
      api.getChat(world.key, chat.id).then(setChat);
    } finally {
      setPending(null);
      setSending(false);
      input.current?.focus();
    }
  }

  async function remove() {
    if (!chat || !confirm(t("ui.chat.delete_confirm", { title: chat.title }))) return;
    try {
      await api.deleteChat(world.key, chat.id);
      onChanged();
    } catch (e) {
      toast(errText(e), "error");
    }
  }

  async function toggleRemember() {
    if (!chat) return;
    try {
      const c = await api.setChatMemoryMode(world.key, chat.id, !chat.charactersRemember);
      setChat({ ...chat, charactersRemember: c.charactersRemember });
    } catch (e) {
      toast(errText(e), "error");
    }
  }

  async function changeMoment(moment: number | null) {
    if (!chat) return;
    try {
      const c = await api.setChatMoment(world.key, chat.id, moment);
      setChat({ ...chat, moment: c.moment });
    } catch (e) {
      toast(errText(e), "error");
    }
  }

  async function rename(title: string) {
    setEditingTitle(false);
    if (!chat || !title.trim() || title === chat.title) return;
    await api.renameChat(world.key, chat.id, title.trim()).catch((e) => toast(errText(e), "error"));
    setChat({ ...chat, title: title.trim() });
    onChanged();
  }

  if (!chat) return <div className="flex h-full items-center justify-center text-[var(--c-ink-3)]">{t("common.loading")}</div>;

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-4 border-b border-[var(--c-rule)] px-6 py-3">
          <div className="min-w-0 flex-1">
            {editingTitle ? (
              <input
                autoFocus
                className="w-full border-b border-[var(--c-accent)] bg-transparent text-[16px] text-[var(--c-ink)] outline-none"
                defaultValue={chat.title}
                onBlur={(e) => rename(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && rename((e.target as HTMLInputElement).value)}
              />
            ) : (
              <button className="group flex max-w-full items-center gap-1.5 overflow-hidden text-left" onClick={() => setEditingTitle(true)} title={t("ui.chat.rename")}>
                <span className="truncate text-[16px] font-semibold text-[var(--c-ink)]">{chat.title}</span>
                <Icon name="pencil" size={12} className="shrink-0 text-[var(--c-ink-3)] opacity-0 transition-opacity group-hover:opacity-100" />
              </button>
            )}
            <div className="truncate text-[12.5px] text-[var(--c-ink-3)]">
              {participants.map((p) => p.name).join(" · ")} — {t("ui.chat.you_are", { name: chat.userName })}
            </div>
          </div>
          {world.timeline.length > 0 && (
            <MomentSelect
              value={chat.moment ?? null}
              timeline={world.timeline}
              onChange={changeMoment}
              bare
              className="c-select max-w-[180px] shrink-0"
              title={t("ui.chat.moment_help")}
            />
          )}
          <label className="c-link cursor-pointer" title={world.links ? t("ui.collab.remember_help") : t("ui.chat.remember_help")}>
            <input type="checkbox" checked={chat.charactersRemember} onChange={toggleRemember} className="accent-[var(--c-accent)]" />
            {t("ui.chat.remember")}
          </label>
          <button
            className="c-link"
            onClick={() => onOpenMemory(participants.length === 1 ? `chat:${chat.id}:${participants[0].id}` : `chat:${chat.id}`)}
            title={t("ui.chat.memory_help")}
          >
            <Icon name="graph" size={14} /> {t("ui.chat.memory")}
          </button>
          <button className={`c-link ${contextOpen ? "on" : ""}`} onClick={() => setContextOpen((o) => !o)} title={t("ui.chat.turn_memory_title")}>
            <Icon name="eye" size={14} /> {t("ui.chat.turn_memory")}
          </button>
          <button className="c-link hover:!text-danger" onClick={remove} title={t("ui.chat.delete")}>
            <Icon name="trash" size={14} />
          </button>
        </header>

        <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto max-w-[68ch] px-6 pt-12 pb-8 font-[family-name:var(--c-font)]">
            {chat.messages.length === 0 && (
              <p className="py-16 text-center text-[17px] text-[var(--c-ink-3)] italic">
                {group ? t("ui.chat.empty_group") : t("ui.chat.empty_single", { name: participants[0]?.name ?? t("ui.chat.your_character") })}
              </p>
            )}
            <div className="space-y-9">
              {chat.messages.map((m, i) => (
                <Passage key={i} m={m} mine={!m.speakerId} avatar={character(m.speakerId)?.avatar} />
              ))}
              {pending && (
                <Passage m={{ speaker: pending.speaker, speakerId: pending.speakerId, text: pending.text, at: 0 }} mine={false} avatar={character(pending.speakerId)?.avatar} streaming />
              )}
              {sending && !pending && <Typing />}
            </div>
          </div>
        </div>

        {/* Writing */}
        <div className="mx-auto w-full max-w-[calc(68ch+3rem)] px-6 pb-5">
          {group && (
            <div className="mb-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-1 text-[12.5px]">
              <span className="text-[var(--c-ink-3)]">{t("ui.chat.replies")}</span>
              <button className={`c-link ${speaker === "" ? "on" : ""}`} onClick={() => setSpeaker("")}>
                {t("ui.chat.auto")}
              </button>
              {participants.map((p) => (
                <button key={p.id} className={`c-link ${speaker === p.id ? "on" : ""}`} onClick={() => setSpeaker(p.id)}>
                  {p.name}
                </button>
              ))}
            </div>
          )}
          <div className="flex items-end gap-3 border-t border-[var(--c-rule)] pt-3 transition-colors focus-within:border-[var(--c-accent)]">
            <textarea
              ref={input}
              rows={1}
              value={draft}
              disabled={sending}
              onChange={(e) => {
                setDraft(e.target.value);
                e.target.style.height = "auto";
                e.target.style.height = Math.min(e.target.scrollHeight, 180) + "px";
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  if (draft.trim()) send(draft);
                }
              }}
              placeholder={sending ? t("ui.chat.waiting") : t("ui.chat.write_as", { name: chat.userName })}
              title={t("ui.chat.input_help")}
              className="max-h-[180px] min-h-[34px] flex-1 resize-none bg-transparent py-1 font-[family-name:var(--c-font)] text-[17px] leading-relaxed text-[var(--c-ink)] outline-none placeholder:text-[var(--c-ink-3)]"
            />
            <button className="c-link mb-1.5" disabled={sending || (busy && !sending)} onClick={() => send("")} title={t("ui.chat.continue_help")}>
              {t("ui.chat.continue")}
            </button>
            <button className="c-send" disabled={sending || !draft.trim()} onClick={() => send(draft)} title={t("ui.chat.send")}>
              <Icon name="send" size={15} />
            </button>
          </div>
        </div>
      </div>

      {/* What the actor received: a technical drawer, in the memory's style */}
      {contextOpen && (
        <aside className="pcb-sidebar flex w-[390px] shrink-0 flex-col text-ink">
          <div className="flex items-center justify-between border-b border-white/10 px-4 py-3">
            <div>
              <h3 className="silk !text-[11px] !text-accent">{t("ui.chat.last_turn")}</h3>
              <p className="text-[11px] text-ink-3">{t("ui.chat.last_turn_help")}</p>
            </div>
            <button className="btn-ghost" onClick={() => setContextOpen(false)}>
              <Icon name="close" size={15} />
            </button>
          </div>
          <pre className="min-h-0 flex-1 overflow-y-auto px-4 py-3 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-ink-2">
            {context ?? t("ui.chat.no_turn")}
          </pre>
        </aside>
      )}
    </div>
  );
}

/**
 * One message. A character: small portrait, name in the accent, time at the right, the text
 * below. The user: a narrower block to the right, introduced by "You —".
 */
export function Passage({ m, mine, avatar, streaming = false }: { m: MessageDto; mine: boolean; avatar?: string | null; streaming?: boolean }) {
  if (mine) {
    return (
      <article className="ml-auto w-fit max-w-[78%] text-[17px] leading-[1.7] text-[var(--c-ink-2)]">
        <span className="mr-1.5 text-[var(--c-ink-3)]">{t("ui.chat.you")} —</span>
        <RichText text={m.text} inline />
      </article>
    );
  }
  return (
    <article>
      <header className="mb-2 flex items-center gap-2">
        <Avatar name={m.speaker} src={avatar} size={22} />
        <span className="text-[14.5px] font-semibold tracking-[0.01em] text-[var(--c-accent)]">{m.speaker}</span>
        {m.at > 0 && <span className="ml-auto text-[12.5px] text-[var(--c-ink-3)] tabular-nums">{relTime(m.at)}</span>}
      </header>
      <div className="text-[18px] leading-[1.72] text-[var(--c-ink)]">
        <RichText text={m.text} />
        {streaming && <span className="ml-1 inline-block h-[1em] w-[2px] animate-pulse bg-[var(--c-accent)] align-[-0.15em]" />}
      </div>
    </article>
  );
}

function Typing() {
  return (
    <div className="flex items-center gap-1.5 text-[var(--c-accent)]">
      <span className="typing-dot h-1.5 w-1.5 rounded-full bg-current" />
      <span className="typing-dot h-1.5 w-1.5 rounded-full bg-current" />
      <span className="typing-dot h-1.5 w-1.5 rounded-full bg-current" />
    </div>
  );
}
