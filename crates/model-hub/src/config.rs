//! Hand-editable configuration (`config/models.toml`). It is configuration, not memory:
//! hence readable TOML and not the binary format of the `.mem` files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where an LLM runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum GpuMode {
    /// `--fit on`: llama-server spreads the layers according to the free VRAM at start-up.
    Auto,
    /// Everything on the CPU (`-ngl 0`, the mmproj too).
    Cpu,
    /// Every layer on the GPU.
    All,
    /// Fixed number of layers on the GPU.
    Layers(u32),
}

impl TryFrom<String> for GpuMode {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(GpuMode::Auto),
            "cpu" => Ok(GpuMode::Cpu),
            "all" | "gpu" | "todo" => Ok(GpuMode::All),
            n => n.parse().map(GpuMode::Layers).map_err(|_| format!("gpu = \"{s}\": use auto, cpu, all or a number of layers")),
        }
    }
}

impl From<GpuMode> for String {
    fn from(g: GpuMode) -> String {
        match g {
            GpuMode::Auto => "auto".into(),
            GpuMode::Cpu => "cpu".into(),
            GpuMode::All => "all".into(),
            GpuMode::Layers(n) => n.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmSlotConfig {
    /// Fragment of the file name in `models/llm` or `models/vision` ("" = automatic).
    pub model: String,
    /// Context per conversation. With the graph memory, 8k is plenty for the actor.
    pub ctx: u32,
    pub port: u16,
    pub gpu: GpuMode,
    /// CPU threads (0 = llama-server decides).
    pub threads: u32,
    /// Turn on vision when there is a paired mmproj.
    pub vision: bool,
    /// Vision projector on the GPU. On the CPU by default: measured with Gemma 4 12B on an
    /// RTX 3070, it leaves more of the model's layers on the GPU (13.1 vs 11.7 tokens/s) at
    /// the cost of encoding images a bit more slowly.
    pub mmproj_gpu: bool,
    /// Extra arguments for llama-server.
    pub extra_args: Vec<String>,
}

impl LlmSlotConfig {
    fn actor() -> Self {
        Self {
            model: String::new(),
            // Memory replaces the history: 6k per conversation is enough.
            ctx: 6144,
            port: 8080,
            gpu: GpuMode::Auto,
            threads: 0,
            vision: true,
            mmproj_gpu: false,
            // 8-bit KV cache: half the VRAM for the context, more layers on the GPU.
            extra_args: vec!["-ctk".into(), "q8_0".into(), "-ctv".into(), "q8_0".into()],
        }
    }
    fn reader() -> Self {
        Self {
            model: String::new(),
            ctx: 16384,
            port: 8082,
            gpu: GpuMode::Auto,
            threads: 0,
            vision: true,
            mmproj_gpu: true,
            // Several large images per request: a wide batch for the image tokens.
            extra_args: vec!["-ub".into(), "2048".into()],
        }
    }

    fn curator() -> Self {
        Self { model: String::new(), ctx: 4096, port: 8081, gpu: GpuMode::Cpu, threads: 0, vision: false, mmproj_gpu: false, extra_args: vec![] }
    }
}

impl Default for LlmSlotConfig {
    fn default() -> Self {
        Self::actor()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageConfig {
    /// Fragment of the name in `models/diffusion` ("" = the first complete one).
    pub model: String,
    /// `--offload-to-cpu`: weights in RAM, moved to the GPU stage by stage. A must with 8 GB.
    pub offload_to_cpu: bool,
    /// Text encoders on the CPU (`--clip-on-cpu`).
    pub clip_on_cpu: bool,
    pub vae_tiling: bool,
    /// Visual style put in front of every portrait.
    pub style: String,
    pub extra_args: Vec<String>,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            offload_to_cpu: true,
            clip_on_cpu: false,
            vae_tiling: true,
            style: "high quality anime style illustration, human character, half-body portrait, cinematic lighting, detailed background".into(),
            extra_args: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BinConfig {
    /// Path to `llama-server(.exe)`; empty = look in `models/bin`, `bin/` and the PATH.
    pub llama_server: String,
    /// Path to stable-diffusion.cpp's `sd-cli(.exe)`.
    pub sd_cli: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubConfig {
    pub models_dir: PathBuf,
    /// Other folders with the same layout (`llm/`, `mmproj/`, `vision/`), e.g. another
    /// project's, to use their models without copying them.
    pub extra_models_dirs: Vec<PathBuf>,
    /// The GPU's total VRAM.
    pub vram_mb: u64,
    /// What is left free for the desktop, the browser, etc.
    pub reserve_mb: u64,
    pub bin: BinConfig,
    pub actor: LlmSlotConfig,
    /// `model = ""` → the curator shares the actor's server (2 slots in parallel).
    pub curator: LlmSlotConfig,
    /// Vision reader that describes reference images. `model = ""` → the actor does it.
    /// Loading it unloads the actor to get the whole GPU (the actor returns on the next turn).
    pub reader: LlmSlotConfig,
    pub image: ImageConfig,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("models"),
            extra_models_dirs: Vec::new(),
            vram_mb: 8192,
            reserve_mb: 700,
            bin: BinConfig::default(),
            actor: LlmSlotConfig::actor(),
            curator: LlmSlotConfig::curator(),
            reader: LlmSlotConfig::reader(),
            image: ImageConfig::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("{path}: {msg}")]
    Parse { path: PathBuf, msg: String },
}

impl HubConfig {
    /// Loads the TOML; if it does not exist, creates one with the defaults.
    pub fn load_or_create(path: &Path) -> Result<Self, ConfigError> {
        if path.is_file() {
            let text = std::fs::read_to_string(path)?;
            return toml::from_str(&text).map_err(|e| ConfigError::Parse { path: path.to_owned(), msg: e.to_string() });
        }
        let cfg = Self::default();
        cfg.save(path)?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self).map_err(|e| ConfigError::Parse { path: path.to_owned(), msg: e.to_string() })?;
        let header = "# Chats model configuration.\n\
                      # gpu: \"auto\" (fits the free VRAM), \"cpu\", \"all\" or a number of layers.\n\
                      # Empty curator.model = the curator shares the actor's server.\n\
                      # Empty reader.model = the actor also describes reference images.\n\n";
        std::fs::write(path, format!("{header}{body}"))?;
        Ok(())
    }

    /// Usable VRAM.
    pub fn budget_mb(&self) -> u64 {
        self.vram_mb.saturating_sub(self.reserve_mb)
    }

    /// Resolves an executable: configured path → `models/bin` → `bin/` → PATH.
    pub fn find_binary(&self, configured: &str, names: &[&str]) -> Option<PathBuf> {
        if !configured.trim().is_empty() {
            let p = PathBuf::from(configured.trim());
            return p.is_file().then_some(p);
        }
        let exe = |n: &str| if cfg!(windows) { format!("{n}.exe") } else { n.to_owned() };
        let mut dirs = vec![self.models_dir.join("bin"), PathBuf::from("bin")];
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(exe(n)))).find(|p| p.is_file())
    }

    pub fn llama_server(&self) -> Option<PathBuf> {
        self.find_binary(&self.bin.llama_server, &["llama-server"])
    }

    pub fn sd_cli(&self) -> Option<PathBuf> {
        self.find_binary(&self.bin.sd_cli, &["sd-cli", "sd"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip_and_gpu_modes() {
        let dir = std::env::temp_dir().join(format!("model-hub-cfg-{}", std::process::id()));
        let path = dir.join("models.toml");
        let _ = std::fs::remove_file(&path);
        let mut cfg = HubConfig::load_or_create(&path).unwrap();
        assert_eq!(cfg, HubConfig::default());
        cfg.actor.gpu = GpuMode::Layers(30);
        cfg.actor.model = "gemma-4-12b".into();
        cfg.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("gpu = \"30\"") && text.contains("[curator]"), "{text}");
        assert_eq!(HubConfig::load_or_create(&path).unwrap(), cfg);
        assert!(toml::from_str::<HubConfig>("[actor]\ngpu = \"lots\"").is_err());
        assert_eq!(cfg.budget_mb(), 8192 - 700);
    }
}
