import { segments } from "../meta";

/**
 * Roleplay text: *actions* in a softer italic, **emphasis** in bold, paragraph spacing.
 * The colours come from the surface it sits on (`--rt-action`, `--rt-bold`). `inline`
 * keeps the first paragraph on the line it starts on (after a "You —" prefix).
 */
export function RichText({ text, inline = false }: { text: string; inline?: boolean }) {
  return (
    <>
      {text.split(/\n{2,}/).map((para, i) => (
        <p key={i} className={`mb-3.5 whitespace-pre-wrap last:mb-0 ${inline && i === 0 ? "inline" : ""}`}>
          {segments(para).map((s, j) =>
            s.style === "action" ? (
              <em key={j} className="text-[color:var(--rt-action,#c4b095)] italic">
                {s.text}
              </em>
            ) : s.style === "bold" ? (
              <strong key={j} className="font-semibold text-[color:var(--rt-bold,#faf3e6)]">
                {s.text}
              </strong>
            ) : (
              <span key={j}>{s.text}</span>
            ),
          )}
        </p>
      ))}
    </>
  );
}
