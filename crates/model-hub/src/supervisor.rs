//! Process supervisor: starts `llama-server` (actor / curator / reader) on demand and
//! shares the VRAM with image generation (`sd-cli`).
//!
//! Policy for small GPUs: before generating an image the VRAM it needs is estimated; if
//! it does not fit next to the loaded LLMs, they are stopped (the largest first). They
//! load again by themselves on the next `ensure` (the first reply after an image takes
//! as long as loading the model).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::Serialize;
use tokio::process::{Child, Command};

use crate::config::{GpuMode, HubConfig, LlmSlotConfig};
use crate::image::{ImageRequest, portrait_prompt};
use crate::registry::{ModelFile, Registry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Actor,
    Curator,
    /// Vision reader (describes reference images); it only loads when needed.
    Reader,
}

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("{0} not found: put it in models/bin, in the PATH or set its path in config/models.toml")]
    NoBinary(&'static str),
    #[error("no model for {0}")]
    NoModel(String),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("llama-server ({slot:?}) closed while starting. Last lines of the log:\n{log}")]
    Exited { slot: Slot, log: String },
    #[error("llama-server ({0:?}) did not answer in time")]
    Timeout(Slot),
    #[error("components missing for {model}: {missing}")]
    MissingComponents { model: String, missing: String },
    #[error("sd-cli failed. Last lines of the log:\n{0}")]
    Image(String),
}

/// Physical batch when there is vision: it covers Gemma 4's image token budget (1120).
pub const VISION_UBATCH: u32 = 1152;

/// Command line of a `llama-server`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LlamaSpec {
    pub model: PathBuf,
    pub mmproj: Option<PathBuf>,
    pub ctx: u32,
    pub gpu: GpuMode,
    pub port: u16,
    pub threads: u32,
    /// Parallel slots (2 if the curator shares this server).
    pub parallel: u32,
    pub alias: String,
    /// Vision projector on the GPU (otherwise `--no-mmproj-offload`).
    pub mmproj_gpu: bool,
    pub extra: Vec<String>,
    /// Estimated VRAM in MB (0 on the CPU).
    pub gpu_mb: u64,
}

impl LlamaSpec {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn args(&self) -> Vec<String> {
        let mut a: Vec<String> = vec![
            "-m".into(),
            self.model.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            self.port.to_string(),
            "-c".into(),
            self.ctx.to_string(),
            "-np".into(),
            self.parallel.to_string(),
            "--alias".into(),
            self.alias.clone(),
            "--jinja".into(),
        ];
        if let Some(p) = &self.mmproj {
            a.push("--mmproj".into());
            a.push(p.to_string_lossy().into_owned());
            // Modern projectors (Gemma 4) use non-causal attention over the image: the
            // number of image tokens is capped by the physical batch. With -ub 512 the
            // image is cut down to 512 tokens and details are lost (colours, accessories).
            if !self.extra.iter().any(|x| x == "-ub" || x == "--ubatch-size") {
                a.extend(["-ub".into(), VISION_UBATCH.to_string()]);
            }
            if !self.mmproj_gpu && self.gpu != GpuMode::Cpu {
                a.push("--no-mmproj-offload".into());
            }
        }
        match self.gpu {
            GpuMode::Auto => a.extend(["--fit".into(), "on".into()]),
            GpuMode::Cpu => {
                a.extend(["-ngl".into(), "0".into()]);
                if self.mmproj.is_some() {
                    a.push("--no-mmproj-offload".into());
                }
            }
            GpuMode::All => a.extend(["-ngl".into(), "999".into()]),
            GpuMode::Layers(n) => a.extend(["-ngl".into(), n.to_string()]),
        }
        if self.threads > 0 {
            a.extend(["-t".into(), self.threads.to_string()]);
        }
        a.extend(self.extra.iter().cloned());
        a
    }
}

/// Which servers must stop for a job of `need_mb` to fit.
/// The one using the most VRAM stops first.
pub fn slots_to_free(budget_mb: u64, running: &[(Slot, u64)], need_mb: u64) -> Vec<Slot> {
    let mut used: u64 = running.iter().map(|(_, mb)| mb).sum();
    let mut by_size: Vec<(Slot, u64)> = running.iter().copied().filter(|(_, mb)| *mb > 0).collect();
    by_size.sort_by_key(|(_, mb)| std::cmp::Reverse(*mb));
    let mut out = Vec::new();
    for (slot, mb) in by_size {
        if used + need_mb <= budget_mb {
            break;
        }
        used -= mb;
        out.push(slot);
    }
    out
}

struct Running {
    child: Child,
    spec: LlamaSpec,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotStatus {
    pub slot: Slot,
    pub running: bool,
    pub model: String,
    pub vision: bool,
    pub gpu_mb: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageOutcome {
    pub path: PathBuf,
    pub seconds: f32,
    /// Servers stopped to free VRAM (they reload on the next turn).
    pub stopped: Vec<Slot>,
    pub model: String,
}

pub struct Supervisor {
    cfg: HubConfig,
    registry: RwLock<Registry>,
    running: tokio::sync::Mutex<HashMap<Slot, Running>>,
    /// Why the mmproj did not load (vision stays off for this session).
    vision_error: parking_lot::Mutex<HashMap<Slot, String>>,
    log_dir: PathBuf,
    http: reqwest::Client,
}

impl Supervisor {
    pub fn new(cfg: HubConfig, log_dir: impl Into<PathBuf>) -> Self {
        let registry = Registry::scan_all(&cfg.models_dir, &cfg.extra_models_dirs);
        Self {
            cfg,
            registry: RwLock::new(registry),
            running: tokio::sync::Mutex::new(HashMap::new()),
            vision_error: parking_lot::Mutex::new(HashMap::new()),
            log_dir: log_dir.into(),
            http: reqwest::Client::builder().timeout(Duration::from_secs(5)).build().expect("HTTP client"),
        }
    }

    pub fn config(&self) -> &HubConfig {
        &self.cfg
    }

    pub fn registry(&self) -> Registry {
        self.registry.read().clone()
    }

    pub fn rescan(&self) {
        *self.registry.write() = Registry::scan_all(&self.cfg.models_dir, &self.cfg.extra_models_dirs);
    }

    pub fn curator_shares_actor(&self) -> bool {
        self.cfg.curator.model.trim().is_empty()
    }

    fn slot_cfg(&self, slot: Slot) -> &LlmSlotConfig {
        match slot {
            Slot::Actor => &self.cfg.actor,
            Slot::Curator => &self.cfg.curator,
            Slot::Reader => &self.cfg.reader,
        }
    }

    /// Is there a vision reader other than the actor?
    pub fn has_reader(&self) -> bool {
        !self.cfg.reader.model.trim().is_empty()
    }

    /// The slot that actually serves (curator and reader may share the actor's).
    fn resolve(&self, slot: Slot) -> Slot {
        match slot {
            Slot::Curator if self.curator_shares_actor() => Slot::Actor,
            Slot::Reader if !self.has_reader() => Slot::Actor,
            s => s,
        }
    }

    fn pick_model(&self, slot: Slot, reg: &Registry) -> Result<ModelFile, HubError> {
        let cfg = self.slot_cfg(slot);
        let found = if cfg.model.trim().is_empty() {
            let (actor, curator) = reg.suggest(self.cfg.budget_mb());
            match slot {
                Slot::Actor | Slot::Reader => actor,
                Slot::Curator => curator,
            }
        } else {
            reg.find_llm(&cfg.model)
        };
        found.cloned().ok_or_else(|| {
            HubError::NoModel(format!(
                "{slot:?} (looked for \"{}\" in {})",
                cfg.model,
                reg.root.join(crate::registry::LLM_DIR).display()
            ))
        })
    }

    /// Server spec of a slot. If the curator shares the actor's, `spec(Curator)` returns
    /// the actor's.
    pub fn spec(&self, slot: Slot) -> Result<LlamaSpec, HubError> {
        let slot = self.resolve(slot);
        let reg = self.registry.read();
        let model = self.pick_model(slot, &reg)?;
        let cfg = self.slot_cfg(slot);
        let shared = slot == Slot::Actor && self.curator_shares_actor();
        let mmproj = if cfg.vision { model.mmproj.clone() } else { None };
        let mmproj_mb = mmproj
            .as_ref()
            .and_then(|p| reg.mmprojs.iter().find(|m| &m.path == p))
            .map_or(0, |m| m.size_mb());
        let parallel = if shared { 2 } else { 1 };
        let ctx = cfg.ctx * parallel;
        let kv_mb = (ctx as u64 / 1024) * (model.params_b.unwrap_or(7.0) * 10.0) as u64;
        let gpu_mb = match cfg.gpu {
            GpuMode::Cpu => 0,
            GpuMode::Layers(0) => 0,
            GpuMode::Auto => (model.size_mb() + mmproj_mb + kv_mb + 300).min(self.cfg.budget_mb()),
            _ => model.size_mb() + mmproj_mb + kv_mb + 300,
        };
        Ok(LlamaSpec {
            alias: model.stem().to_owned(),
            mmproj_gpu: cfg.mmproj_gpu,
            model: model.path,
            mmproj,
            ctx,
            gpu: cfg.gpu,
            port: cfg.port,
            threads: cfg.threads,
            parallel,
            extra: cfg.extra_args.clone(),
            gpu_mb,
        })
    }

    pub fn base_url(&self, slot: Slot) -> String {
        let slot = self.resolve(slot);
        format!("http://127.0.0.1:{}", self.slot_cfg(slot).port)
    }

    /// Makes sure the slot's server is loaded and returns its base URL.
    pub async fn ensure(&self, slot: Slot) -> Result<String, HubError> {
        let slot = self.resolve(slot);
        let mut running = self.running.lock().await;
        if let Some(r) = running.get_mut(&slot) {
            if matches!(r.child.try_wait(), Ok(None)) {
                return Ok(r.spec.base_url());
            }
            running.remove(&slot);
        }
        // The reader and the actor do not fit on the GPU together: loading one unloads the other.
        let rivals: &[Slot] = if slot == Slot::Reader { &[Slot::Actor, Slot::Curator] } else { &[Slot::Reader] };
        for s in rivals {
            if let Some(mut r) = running.remove(s) {
                let _ = r.child.kill().await;
            }
        }
        let mut spec = self.spec(slot)?;
        if self.vision_error.lock().contains_key(&slot) {
            spec.mmproj = None;
        }
        let child = match self.launch(slot, &spec).await {
            // An mmproj this llama-server build does not understand must not leave us
            // without chat: it retries without vision and reports it in the status.
            Err(HubError::Exited { log, .. }) if spec.mmproj.is_some() && is_vision_failure(&log) => {
                let reason = log
                    .lines()
                    .find(|l| l.contains("projector type"))
                    .or_else(|| log.lines().rev().find(|l| l.contains("clip") || l.contains("mmproj")))
                    .map(|l| l.rsplit(": ").next().unwrap_or(l))
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                self.vision_error.lock().insert(slot, reason);
                spec.mmproj = None;
                self.launch(slot, &spec).await?
            }
            other => other?,
        };
        let url = spec.base_url();
        running.insert(slot, Running { child, spec });
        Ok(url)
    }

    /// Why vision was turned off for a slot (if it was): the mmproj did not load.
    pub fn vision_error(&self, slot: Slot) -> Option<String> {
        self.vision_error.lock().get(&self.resolve(slot)).cloned()
    }

    async fn launch(&self, slot: Slot, spec: &LlamaSpec) -> Result<Child, HubError> {
        let bin = self.cfg.llama_server().ok_or(HubError::NoBinary("llama-server"))?;
        std::fs::create_dir_all(&self.log_dir)?;
        let log_path = self.log_dir.join(format!("{}.log", slot_name(slot)));
        let log = std::fs::File::create(&log_path)?;
        let mut cmd = Command::new(&bin);
        cmd.args(spec.args()).stdout(Stdio::from(log.try_clone()?)).stderr(Stdio::from(log)).stdin(Stdio::null()).kill_on_drop(true);
        hide_window(&mut cmd);
        let mut child = cmd.spawn()?;
        die_with_parent(&child);

        let url = spec.base_url();
        let started = Instant::now();
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                return Err(HubError::Exited { slot, log: tail(&log_path, 1800) });
            }
            if let Ok(resp) = self.http.get(format!("{url}/health")).send().await
                && resp.status().is_success()
            {
                return Ok(child);
            }
            if started.elapsed() > Duration::from_secs(300) {
                let _ = child.kill().await;
                return Err(HubError::Timeout(slot));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    pub async fn stop(&self, slot: Slot) -> bool {
        let mut running = self.running.lock().await;
        match running.remove(&slot) {
            Some(mut r) => {
                let _ = r.child.kill().await;
                true
            }
            None => false,
        }
    }

    pub async fn stop_all(&self) {
        let mut running = self.running.lock().await;
        for (_, mut r) in running.drain() {
            let _ = r.child.kill().await;
        }
    }

    pub async fn status(&self) -> Vec<SlotStatus> {
        let mut running = self.running.lock().await;
        let mut out = Vec::new();
        for slot in [Slot::Actor, Slot::Curator, Slot::Reader] {
            if self.resolve(slot) != slot {
                continue;
            }
            let alive = running.get_mut(&slot).is_some_and(|r| matches!(r.child.try_wait(), Ok(None)));
            let spec = running.get(&slot).map(|r| r.spec.clone()).or_else(|| {
                self.spec(slot).ok().map(|mut s| {
                    if self.vision_error.lock().contains_key(&slot) {
                        s.mmproj = None;
                    }
                    s
                })
            });
            out.push(SlotStatus {
                slot,
                running: alive,
                model: spec.as_ref().map(|s| s.alias.clone()).unwrap_or_default(),
                vision: spec.as_ref().is_some_and(|s| s.mmproj.is_some()),
                gpu_mb: spec.as_ref().map_or(0, |s| s.gpu_mb),
                url: self.base_url(slot),
            });
        }
        out
    }

    /// Generates an image at `out` (PNG). Frees VRAM if needed.
    pub async fn generate_image(&self, req: &ImageRequest, out: &Path) -> Result<ImageOutcome, HubError> {
        let reg = self.registry();
        let kit = reg
            .diffusion_kit(&self.cfg.image.model)
            .ok_or_else(|| HubError::NoModel(format!("images (no models in {})", reg.root.join("diffusion").display())))?;
        let missing = kit.missing();
        if !missing.is_empty() {
            return Err(HubError::MissingComponents { model: kit.model.file_name.clone(), missing: missing.join(", ") });
        }
        let bin = self.cfg.sd_cli().ok_or(HubError::NoBinary("sd-cli (stable-diffusion.cpp)"))?;
        let need = kit.vram_estimate_mb(&self.cfg.image, &reg);

        let loaded: Vec<(Slot, u64)> = {
            let mut running = self.running.lock().await;
            running
                .iter_mut()
                .filter_map(|(s, r)| matches!(r.child.try_wait(), Ok(None)).then_some((*s, r.spec.gpu_mb)))
                .collect()
        };
        let stopped = slots_to_free(self.cfg.budget_mb(), &loaded, need);
        for s in &stopped {
            self.stop(*s).await;
        }

        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::create_dir_all(&self.log_dir)?;
        let log_path = self.log_dir.join("image.log");
        let log = std::fs::File::create(&log_path)?;
        let started = Instant::now();
        let mut cmd = Command::new(&bin);
        cmd.args(kit.args(req, out, &self.cfg.image))
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        hide_window(&mut cmd);
        let mut child = cmd.spawn()?;
        die_with_parent(&child);
        let status = match tokio::time::timeout(Duration::from_secs(900), child.wait()).await {
            Ok(s) => s?,
            Err(_) => {
                let _ = child.kill().await;
                return Err(HubError::Image("timed out (15 min)".into()));
            }
        };
        if !status.success() || !out.is_file() {
            return Err(HubError::Image(tail(&log_path, 1800)));
        }
        Ok(ImageOutcome { path: out.to_owned(), seconds: started.elapsed().as_secs_f32(), stopped, model: kit.model.file_name })
    }

    /// Shortcut for portraits: appearance + mood + scene at the family's size.
    pub fn portrait_request(&self, appearance: &str, mood: &str, scene: &str, style: &str) -> Option<ImageRequest> {
        let kit = self.registry.read().diffusion_kit(&self.cfg.image.model)?;
        Some(ImageRequest::portrait(portrait_prompt(appearance, mood, scene, style), kit.family))
    }
}

/// Does the log of a crashed llama-server point at the vision projector?
pub fn is_vision_failure(log: &str) -> bool {
    ["mmproj", "clip_init", "CLIP model", "projector type", "multimodal"].iter().any(|m| log.contains(m))
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Actor => "actor",
        Slot::Curator => "curator",
        Slot::Reader => "reader",
    }
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_: &mut Command) {}

/// Puts the child process in a Job Object with `KILL_ON_JOB_CLOSE`: however `chats` dies
/// (window closed, Ctrl+C, crash), Windows also kills the `llama-server` / `sd-cli`
/// processes and the VRAM is freed.
#[cfg(windows)]
fn die_with_parent(child: &Child) {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    struct Job(HANDLE);
    // The job handle is only used to assign processes; Windows closes it on exit.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}
    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    let job = JOB.get_or_init(|| unsafe {
        let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if h.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            h,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        (ok != 0).then_some(Job(h))
    });
    if let (Some(job), Some(handle)) = (job, child.raw_handle()) {
        unsafe {
            AssignProcessToJobObject(job.0, handle as HANDLE);
        }
    }
}

#[cfg(not(windows))]
fn die_with_parent(_: &Child) {}

/// Last `max` bytes of a log (for error messages).
fn tail(path: &Path, max: usize) -> String {
    let bytes = std::fs::read(path).unwrap_or_default();
    let start = bytes.len().saturating_sub(max);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frees_largest_gpu_users_first_only_when_needed() {
        let running = [(Slot::Actor, 6500), (Slot::Curator, 0)];
        assert!(slots_to_free(7400, &running, 800).is_empty());
        assert_eq!(slots_to_free(7400, &running, 5000), [Slot::Actor]);
        assert_eq!(slots_to_free(7400, &[(Slot::Actor, 4000), (Slot::Curator, 2500)], 2000), [Slot::Actor]);
        assert_eq!(slots_to_free(7400, &[(Slot::Actor, 4000), (Slot::Curator, 2500)], 6000), [Slot::Actor, Slot::Curator]);
    }

    #[test]
    fn recognises_vision_load_failures() {
        let log = "clip_init: failed to load model 'x': load_hparams: unknown projector type: gemma4uv\nmain: exiting due to model loading error";
        assert!(is_vision_failure(log));
        assert!(!is_vision_failure("llama_model_load: error loading model: tensor shape mismatch"));
    }

    #[test]
    fn llama_args_per_gpu_mode() {
        let mut spec = LlamaSpec {
            model: "m.gguf".into(),
            mmproj: Some("p.gguf".into()),
            ctx: 8192,
            gpu: GpuMode::Auto,
            port: 8080,
            threads: 0,
            parallel: 2,
            alias: "gemma".into(),
            mmproj_gpu: false,
            extra: vec![],
            gpu_mb: 0,
        };
        let a = spec.args().join(" ");
        assert!(a.contains("--fit on") && a.contains("--mmproj p.gguf") && a.contains("-np 2") && a.contains("--jinja"), "{a}");
        assert!(a.contains("-ub 1152") && a.contains("--no-mmproj-offload"), "vision: wide batch and projector on the CPU: {a}");
        spec.gpu = GpuMode::Cpu;
        spec.threads = 6;
        let a = spec.args().join(" ");
        assert!(a.contains("-ngl 0") && a.contains("--no-mmproj-offload") && a.contains("-t 6"), "{a}");
    }
}
