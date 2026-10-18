import { useCallback, useEffect, useState } from "react";
import { api, errText, on, type ModelStatus, type WorldDto } from "./api";
import { Icon } from "./components/Icon";
import { lang, languages, setBundle, switchLanguage, t } from "./i18n";
import { ChatsView } from "./views/ChatsView";
import { CollabView } from "./views/CollabView";
import { MemoryView } from "./views/MemoryView";
import { ModelsView } from "./views/ModelsView";

type View = "chats" | "memory" | "collab" | "models";

export interface Toast {
  id: number;
  text: string;
  tone: "info" | "ok" | "error";
}

let toastSeq = 0;

export default function App() {
  // Development shortcut: #view=memory opens that view directly.
  const [view, setView] = useState<View>(() => (new URLSearchParams(location.hash.slice(1)).get("view") as View) || "chats");
  const [worlds, setWorlds] = useState<WorldDto[]>([]);
  const [worldKey, setWorldKey] = useState<string>(() => localStorage.getItem("chats.world") ?? "");
  const [status, setStatus] = useState<{ text: string; busy: boolean }>({ text: "", busy: false });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [models, setModels] = useState<ModelStatus | null>(null);
  const [memoryTick, setMemoryTick] = useState(0);
  // Changing it re-renders every text with the new dictionary.
  const [uiLang, setUiLang] = useState(lang());
  // Graph the Memory view should open (e.g. "chat:<id>" from a conversation).
  const [requestedGraph, setRequestedGraph] = useState<{ key: string; n: number } | null>(null);

  const openMemory = useCallback((key: string) => {
    setRequestedGraph((r) => ({ key, n: (r?.n ?? 0) + 1 }));
    setView("memory");
  }, []);

  const toast = useCallback((text: string, tone: Toast["tone"] = "info") => {
    const id = ++toastSeq;
    setToasts((t) => [...t, { id, text, tone }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 5000);
  }, []);

  const loadWorlds = useCallback(async () => {
    try {
      const ws = await api.listWorlds();
      setWorlds(ws);
      setWorldKey((k) => (ws.some((w) => w.key === k) ? k : (ws[0]?.key ?? "")));
    } catch (e) {
      toast(errText(e), "error");
    }
  }, [toast]);

  const refreshModels = useCallback(() => {
    api.modelStatus().then(setModels).catch(() => {});
  }, []);

  async function toggleLanguage() {
    const next = languages().find((l) => l.code !== lang());
    if (!next) return;
    try {
      const bundle = await switchLanguage(next.code);
      setBundle(bundle);
      setUiLang(bundle.lang);
      refreshModels();
    } catch (e) {
      toast(errText(e), "error");
    }
  }

  useEffect(() => {
    loadWorlds();
    refreshModels();
    const timer = setInterval(refreshModels, 8000);
    const subs = [
      on.status((s) => {
        setStatus(s);
        refreshModels();
      }),
      on.memory((m) => {
        setMemoryTick((x) => x + 1);
        loadWorlds(); // keep the node counters up to date
        if (m.error) toast(t("ui.app.memory_error", { name: m.name, error: m.error }), "error");
        else if (m.applied === 1) toast(t("ui.app.memory_change", { name: m.name }), "ok");
        else if (m.applied > 1) toast(t("ui.app.memory_changes", { name: m.name, n: m.applied }), "ok");
      }),
    ];
    return () => {
      clearInterval(timer);
      subs.forEach((p) => p.then((f) => f()));
    };
  }, [loadWorlds, refreshModels, toast]);

  useEffect(() => {
    if (worldKey) localStorage.setItem("chats.world", worldKey);
  }, [worldKey]);

  const world = worlds.find((w) => w.key === worldKey) ?? null;
  const actor = models?.slots.find((s) => s.slot === "actor");

  const nav: { id: View; icon: string; label: string }[] = [
    { id: "chats", icon: "chat", label: t("ui.nav.chats") },
    { id: "memory", icon: "graph", label: t("ui.nav.memory") },
    { id: "collab", icon: "link", label: t("ui.nav.collab") },
    { id: "models", icon: "chip", label: t("ui.nav.models") },
  ];

  const isCodex = view === "chats";

  return (
    <div className="flex h-full flex-col" data-lang={uiLang} data-atmosphere={isCodex ? "codex" : "silicon"}>
      <div className="flex min-h-0 flex-1">
        {/* Navigation rail */}
        <nav
          className={`flex w-[66px] shrink-0 flex-col items-center gap-1.5 border-r py-3.5 transition-colors duration-300 ${
            isCodex ? "border-[#d6b17a]/15 bg-[#110d09]" : "border-white/10 bg-[#07090f]"
          }`}
        >
          <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-md border border-accent/40 bg-accent/15 text-accent shadow-[inset_0_1px_0_0_rgba(255,255,255,0.12)]">
            <Icon name="graph" size={19} />
          </div>
          {nav.map((n) => {
            const active = view === n.id;
            return (
              <button
                key={n.id}
                onClick={() => setView(n.id)}
                className={`relative flex w-14 flex-col items-center gap-1 rounded-md py-2 text-[10px] font-medium tracking-wide transition-all duration-150 active:scale-[0.96] ${
                  active
                    ? "bg-accent/15 text-accent shadow-[inset_0_1px_0_0_rgba(229,169,60,0.2)]"
                    : "text-ink-3 hover:bg-white/[0.04] hover:text-ink"
                }`}
              >
                {active && <span className="absolute top-2 bottom-2 left-0 w-[2px] rounded-r bg-accent" />}
                <Icon name={n.icon} size={19} />
                {n.label}
              </button>
            );
          })}
          <button
            onClick={toggleLanguage}
            title={t("ui.app.switch_language")}
            className="mt-auto flex w-14 flex-col items-center gap-0.5 rounded-md py-2 font-mono text-[10px] font-semibold tracking-wider text-ink-3 uppercase transition-all hover:bg-white/[0.04] hover:text-ink active:scale-[0.96]"
          >
            <Icon name="globe" size={17} />
            {uiLang}
          </button>
        </nav>

        <main className="min-w-0 flex-1">
          {worlds.length === 0 ? (
            <EmptyWorlds onReload={loadWorlds} home={models?.home} />
          ) : view === "chats" ? (
            <ChatsView worlds={worlds} world={world} setWorldKey={setWorldKey} toast={toast} busy={status.busy} onOpenMemory={openMemory} />
          ) : view === "collab" ? (
            <CollabView worlds={worlds} toast={toast} busy={status.busy} tick={memoryTick} onWorldsChanged={loadWorlds} />
          ) : view === "memory" ? (
            <MemoryView
              worlds={worlds}
              world={world}
              setWorldKey={setWorldKey}
              toast={toast}
              tick={memoryTick}
              onWorldsChanged={loadWorlds}
              requestedGraph={requestedGraph}
            />
          ) : (
            <ModelsView status={models} refresh={refreshModels} toast={toast} />
          )}
        </main>
      </div>

      {/* Telemetry Status bar */}
      <footer
        className={`flex h-7 shrink-0 items-center gap-4 border-t px-3.5 font-mono text-[11px] text-ink-3 tabular-nums transition-colors duration-300 ${
          isCodex ? "border-[#d6b17a]/15 bg-[#110d09]" : "border-white/10 bg-[#07090f]"
        }`}
      >
        <span className="flex items-center gap-2">
          <span className={`h-2 w-2 rounded-[2px] ${actor?.running ? "bg-accent shadow-[0_0_6px_rgba(229,169,60,0.8)]" : "bg-white/15"}`} />
          {actor?.running ? t("ui.app.model_loaded", { model: actor.model }) : t("ui.app.model_idle")}
        </span>
        {status.text && (
          <span className={`flex items-center gap-1.5 ${status.busy ? "text-accent" : ""}`}>
            {status.busy && <span className="h-2 w-2 animate-pulse rounded-[2px] bg-accent" />}
            {status.text}
          </span>
        )}
        <span className="ml-auto">{world ? t("ui.app.world_footer", { world: world.name, n: world.characters.length }) : ""}</span>
      </footer>

      {/* Toasts */}
      <div className="pointer-events-none fixed right-4 bottom-10 z-50 flex w-80 flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`toast rounded-md border px-3.5 py-2.5 font-mono text-[11.5px] shadow-[inset_0_1px_0_0_rgba(255,255,255,0.08),0_8px_24px_rgba(0,0,0,0.5)] backdrop-blur-md ${
              t.tone === "error"
                ? "border-danger/45 bg-danger/15 text-danger"
                : t.tone === "ok"
                  ? "border-accent/45 bg-panel-2/95 text-ink"
                  : "border-white/15 bg-panel-2/95 text-ink-2"
            }`}
          >
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

function EmptyWorlds({ onReload, home }: { onReload: () => void; home?: string }) {
  const [before, after] = t("ui.app.create_hint").split("{command}");
  return (
    <div className="pcb-surface flex h-full flex-col items-center justify-center gap-3 text-center text-ink-2">
      <Icon name="graph" size={40} className="text-accent" />
      <p className="text-base font-semibold text-ink">{t("ui.app.no_worlds")}</p>
      <p className="max-w-md text-[13px]">
        {before}
        <code className="rounded border border-white/10 bg-panel-2 px-1.5 py-0.5 font-mono text-[12px] text-accent">chats seed seeds\terra.toml</code>
        {after}
        {home && (
          <>
            <br />
            {t("ui.app.project_folder", { dir: home })}
          </>
        )}
      </p>
      <button className="btn" onClick={onReload}>
        <Icon name="refresh" size={14} /> {t("ui.app.search_again")}
      </button>
    </div>
  );
}
