import type { MomentDto, Story } from "../api";
import { t } from "../i18n";

/** Story time is counted in in-world days (as in the backend). */
const DAYS_PER_YEAR = 365;

/** A story time in words: the timeline's moment when it falls on one, else the year. */
export function storyLabel(s: Story, timeline: MomentDto[]): string {
  if (s === "always") return t("story.always");
  if (s === "open") return t("story.open");
  if (s === "present") return t("story.present");
  const m = timeline.find((x) => x.at === s);
  if (m) return m.name;
  const year = Math.floor(s / DAYS_PER_YEAR);
  const day = s - year * DAYS_PER_YEAR;
  return day === 0 ? t("story.year", { year }) : t("story.year_day", { year, day });
}

const encode = (s: Story) => (typeof s === "number" ? `d:${s}` : `w:${s}`);
const decode = (v: string): Story => (v.startsWith("d:") ? Number(v.slice(2)) : (v.slice(2) as Story));

/**
 * Picks a point of the story among the world's moments. `open` is the word for the
 * unbounded end: "always" for a start ("exists since"), "open" for an end ("until").
 */
export function StoryPicker({
  value,
  timeline,
  open,
  onChange,
  className = "",
  title,
}: {
  value: Story;
  timeline: MomentDto[];
  open: "always" | "open";
  onChange: (s: Story) => void;
  className?: string;
  title?: string;
}) {
  const known = value === open || timeline.some((m) => m.at === value);
  return (
    <select className={`field w-auto py-1 text-[12px] ${className}`} value={encode(value)} onChange={(e) => onChange(decode(e.target.value))} title={title}>
      <option value={encode(open)}>{storyLabel(open, timeline)}</option>
      {timeline.map((m) => (
        <option key={m.id} value={encode(m.at)}>
          {m.name}
        </option>
      ))}
      {!known && <option value={encode(value)}>{storyLabel(value, timeline)}</option>}
    </select>
  );
}

/** Picks a moment of the timeline by id (`null` = the latest, the default of a chat). */
export function MomentSelect({
  value,
  timeline,
  onChange,
  className = "",
  title,
  bare = false,
}: {
  value: number | null;
  timeline: MomentDto[];
  onChange: (id: number | null) => void;
  className?: string;
  title?: string;
  /** Without the field box (the chat styles it with `c-select`). */
  bare?: boolean;
}) {
  return (
    <select
      className={bare ? className : `field ${className}`}
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
      title={title}
    >
      <option value="">{t("ui.timeline.latest")}</option>
      {timeline.map((m) => (
        <option key={m.id} value={m.id}>
          {m.name}
        </option>
      ))}
    </select>
  );
}
