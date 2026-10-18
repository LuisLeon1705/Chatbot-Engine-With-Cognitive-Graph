//! Registry of the models in the `models/` folder:
//!
//! ```text
//! models/
//! ├─ llm/         GGUF text models (actor and curator)
//! ├─ vision/      vision models used as readers (treated as LLMs)
//! ├─ mmproj/      vision projectors; they pair with their LLM by themselves
//! └─ diffusion/   image generation (stable-diffusion.cpp)
//!    ├─ vae/        (optional: also recognised by name: ae.safetensors, *vae*)
//!    ├─ encoders/   clip_l, clip_g, t5xxl, Qwen3-4B… (or by name)
//!    └─ loras/
//! ```
//!
//! The mmproj ↔ LLM pairing uses the normalised name (no quantisation or suffixes) and
//! **checks** it against the metadata: the mmproj's `clip.vision.projection_dim` must
//! match the model's `<arch>.embedding_length`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::gguf::{GgufInfo, read_gguf};

pub const LLM_DIR: &str = "llm";
/// Folder of vision models ("readers"); they are treated as LLMs.
pub const VISION_DIR: &str = "vision";
pub const MMPROJ_DIR: &str = "mmproj";
/// Main name of the diffusion folder and the old names still accepted.
pub const DIFFUSION_DIRS: [&str; 3] = ["diffusion", "difusion", "fusion"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Gguf,
    Safetensors,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Llm,
    Mmproj,
    Diffusion,
    Vae,
    TextEncoder,
    Lora,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffusionFamily {
    Sd15,
    Sdxl,
    Sd3,
    Flux,
    Chroma,
    ZImage,
    QwenImage,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EncoderKind {
    ClipL,
    ClipG,
    T5xxl,
    /// LLM-based encoder (Qwen3-4B for Z-Image, Qwen2.5-VL for Qwen-Image).
    Llm,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub format: Format,
    pub role: Role,
    pub arch: Option<String>,
    pub name: Option<String>,
    pub size_label: Option<String>,
    /// Total parameters (billions). For "E4B" it is the effective count.
    pub params_b: Option<f32>,
    /// MoE: active parameters per token.
    pub active_params_b: Option<f32>,
    pub quant: Option<String>,
    pub context_length: Option<i64>,
    pub embedding_length: Option<i64>,
    /// mmproj: output dimension of the projection.
    pub projection_dim: Option<i64>,
    pub family: Option<DiffusionFamily>,
    pub encoder: Option<EncoderKind>,
    /// LLM: paired mmproj (vision available).
    pub mmproj: Option<PathBuf>,
    /// Normalised key for pairing.
    pub key: String,
}

impl ModelFile {
    pub fn size_mb(&self) -> u64 {
        self.size_bytes / (1024 * 1024)
    }

    pub fn stem(&self) -> &str {
        self.file_name.rsplit_once('.').map_or(&self.file_name, |(s, _)| s)
    }

    pub fn has_vision(&self) -> bool {
        self.mmproj.is_some()
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Registry {
    pub root: PathBuf,
    pub llms: Vec<ModelFile>,
    pub mmprojs: Vec<ModelFile>,
    pub diffusion: Vec<ModelFile>,
    pub vaes: Vec<ModelFile>,
    pub encoders: Vec<ModelFile>,
    pub loras: Vec<ModelFile>,
    pub warnings: Vec<String>,
}

const DROP_TOKENS: &[&str] = &[
    "mmproj", "instruct", "it", "chat", "gguf", "ud", "imat", "imatrix", "unsloth", "bartowski", "k", "m", "s", "l",
    "xl", "xs", "xxs", "nl", "f16", "f32", "bf16", "fp16", "fp8", "0", "1", "model",
];

/// Pairing key: lowercase, without quantisation, suffixes or separators.
/// `gemma-4-12B-it-Q3_K_XL` and `mmproj-gemma-4-12b-it-F16` → `gemma412b`.
pub fn pair_key(stem: &str) -> String {
    stem.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .filter(|t| !DROP_TOKENS.contains(t))
        .filter(|t| !is_quant_token(t))
        .collect()
}

fn is_quant_token(t: &str) -> bool {
    let b = t.as_bytes();
    (b.len() >= 2 && b[0] == b'q' && b[1].is_ascii_digit() && b.len() <= 3)
        || (b.len() >= 3 && t.starts_with("iq") && b[2].is_ascii_digit() && b.len() <= 4)
}

/// "12B" → (12, None); "35B-A3B" → (35, Some(3)); "E4B" → (4, None); "478M" → (0.478, None).
pub fn parse_size_label(label: &str) -> (Option<f32>, Option<f32>) {
    let l = label.to_ascii_uppercase();
    let mut parts = l.split('-');
    let num = |s: &str| -> Option<f32> {
        let s = s.trim_start_matches(['E', 'A']);
        if let Some(n) = s.strip_suffix('B') {
            n.parse().ok()
        } else if let Some(n) = s.strip_suffix('M') {
            n.parse::<f32>().ok().map(|m| m / 1000.0)
        } else {
            None
        }
    };
    let total = parts.next().and_then(num);
    let active = parts.find(|p| p.starts_with('A')).and_then(num);
    (total, active)
}

/// Size deduced from the file name ("…-12B-…", "e4b", "35b-a3b").
fn size_from_name(stem: &str) -> Option<String> {
    let tokens: Vec<String> = stem.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric() && c != '.').map(str::to_owned).collect();
    for (i, t) in tokens.iter().enumerate() {
        let body = t.trim_start_matches('e');
        if t.ends_with('b') && body.len() > 1 && body[..body.len() - 1].parse::<f32>().is_ok() {
            let mut label = t.to_ascii_uppercase();
            if let Some(next) = tokens.get(i + 1).filter(|n| n.starts_with('a') && n.ends_with('b') && n[1..n.len() - 1].parse::<f32>().is_ok()) {
                label = format!("{label}-{}", next.to_ascii_uppercase());
            }
            return Some(label);
        }
    }
    None
}

fn quant_from_name(stem: &str) -> Option<String> {
    let up = stem.to_ascii_uppercase();
    let tokens: Vec<&str> = up.split(['-', '.']).collect();
    tokens
        .iter()
        .rposition(|t| {
            let t = t.trim_start_matches("UD_");
            (t.starts_with('Q') || t.starts_with("IQ")) && t.chars().nth(if t.starts_with("IQ") { 2 } else { 1 }).is_some_and(|c| c.is_ascii_digit())
                || matches!(t, "F16" | "F32" | "BF16")
        })
        .map(|i| {
            let q = tokens[i];
            if i > 0 && tokens[i - 1] == "UD" { format!("UD-{q}") } else { q.to_owned() }
        })
}

const LLM_ARCHS_AS_ENCODER: &[&str] = &["qwen3", "qwen2", "qwen2vl", "qwen25vl", "llama", "t5", "t5encoder", "gemma2", "gemma3", "mistral3"];

fn family_from_name(lower: &str) -> Option<DiffusionFamily> {
    use DiffusionFamily::*;
    let has = |s: &str| lower.contains(s);
    Some(if has("z_image") || has("z-image") || has("zimage") {
        ZImage
    } else if has("qwen_image") || has("qwen-image") {
        QwenImage
    } else if has("chroma") {
        Chroma
    } else if has("flux") {
        Flux
    } else if has("sd3") || has("sd_3") {
        Sd3
    } else if has("sdxl") || has("sd_xl") || has("illustrious") || has("pony") || has("noob") {
        Sdxl
    } else if has("v1-5") || has("sd15") || has("sd1.5") || has("sd-v1") {
        Sd15
    } else {
        return None;
    })
}

fn family_from_arch(arch: &str) -> Option<DiffusionFamily> {
    use DiffusionFamily::*;
    Some(match arch {
        "flux" => Flux,
        "sd3" => Sd3,
        "sdxl" => Sdxl,
        "sd1" => Sd15,
        "chroma" => Chroma,
        "qwen_image" => QwenImage,
        "z_image" | "zimage" => ZImage,
        _ => return None,
    })
}

/// Family from the tensor names.
pub fn family_from_tensors(names: &[String]) -> DiffusionFamily {
    use DiffusionFamily::*;
    let strip = |n: &str| n.trim_start_matches("model.diffusion_model.").to_owned();
    let names: Vec<String> = names.iter().map(|n| strip(n)).collect();
    let any = |f: &dyn Fn(&str) -> bool| names.iter().any(|n| f(n));
    if any(&|n| n.starts_with("double_blocks.")) {
        if any(&|n| n.contains("distilled_guidance_layer")) { Chroma } else { Flux }
    } else if any(&|n| n.starts_with("joint_blocks.")) {
        Sd3
    } else if any(&|n| n.contains("noise_refiner")) && any(&|n| n.contains("context_refiner")) {
        ZImage
    } else if any(&|n| n.starts_with("transformer_blocks.")) && any(&|n| n.contains("img_mod")) {
        QwenImage
    } else if any(&|n| n.contains("conditioner.embedders.1")) || (any(&|n| n.starts_with("label_emb")) && any(&|n| n.starts_with("input_blocks"))) {
        Sdxl
    } else if any(&|n| n.starts_with("input_blocks")) {
        Sd15
    } else {
        Unknown
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn format_of(path: &Path) -> Format {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("gguf") => Format::Gguf,
        Some("safetensors" | "sft") => Format::Safetensors,
        _ => Format::Other,
    }
}

impl Registry {
    /// Scans `root` (the `models/` folder). Unreadable files go to `warnings`.
    pub fn scan(root: impl Into<PathBuf>) -> Self {
        Self::scan_all(root, &[])
    }

    /// Scans the main folder and others with the same layout.
    pub fn scan_all(root: impl Into<PathBuf>, extra: &[PathBuf]) -> Self {
        let root = root.into();
        let mut reg = Registry { root: root.clone(), ..Default::default() };

        let mut files = Vec::new();
        for base in std::iter::once(&root).chain(extra) {
            walk(&base.join(LLM_DIR), &mut files);
            walk(&base.join(VISION_DIR), &mut files);
            walk(&base.join(MMPROJ_DIR), &mut files);
        }
        // The same file in two folders counts once (the main folder wins).
        let mut seen = std::collections::HashSet::new();
        for f in files {
            if format_of(&f) != Format::Gguf || !seen.insert(f.file_name().map(|n| n.to_ascii_lowercase())) {
                continue;
            }
            match reg.inspect(&f, false) {
                Ok((mut m, info)) => {
                    let in_mmproj_dir = f.parent().is_some_and(|p| p.ends_with(MMPROJ_DIR));
                    if info.is_mmproj() || in_mmproj_dir {
                        m.role = Role::Mmproj;
                        reg.mmprojs.push(m);
                    } else {
                        reg.llms.push(m);
                    }
                }
                Err(e) => reg.warnings.push(format!("{}: {e}", f.display())),
            }
        }

        let mut files = Vec::new();
        for d in DIFFUSION_DIRS {
            walk(&root.join(d), &mut files);
        }
        for f in files {
            if format_of(&f) == Format::Other && f.extension().is_none_or(|e| e != "ckpt") {
                continue;
            }
            reg.classify_diffusion(&f);
        }

        reg.pair_mmprojs();
        for list in [&mut reg.llms, &mut reg.mmprojs, &mut reg.diffusion, &mut reg.vaes, &mut reg.encoders, &mut reg.loras] {
            list.sort_by_key(|m| m.file_name.to_lowercase());
        }
        reg
    }

    fn inspect(&self, path: &Path, with_tensors: bool) -> Result<(ModelFile, GgufInfo), String> {
        let info = read_gguf(path, with_tensors).map_err(|e| e.to_string())?;
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let stem = file_name.rsplit_once('.').map_or(file_name.as_str(), |(s, _)| s).to_owned();
        let size_label = info.str("general.size_label").map(str::to_owned).or_else(|| size_from_name(&stem));
        // The file name is usually more reliable than the metadata for "E4B" or "35B-A3B".
        let size_for_params = size_from_name(&stem).or_else(|| size_label.clone());
        let (params_b, active_params_b) = size_for_params.as_deref().map(parse_size_label).unwrap_or((None, None));
        let m = ModelFile {
            path: path.to_owned(),
            size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            format: Format::Gguf,
            role: Role::Llm,
            arch: info.architecture().map(str::to_owned),
            name: info.str("general.name").map(str::to_owned),
            size_label,
            params_b,
            active_params_b,
            quant: quant_from_name(&stem).or_else(|| info.file_type_name().map(str::to_owned)),
            context_length: info.context_length(),
            embedding_length: info.embedding_length(),
            projection_dim: info.int("clip.vision.projection_dim"),
            family: None,
            encoder: None,
            mmproj: None,
            key: pair_key(&stem),
            file_name,
        };
        Ok((m, info))
    }

    fn classify_diffusion(&mut self, path: &Path) {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let lower = file_name.to_ascii_lowercase();
        let rel = path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().to_ascii_lowercase().replace('\\', "/");
        let in_dir = |d: &str| rel.split('/').any(|c| c == d);
        let format = format_of(path);
        let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s).to_owned();

        let (mut m, tensors) = match format {
            Format::Gguf => match self.inspect(path, true) {
                Ok((m, info)) => (m, info.tensor_names),
                Err(e) => {
                    self.warnings.push(format!("{}: {e}", path.display()));
                    return;
                }
            },
            _ => {
                let m = ModelFile {
                    path: path.to_owned(),
                    file_name: file_name.clone(),
                    size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
                    format,
                    role: Role::Diffusion,
                    arch: None,
                    name: None,
                    size_label: None,
                    params_b: None,
                    active_params_b: None,
                    quant: quant_from_name(&stem),
                    context_length: None,
                    embedding_length: None,
                    projection_dim: None,
                    family: None,
                    encoder: None,
                    mmproj: None,
                    key: pair_key(&stem),
                };
                let tensors = if format == Format::Safetensors {
                    crate::safetensors::tensor_names(path).unwrap_or_else(|e| {
                        self.warnings.push(format!("{}: {e}", path.display()));
                        Vec::new()
                    })
                } else {
                    Vec::new()
                };
                (m, tensors)
            }
        };

        let is_llm_encoder = m.arch.as_deref().is_some_and(|a| LLM_ARCHS_AS_ENCODER.contains(&a));
        let encoder = if lower.contains("clip_l") {
            Some(EncoderKind::ClipL)
        } else if lower.contains("clip_g") {
            Some(EncoderKind::ClipG)
        } else if lower.contains("t5") {
            Some(EncoderKind::T5xxl)
        } else if is_llm_encoder || lower.contains("qwen") && !lower.contains("image") {
            Some(EncoderKind::Llm)
        } else {
            None
        };

        if in_dir("loras") || in_dir("lora") || lower.contains("lora") {
            m.role = Role::Lora;
            self.loras.push(m);
        } else if in_dir("vae") || stem == "ae" || lower.contains("vae") {
            m.role = Role::Vae;
            self.vaes.push(m);
        } else if in_dir("encoders") || in_dir("text_encoders") || encoder.is_some() {
            m.role = Role::TextEncoder;
            m.encoder = encoder.or(Some(EncoderKind::Llm));
            self.encoders.push(m);
        } else {
            m.role = Role::Diffusion;
            m.family = Some(
                family_from_name(&lower)
                    .or_else(|| m.arch.as_deref().and_then(family_from_arch))
                    .unwrap_or_else(|| family_from_tensors(&tensors)),
            );
            self.diffusion.push(m);
        }
    }

    fn pair_mmprojs(&mut self) {
        let mmprojs = self.mmprojs.clone();
        for llm in &mut self.llms {
            let compatible = |p: &&ModelFile| match (llm.embedding_length, p.projection_dim) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            let exact = mmprojs.iter().filter(compatible).find(|p| !p.key.is_empty() && p.key == llm.key);
            let best = exact.or_else(|| {
                // Common prefix (e.g. "qwen2vl2b" vs "qwen2vl2binstruct"); the longest wins.
                let mut cands: Vec<&ModelFile> = mmprojs
                    .iter()
                    .filter(compatible)
                    .filter(|p| p.key.len() >= 4 && (llm.key.starts_with(&p.key) || p.key.starts_with(&llm.key)))
                    .collect();
                cands.sort_by_key(|p| std::cmp::Reverse(p.key.len()));
                cands.first().copied()
            });
            llm.mmproj = best.map(|p| p.path.clone());
        }
    }

    /// Finds an LLM by a fragment of its name (case-insensitive, without separators).
    pub fn find_llm(&self, query: &str) -> Option<&ModelFile> {
        find(&self.llms, query)
    }

    pub fn find_diffusion(&self, query: &str) -> Option<&ModelFile> {
        find(&self.diffusion, query)
    }

    /// Suggested roles for the available VRAM: actor = the largest that fits (preferably
    /// with vision); curator = the smallest ≤5B that is not the actor.
    pub fn suggest(&self, vram_mb: u64) -> (Option<&ModelFile>, Option<&ModelFile>) {
        let fits = |m: &&ModelFile| m.size_mb() + 600 <= vram_mb * 115 / 100;
        let mut actors: Vec<&ModelFile> = self.llms.iter().filter(|m| m.params_b.unwrap_or(0.0) >= 6.0).filter(fits).collect();
        actors.sort_by_key(|m| std::cmp::Reverse((m.has_vision(), m.size_bytes)));
        let actor = actors.first().copied().or_else(|| self.llms.iter().filter(fits).max_by_key(|m| m.size_bytes));
        let curator = self
            .llms
            .iter()
            .filter(|m| m.params_b.is_some_and(|p| p <= 5.0) && Some(m.path.as_path()) != actor.map(|a| a.path.as_path()))
            .min_by_key(|m| m.size_bytes);
        (actor, curator)
    }
}

fn find<'a>(list: &'a [ModelFile], query: &str) -> Option<&'a ModelFile> {
    let q: String = query.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if q.is_empty() {
        return None;
    }
    let norm = |m: &ModelFile| -> String { m.file_name.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect() };
    list.iter().find(|m| norm(m).starts_with(&q)).or_else(|| list.iter().find(|m| norm(m).contains(&q)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_keys_ignore_quant_and_suffixes() {
        assert_eq!(pair_key("gemma-4-12B-it-Q3_K_XL"), "gemma412b");
        assert_eq!(pair_key("mmproj-gemma-4-12b-it-F16"), "gemma412b");
        assert_eq!(pair_key("Qwen3.6-35B-A3B-UD-IQ4_XS"), pair_key("mmproj-qwen36-35b-a3b-F16"));
        assert_eq!(pair_key("Qwen2-VL-2B-Instruct-Q4_K_M"), pair_key("Qwen2-VL-2B-mmproj-q5_1"));
        assert_eq!(pair_key("Qwen2.5-VL-7B-Instruct-UD-Q6_K_XL"), pair_key("mmproj-Qwen2.5-VL-7B-Instruct-Q8_0"));
    }

    #[test]
    fn sizes_and_quants_from_labels_and_names() {
        assert_eq!(parse_size_label("12B"), (Some(12.0), None));
        assert_eq!(parse_size_label("35B-A3B"), (Some(35.0), Some(3.0)));
        assert_eq!(parse_size_label("E4B"), (Some(4.0), None));
        assert_eq!(parse_size_label("478M").0, Some(0.478));
        assert_eq!(size_from_name("Qwen3.6-35B-A3B-UD-IQ4_XS").as_deref(), Some("35B-A3B"));
        assert_eq!(size_from_name("gemma-4-E4B-it-Q4_K_M").as_deref(), Some("E4B"));
        assert_eq!(size_from_name("Qwen3.5-9B-Q4_K_M").as_deref(), Some("9B"));
        assert_eq!(quant_from_name("gemma-4-12B-it-Q3_K_XL").as_deref(), Some("Q3_K_XL"));
        assert_eq!(quant_from_name("Qwen3.6-35B-A3B-UD-IQ4_XS").as_deref(), Some("UD-IQ4_XS"));
        assert_eq!(quant_from_name("mmproj-gemma-4-12b-it-F16").as_deref(), Some("F16"));
    }

    #[test]
    fn diffusion_families_from_tensor_names() {
        let t = |v: &[&str]| family_from_tensors(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(t(&["double_blocks.0.img_attn.qkv.weight"]), DiffusionFamily::Flux);
        assert_eq!(t(&["model.diffusion_model.joint_blocks.0.x_block.attn.qkv.weight"]), DiffusionFamily::Sd3);
        assert_eq!(t(&["noise_refiner.0.attention.qkv.weight", "context_refiner.0.attention.qkv.weight"]), DiffusionFamily::ZImage);
        assert_eq!(t(&["model.diffusion_model.input_blocks.0.0.weight", "model.diffusion_model.label_emb.0.0.weight"]), DiffusionFamily::Sdxl);
        assert_eq!(t(&["model.diffusion_model.input_blocks.0.0.weight"]), DiffusionFamily::Sd15);
        assert_eq!(t(&["foo"]), DiffusionFamily::Unknown);
    }
}
