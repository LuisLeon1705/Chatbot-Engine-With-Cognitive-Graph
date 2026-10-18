import type { WorldDto } from "../api";
import { t } from "../i18n";

/** World (or Collab) selector. `chat` sets it in the chat's colours (the world's style). */
export function WorldPicker({ worlds, value, onChange, chat = false }: { worlds: WorldDto[]; value: string; onChange: (k: string) => void; chat?: boolean }) {
  const categories = [...new Set(worlds.map((w) => w.category))];
  const collabs = worlds.length > 0 && worlds.every((w) => w.links);
  if (collabs) {
    return (
      <div>
        <label className={chat ? "mb-0.5 block text-[11.5px] text-[var(--c-ink-3)]" : "label"}>{t("ui.collab.pick")}</label>
        <select className={chat ? "c-select w-full !text-[14px] !text-[var(--c-ink)]" : "field cursor-pointer"} value={value} onChange={(e) => onChange(e.target.value)}>
          {worlds.map((w) => (
            <option key={w.key} value={w.key}>
              {t("ui.collab.option", { name: w.name, n: w.links?.length ?? 0 })}
            </option>
          ))}
        </select>
      </div>
    );
  }
  return (
    <div>
      <label className={chat ? "mb-0.5 block text-[11.5px] text-[var(--c-ink-3)]" : "label"}>{t("ui.world_picker.label")}</label>
      <select className={chat ? "c-select w-full !text-[14px] !text-[var(--c-ink)]" : "field cursor-pointer"} value={value} onChange={(e) => onChange(e.target.value)}>
        {categories.map((cat) => (
          <optgroup key={cat} label={cat}>
            {worlds
              .filter((w) => w.category === cat)
              .map((w) => (
                <option key={w.key} value={w.key}>
                  {t("ui.world_picker.option", { name: w.name, n: w.characters.length })}
                </option>
              ))}
          </optgroup>
        ))}
      </select>
    </div>
  );
}
