import { initials, nameColor } from "../meta";

export function Avatar({ name, src, size = 36, ring = false }: { name: string; src?: string | null; size?: number; ring?: boolean }) {
  const style = { width: size, height: size, fontSize: Math.round(size * 0.38) };
  const ringCls = ring ? "ring-2 ring-panel" : "";
  if (src) {
    return <img src={src} alt={name} style={style} className={`shrink-0 rounded-full object-cover ${ringCls}`} />;
  }
  return (
    <div
      style={{ ...style, background: `color-mix(in srgb, ${nameColor(name)} 22%, #13181e)`, color: nameColor(name) }}
      className={`flex shrink-0 items-center justify-center rounded-full font-semibold ${ringCls}`}
      title={name}
    >
      {initials(name)}
    </div>
  );
}

/** Overlapping avatars (groups). */
export function AvatarStack({ people, size = 36 }: { people: { name: string; avatar?: string | null }[]; size?: number }) {
  if (people.length <= 1) {
    const p = people[0];
    return <Avatar name={p?.name ?? "?"} src={p?.avatar} size={size} />;
  }
  const small = Math.round(size * 0.68);
  return (
    <div className="relative shrink-0" style={{ width: size, height: size }}>
      <div className="absolute top-0 left-0">
        <Avatar name={people[0].name} src={people[0].avatar} size={small} ring />
      </div>
      <div className="absolute right-0 bottom-0">
        <Avatar name={people[1].name} src={people[1].avatar} size={small} ring />
      </div>
      {people.length > 2 && (
        <div className="absolute -right-1 -top-1 rounded-full bg-panel-2 px-1 text-[9px] font-bold text-ink-2 ring-2 ring-panel">
          +{people.length - 2}
        </div>
      )}
    </div>
  );
}
