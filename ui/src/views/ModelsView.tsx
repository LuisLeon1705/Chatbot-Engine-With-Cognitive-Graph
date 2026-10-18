import { api, errText, type ModelStatus } from "../api";
import { Icon } from "../components/Icon";
import type { Toast } from "../App";
import { t } from "../i18n";

export function ModelsView({ status, refresh, toast }: { status: ModelStatus | null; refresh: () => void; toast: (t: string, tone?: Toast["tone"]) => void }) {
  async function stop() {
    try {
      await api.stopModels();
      refresh();
    } catch (e) {
      toast(errText(e), "error");
    }
  }
  return (
    <div className="pcb-surface h-full overflow-y-auto p-8">
      <div className="mx-auto max-w-3xl space-y-6">
        <div className="border-b border-white/10 pb-4">
          <h1 className="text-xl font-semibold tracking-tight">{t("ui.models.title")}</h1>
          <p className="mt-1 font-mono text-[12px] text-ink-3">{t("ui.models.intro", { file: "config/models.toml" })}</p>
        </div>

        <div className="space-y-3">
          {status?.slots.map((s) => (
            <div key={s.slot} className="pcb-card flex items-center gap-4 p-4">
              <span
                className={`h-2.5 w-2.5 rounded-[2px] ${
                  s.running ? "bg-accent shadow-[0_0_10px_rgba(229,169,60,0.8)]" : "bg-white/15"
                }`}
              />
              <div className="min-w-0 flex-1">
                <div className="font-semibold tracking-tight">{t(`ui.models.${s.slot}`)}</div>
                <div className="mt-0.5 truncate font-mono text-[11.5px] text-ink-3 tabular-nums">
                  {s.model || t("ui.models.no_model")} · {s.vision ? t("ui.models.with_vision") : t("ui.models.without_vision")} ·{" "}
                  {t("ui.models.vram", { mb: s.gpu_mb })} · {s.url}
                </div>
              </div>
              <span className={`chip ${s.running ? "border-accent/50 bg-accent/10 text-accent" : "border-white/15 text-ink-3"}`}>
                {s.running ? t("ui.models.loaded") : t("ui.models.idle")}
              </span>
            </div>
          ))}
          {status && !status.slots.some((s) => s.slot === "curator") && (
            <p className="font-mono text-[11.5px] text-ink-3">{t("ui.models.curator_shared")}</p>
          )}
        </div>

        {status?.visionError && (
          <div className="pcb-card border-warn/40 bg-warn/10 p-4 text-[13px] text-warn">
            <div className="font-semibold">{t("ui.models.vision_off")}</div>
            <div className="mt-0.5 font-mono text-[11.5px] text-ink-2">{t("ui.models.vision_off_help", { error: status.visionError })}</div>
          </div>
        )}

        <div className="pcb-card flex items-center gap-4 p-4">
          <Icon name="image" size={20} className={status?.imageReady ? "text-accent" : "text-ink-3"} />
          <div className="flex-1">
            <div className="font-semibold tracking-tight">{t("ui.models.images")}</div>
            <div className="mt-0.5 font-mono text-[11.5px] text-ink-3">
              {status?.imageReady ? t("ui.models.image_ready", { info: status.imageInfo }) : t("ui.models.image_unavailable", { info: status?.imageInfo ?? "…" })}
            </div>
          </div>
        </div>

        <div className="flex gap-2">
          <button className="btn" onClick={refresh}>
            <Icon name="refresh" size={14} /> {t("common.refresh")}
          </button>
          <button className="btn-danger" onClick={stop}>
            <Icon name="close" size={14} /> {t("ui.models.free_gpu")}
          </button>
        </div>
        {status && <p className="font-mono text-[11px] text-ink-3">{t("ui.app.project_folder", { dir: status.home })}</p>}
      </div>
    </div>
  );
}
