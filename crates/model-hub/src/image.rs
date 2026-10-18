//! Image generation with stable-diffusion.cpp (`sd-cli`).
//!
//! One process per image: it loads the model, generates and frees ALL the VRAM when it
//! finishes. With 8 GB this coexists with the actor (the supervisor unloads it if needed).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ImageConfig;
use crate::registry::{DiffusionFamily, EncoderKind, ModelFile, Registry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub prompt: String,
    #[serde(default)]
    pub negative: String,
    pub width: u32,
    pub height: u32,
    /// `None` = the family's recommended value.
    pub steps: Option<u32>,
    pub cfg_scale: Option<f32>,
    pub sampler: Option<String>,
    /// -1 = random.
    pub seed: i64,
}

impl ImageRequest {
    /// Vertical portrait with the family's defaults.
    pub fn portrait(prompt: impl Into<String>, family: DiffusionFamily) -> Self {
        let (w, h) = family.portrait_size();
        Self { prompt: prompt.into(), negative: String::new(), width: w, height: h, steps: None, cfg_scale: None, sampler: None, seed: -1 }
    }
}

/// Recommended parameters per family (steps, CFG, sampler).
pub struct FamilyDefaults {
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: Option<&'static str>,
    /// Default negative prompt (families with CFG 1 ignore it).
    pub negative: &'static str,
}

impl DiffusionFamily {
    pub fn defaults(self, file_name: &str) -> FamilyDefaults {
        let lower = file_name.to_ascii_lowercase();
        let neg = "lowres, blurry, bad anatomy, extra fingers, deformed hands, watermark, text";
        match self {
            DiffusionFamily::ZImage if lower.contains("turbo") => FamilyDefaults { steps: 8, cfg_scale: 1.0, sampler: None, negative: "" },
            DiffusionFamily::ZImage => FamilyDefaults { steps: 28, cfg_scale: 5.0, sampler: None, negative: neg },
            DiffusionFamily::Flux if lower.contains("schnell") => FamilyDefaults { steps: 4, cfg_scale: 1.0, sampler: Some("euler"), negative: "" },
            DiffusionFamily::Flux => FamilyDefaults { steps: 20, cfg_scale: 1.0, sampler: Some("euler"), negative: "" },
            DiffusionFamily::Chroma => FamilyDefaults { steps: 26, cfg_scale: 4.0, sampler: Some("euler"), negative: neg },
            DiffusionFamily::QwenImage => FamilyDefaults { steps: 20, cfg_scale: 2.5, sampler: Some("euler"), negative: "" },
            DiffusionFamily::Sd3 => FamilyDefaults { steps: 28, cfg_scale: 4.5, sampler: Some("euler"), negative: neg },
            DiffusionFamily::Sdxl => FamilyDefaults { steps: 26, cfg_scale: 6.0, sampler: Some("euler_a"), negative: neg },
            DiffusionFamily::Sd15 | DiffusionFamily::Unknown => FamilyDefaults { steps: 25, cfg_scale: 7.0, sampler: Some("euler_a"), negative: neg },
        }
    }

    pub fn portrait_size(self) -> (u32, u32) {
        match self {
            DiffusionFamily::Sd15 | DiffusionFamily::Unknown => (512, 768),
            DiffusionFamily::Sdxl => (832, 1216),
            _ => (768, 1024),
        }
    }

    /// "Diffusion only" models that need `--diffusion-model` and separate components.
    fn split_weights(self) -> bool {
        matches!(self, DiffusionFamily::Flux | DiffusionFamily::Chroma | DiffusionFamily::ZImage | DiffusionFamily::QwenImage)
    }

    /// Components each family needs (complete SD1.5/SDXL checkpoints carry them inside).
    fn needs(self) -> &'static [Need] {
        use Need::*;
        match self {
            DiffusionFamily::Flux => &[Vae, ClipL, T5],
            DiffusionFamily::Chroma => &[Vae, T5],
            DiffusionFamily::ZImage => &[Vae, Llm],
            DiffusionFamily::QwenImage => &[Vae, Llm],
            DiffusionFamily::Sd3 => &[ClipL, ClipG, T5],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    Vae,
    ClipL,
    ClipG,
    T5,
    Llm,
}

/// A diffusion model + the components that go with it.
#[derive(Debug, Clone, Serialize)]
pub struct DiffusionKit {
    pub model: ModelFile,
    pub family: DiffusionFamily,
    pub vae: Option<PathBuf>,
    pub clip_l: Option<PathBuf>,
    pub clip_g: Option<PathBuf>,
    pub t5xxl: Option<PathBuf>,
    pub llm: Option<PathBuf>,
}

impl DiffusionKit {
    /// Required components that were not found, as keys (`vae`, `clip_l`, `clip_g`,
    /// `t5xxl`, `llm_qwen3`, `llm_qwen25vl`); the dictionary has `kit.missing.<key>`.
    pub fn missing(&self) -> Vec<&'static str> {
        let embedded_sd3 = self.family == DiffusionFamily::Sd3 && self.model.file_name.to_ascii_lowercase().contains("incl_clips");
        self.family
            .needs()
            .iter()
            .filter(|n| !(embedded_sd3 && **n != Need::Vae))
            .filter_map(|n| match n {
                Need::Vae if self.vae.is_none() => Some("vae"),
                Need::ClipL if self.clip_l.is_none() => Some("clip_l"),
                Need::ClipG if self.clip_g.is_none() => Some("clip_g"),
                Need::T5 if self.t5xxl.is_none() => Some("t5xxl"),
                Need::Llm if self.llm.is_none() => Some(match self.family {
                    DiffusionFamily::QwenImage => "llm_qwen25vl",
                    _ => "llm_qwen3",
                }),
                _ => None,
            })
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        self.missing().is_empty()
    }

    /// Approximate VRAM in MB during generation.
    pub fn vram_estimate_mb(&self, cfg: &ImageConfig, reg: &Registry) -> u64 {
        let size = |p: &Option<PathBuf>| {
            p.as_ref().and_then(|p| reg.encoders.iter().chain(&reg.vaes).find(|m| &m.path == p)).map_or(0, |m| m.size_mb())
        };
        let encoders = size(&self.clip_l) + size(&self.clip_g) + size(&self.t5xxl) + size(&self.llm);
        let vae = size(&self.vae);
        let model = self.model.size_mb();
        let activations = 1200;
        if cfg.offload_to_cpu {
            // The weights live in RAM and move up stage by stage: the peak is the largest stage.
            model.max(if cfg.clip_on_cpu { 0 } else { encoders }) + vae + activations
        } else {
            model + vae + activations + if cfg.clip_on_cpu { 0 } else { encoders }
        }
    }

    /// Arguments for `sd-cli`.
    pub fn args(&self, req: &ImageRequest, out: &Path, cfg: &ImageConfig) -> Vec<String> {
        let d = self.family.defaults(&self.model.file_name);
        let mut a: Vec<String> = Vec::new();
        let mut push = |k: &str, v: String| {
            a.push(k.to_owned());
            a.push(v);
        };
        let path = |p: &Path| p.to_string_lossy().into_owned();
        push(if self.family.split_weights() { "--diffusion-model" } else { "-m" }, path(&self.model.path));
        if let Some(p) = &self.vae {
            push("--vae", path(p));
        }
        if let Some(p) = &self.clip_l {
            push("--clip_l", path(p));
        }
        if let Some(p) = &self.clip_g {
            push("--clip_g", path(p));
        }
        if let Some(p) = &self.t5xxl {
            push("--t5xxl", path(p));
        }
        if let Some(p) = &self.llm {
            push("--llm", path(p));
        }
        push("-p", req.prompt.clone());
        let negative = if req.negative.trim().is_empty() { d.negative.to_owned() } else { req.negative.clone() };
        if !negative.is_empty() {
            push("-n", negative);
        }
        push("-W", req.width.to_string());
        push("-H", req.height.to_string());
        push("--steps", req.steps.unwrap_or(d.steps).to_string());
        push("--cfg-scale", format!("{}", req.cfg_scale.unwrap_or(d.cfg_scale)));
        if let Some(s) = req.sampler.as_deref().or(d.sampler) {
            push("--sampling-method", s.to_owned());
        }
        push("-s", req.seed.to_string());
        push("-o", path(out));
        if cfg.offload_to_cpu {
            a.push("--offload-to-cpu".into());
        }
        if cfg.clip_on_cpu {
            a.push("--clip-on-cpu".into());
        }
        if cfg.vae_tiling {
            a.push("--vae-tiling".into());
        }
        a.extend(cfg.extra_args.iter().cloned());
        a
    }
}

fn pick<'a>(list: impl Iterator<Item = &'a ModelFile>, prefer: &[&str]) -> Option<PathBuf> {
    let items: Vec<&ModelFile> = list.collect();
    prefer
        .iter()
        .find_map(|p| items.iter().find(|m| m.file_name.to_ascii_lowercase().contains(p)))
        .or(items.first())
        .map(|m| m.path.clone())
}

impl Registry {
    /// Builds the kit for a model (or the first complete one when `query` is empty).
    pub fn diffusion_kit(&self, query: &str) -> Option<DiffusionKit> {
        let build = |m: &ModelFile| {
            let family = m.family.unwrap_or(DiffusionFamily::Unknown);
            let needs = family.needs();
            let enc = |k: EncoderKind| self.encoders.iter().filter(move |e| e.encoder == Some(k));
            let vae = needs.contains(&Need::Vae).then(|| match family {
                DiffusionFamily::QwenImage => pick(self.vaes.iter(), &["qwen"]),
                _ => pick(self.vaes.iter().filter(|v| !v.file_name.to_ascii_lowercase().contains("qwen")), &["ae.", "flux"]),
            });
            let llm = needs.contains(&Need::Llm).then(|| match family {
                DiffusionFamily::QwenImage => pick(enc(EncoderKind::Llm), &["2.5", "2_5", "vl"]),
                _ => pick(enc(EncoderKind::Llm).filter(|e| !e.file_name.to_ascii_lowercase().contains("vl")), &["qwen3", "qwen_3"]),
            });
            DiffusionKit {
                model: m.clone(),
                family,
                vae: vae.flatten(),
                clip_l: needs.contains(&Need::ClipL).then(|| pick(enc(EncoderKind::ClipL), &[])).flatten(),
                clip_g: needs.contains(&Need::ClipG).then(|| pick(enc(EncoderKind::ClipG), &[])).flatten(),
                t5xxl: needs.contains(&Need::T5).then(|| pick(enc(EncoderKind::T5xxl), &["t5xxl"])).flatten(),
                llm: llm.flatten(),
            }
        };
        if query.trim().is_empty() {
            let kits: Vec<DiffusionKit> = self.diffusion.iter().map(build).collect();
            kits.iter().find(|k| k.is_complete()).cloned().or_else(|| kits.into_iter().next())
        } else {
            self.find_diffusion(query).map(build)
        }
    }
}

/// Consistent portrait prompt: the character's fixed appearance leads; mood and scene vary.
pub fn portrait_prompt(appearance: &str, mood: &str, scene: &str, style: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for p in [style, appearance, mood, scene] {
        let p = p.trim().trim_end_matches(['.', ',']);
        if !p.is_empty() {
            parts.push(p);
        }
    }
    parts.join(", ")
}
