import { useEffect, useRef, useState } from "react";
import { api, errText, type CollabLink, type WorldDto } from "../api";
import { t } from "../i18n";
import { Icon } from "./Icon";
import { MomentSelect } from "./StoryPicker";
import type { Toast } from "../App";

interface Props {
  collab: WorldDto;
  /** Every world (linked or not). */
  worlds: WorldDto[];
  toast: (t: string, tone?: Toast["tone"]) => void;
  onChanged: (c: WorldDto) => void;
}

const CARD_W = 232;
const DRAG_TYPE = "application/x-chats-world";

/**
 * The Collab as a board: its memory in the middle, each linked world plugged into it by a
 * trace, at the moment of its timeline the Collab happens at. Worlds are linked by
 * dragging them from the side tray onto the board (or with +).
 */
export function CollabBoard({ collab, worlds, toast, onChanged }: Props) {
  const links = collab.links ?? [];
  const board = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 560 });
  const [saving, setSaving] = useState(false);
  const [dropping, setDropping] = useState(false);

  useEffect(() => {
    const el = board.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  async function save(next: CollabLink[]) {
    setSaving(true);
    try {
      const c = await api.setCollabLinks(collab.key, next);
      onChanged(c);
      toast(t("ui.collab.saved"), "ok");
    } catch (e) {
      toast(errText(e), "error");
    } finally {
      setSaving(false);
    }
  }

  const connect = (world: string) => !links.some((l) => l.world === world) && save([...links, { world, moment: null }]);
  const unlink = (world: string) => save(links.filter((l) => l.world !== world));
  const setMoment = (world: string, moment: number | null) => save(links.map((l) => (l.world === world ? { ...l, moment } : l)));

  const linked = links.flatMap((l) => {
    const w = worlds.find((x) => x.key === l.world);
    return w ? [{ link: l, world: w }] : [];
  });
  const available = worlds.filter((w) => !links.some((l) => l.world === w.key));
  const categories = [...new Set(available.map((w) => w.category))];

  // Worlds around the hub, the first one on the left (two worlds face each other).
  const cx = size.w / 2;
  const cy = size.h / 2;
  // An ellipse that fills the board, keeping the cards clear of the hub.
  const rx = Math.max(CARD_W + 90, size.w / 2 - CARD_W / 2 - 36);
  const ry = Math.max(150, size.h / 2 - 95);
  const placed = linked.map((x, i) => {
    const a = Math.PI + (i / Math.max(1, linked.length)) * Math.PI * 2;
    return { ...x, x: cx + Math.cos(a) * rx, y: cy + Math.sin(a) * ry };
  });

  return (
    <div className="absolute inset-0 flex">
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="border-b border-white/10 bg-panel/80 px-5 py-3">
          <div className="text-[14px] font-semibold">{t("ui.collab.links_title")}</div>
          <p className="mt-0.5 max-w-3xl text-[11.5px] leading-relaxed text-ink-3">{t("ui.collab.links_help")}</p>
          {links.length < 2 && <p className="mt-1 font-mono text-[11px] text-accent">{t("ui.collab.few_links")}</p>}
        </div>

        <div
          ref={board}
          className={`substrate relative min-h-0 flex-1 overflow-hidden transition-[box-shadow] ${dropping ? "shadow-[inset_0_0_0_2px_rgba(229,169,60,0.55)]" : ""}`}
          onDragOver={(e) => {
            if (e.dataTransfer.types.includes(DRAG_TYPE)) {
              e.preventDefault();
              setDropping(true);
            }
          }}
          onDragLeave={() => setDropping(false)}
          onDrop={(e) => {
            setDropping(false);
            const key = e.dataTransfer.getData(DRAG_TYPE);
            if (key) connect(key);
          }}
        >
          <svg className="pointer-events-none absolute inset-0" width={size.w} height={size.h}>
            {placed.map((p) => {
              const d = trace(cx, cy, p.x, p.y);
              return (
                <g key={p.world.key}>
                  <path d={d} fill="none" stroke="rgba(229,169,60,0.16)" strokeWidth={7} strokeLinejoin="round" />
                  <path d={d} fill="none" stroke={p.world.style.accent} strokeOpacity={0.75} strokeWidth={2} strokeLinejoin="round" />
                  <circle cx={p.x} cy={p.y} r={4.5} fill="#0b0f16" stroke={p.world.style.accent} strokeWidth={2} />
                </g>
              );
            })}
          </svg>

          {/* The Collab: its memory */}
          {/* `.ic` and `.pcb-card` set `position: relative` outside the Tailwind layers, so it goes inline. */}
          <div className="ic on w-[250px] -translate-x-1/2 -translate-y-1/2 px-5 py-3.5" style={{ position: "absolute", left: cx, top: cy }}>
            <span className="pin1" />
            <div className="silk flex items-center gap-1.5 !text-accent/80">
              <Icon name="link" size={11} />
              {t("ui.nav.collab")}
            </div>
            <div className="mt-0.5 truncate text-[15px] font-semibold text-[#f6dca6]">{collab.name}</div>
            {collab.premise && <p className="mt-1 line-clamp-3 text-[11.5px] leading-snug text-ink-2">{collab.premise}</p>}
            <div className="mt-1.5 font-mono text-[10.5px] text-ink-3 tabular-nums">
              {t("ui.memory.collab_sub", { n: collab.nodes })} · {t("ui.collab.worlds_n", { n: links.length })}
            </div>
          </div>

          {placed.map(({ link, world, x, y }) => {
            const people = collab.characters.filter((c) => c.world === world.key).length;
            return (
              <div key={world.key} className="pcb-card -translate-x-1/2 -translate-y-1/2 px-3.5 py-3" style={{ position: "absolute", left: x, top: y, width: CARD_W }}>
                <div className="flex items-center gap-2">
                  <span className="led" style={{ color: world.style.accent }} />
                  <span className="min-w-0 flex-1 truncate text-[13.5px] font-semibold">{world.name}</span>
                  <button className="text-ink-3 transition-colors hover:text-danger" onClick={() => unlink(world.key)} disabled={saving} title={t("ui.collab.unlink")}>
                    <Icon name="close" size={13} />
                  </button>
                </div>
                <div className="silk mt-0.5">{world.category}</div>
                <div className="mt-2 flex items-center gap-2">
                  <span className="silk shrink-0">{t("ui.collab.moment")}</span>
                  {world.timeline.length > 0 ? (
                    <MomentSelect
                      value={link.moment}
                      timeline={world.timeline}
                      onChange={(m) => setMoment(world.key, m)}
                      className="min-w-0 flex-1 py-1 font-mono text-[11px]"
                    />
                  ) : (
                    <span className="font-mono text-[11px] text-ink-3">{t("ui.collab.no_timeline")}</span>
                  )}
                </div>
                <div className="mt-1.5 font-mono text-[10.5px] text-ink-3 tabular-nums">{t("ui.collab.people", { n: people })}</div>
              </div>
            );
          })}

          {dropping && (
            <div className="pointer-events-none absolute inset-x-0 bottom-5 text-center font-mono text-[11px] tracking-wide text-accent">{t("ui.collab.drop_here")}</div>
          )}
        </div>
      </div>

      {/* Tray of worlds not linked yet */}
      <aside className="pcb-sidebar flex w-[250px] shrink-0 flex-col border-l border-white/10">
        <div className="silk px-4 pt-4 pb-2">{t("ui.collab.available")}</div>
        <div className="min-h-0 flex-1 overflow-y-auto px-2.5 pb-4">
          {available.length === 0 && <p className="px-2 font-mono text-[11px] text-ink-3">{t("ui.collab.all_linked")}</p>}
          {categories.map((cat) => (
            <div key={cat} className="mb-2">
              <div className="silk px-2 pt-2 pb-1 !text-[9.5px] !text-ink-3">{cat}</div>
              {available
                .filter((w) => w.category === cat)
                .map((w) => (
                  <div
                    key={w.key}
                    draggable
                    onDragStart={(e) => {
                      e.dataTransfer.setData(DRAG_TYPE, w.key);
                      e.dataTransfer.effectAllowed = "link";
                    }}
                    className="ic mx-1 my-1.5 flex cursor-grab items-center gap-2.5 py-2 pr-2 pl-4 active:cursor-grabbing"
                  >
                    <span className="pin1" />
                    <span className="led shrink-0" style={{ color: w.style.accent }} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px] font-medium">{w.name}</div>
                      <div className="font-mono text-[10.5px] text-ink-3 tabular-nums">{t("ui.collab.characters_n", { n: w.characters.length })}</div>
                    </div>
                    <button className="btn-ghost !px-1.5" onClick={() => connect(w.key)} disabled={saving} title={t("ui.collab.connect")}>
                      <Icon name="plus" size={14} />
                    </button>
                  </div>
                ))}
            </div>
          ))}
        </div>
      </aside>
    </div>
  );
}

/** A board trace from the hub to a world: straight, then 45°, like routed copper. */
function trace(x0: number, y0: number, x1: number, y1: number): string {
  const dx = x1 - x0;
  const dy = y1 - y0;
  if (Math.abs(dx) > Math.abs(dy)) {
    const ex = x1 - Math.sign(dx) * Math.abs(dy);
    return `M${x0},${y0} L${ex},${y0} L${x1},${y1}`;
  }
  const ey = y1 - Math.sign(dy) * Math.abs(dx);
  return `M${x0},${y0} L${x0},${ey} L${x1},${y1}`;
}
