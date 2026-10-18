use std::path::{Path, PathBuf};

use model_hub::config::ImageConfig;
use model_hub::gguf::{MetaValue, write_header};
use model_hub::registry::{DiffusionFamily, EncoderKind};
use model_hub::*;

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    p
}

fn s(v: &str) -> MetaValue {
    MetaValue::Str(v.into())
}

/// Synthetic `models/` folder with the same layout as the real one.
fn fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("model-hub-reg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let llm = |arch: &str, emb: i64| write_header(&[("general.architecture", s(arch)), (&format!("{arch}.embedding_length"), MetaValue::Int(emb))], &[]);
    let mmproj = |dim: i64| {
        write_header(&[("general.architecture", s("clip")), ("general.type", s("mmproj")), ("clip.vision.projection_dim", MetaValue::Int(dim))], &[])
    };
    put(&root, "llm/gemma-4-12B-it-Q3_K_XL.gguf", &llm("gemma4", 3840));
    put(&root, "llm/Qwen3.5-4B-UD-Q4_K_XL.gguf", &llm("qwen35", 2560));
    put(&root, "mmproj/mmproj-gemma-4-12b-it-F16.gguf", &mmproj(3840));
    // Same base name but a different dimension: it must not pair with anything.
    put(&root, "mmproj/mmproj-Qwen3.5-4B-wrong-F16.gguf", &mmproj(9999));
    put(&root, "diffusion/z_image_turbo-Q4_K.gguf", &write_header(&[], &["noise_refiner.0.w", "context_refiner.0.w"]));
    put(&root, "diffusion/vae/ae.safetensors", &model_hub::safetensors::write_header(&["decoder.conv_in.weight"]));
    put(&root, "diffusion/encoders/Qwen3-4B-Instruct-2507-Q4_K_M.gguf", &llm("qwen3", 2560));
    put(&root, "fusion/flux1-schnell-q4_k.gguf", &write_header(&[], &["double_blocks.0.img_attn.qkv.weight"]));
    put(&root, "diffusion/broken.gguf", b"this is not a gguf");
    root
}

#[test]
fn scan_classifies_pairs_and_builds_kits() {
    let root = fixture("scan");
    let reg = Registry::scan(&root);

    assert_eq!(reg.llms.len(), 2);
    let gemma = reg.find_llm("gemma-4-12b").unwrap();
    assert_eq!(gemma.params_b, Some(12.0));
    assert_eq!(gemma.quant.as_deref(), Some("Q3_K_XL"));
    assert!(gemma.mmproj.as_ref().unwrap().ends_with("mmproj-gemma-4-12b-it-F16.gguf"));
    assert!(reg.find_llm("qwen3.5").unwrap().mmproj.is_none(), "different dimensions: no pairing");

    let (actor, curator) = reg.suggest(7500);
    assert_eq!(actor.unwrap().file_name, "gemma-4-12B-it-Q3_K_XL.gguf");
    assert_eq!(curator.unwrap().file_name, "Qwen3.5-4B-UD-Q4_K_XL.gguf");

    assert_eq!(reg.vaes.len(), 1);
    assert_eq!(reg.encoders[0].encoder, Some(EncoderKind::Llm));
    assert_eq!(reg.warnings.len(), 1, "{:?}", reg.warnings);
    let fams: Vec<_> = reg.diffusion.iter().map(|d| d.family.unwrap()).collect();
    assert_eq!(fams, [DiffusionFamily::Flux, DiffusionFamily::ZImage]);

    let kit = reg.diffusion_kit("").unwrap();
    assert_eq!(kit.family, DiffusionFamily::ZImage, "the first COMPLETE one (Flux lacks clip_l and t5xxl)");
    assert!(kit.is_complete());
    let flux = reg.diffusion_kit("flux").unwrap();
    assert_eq!(flux.missing().len(), 2, "{:?}", flux.missing());

    let req = ImageRequest::portrait(portrait_prompt("elf, silver hair", "smiling", "library in ruins", "illustration"), kit.family);
    let args = kit.args(&req, Path::new("out.png"), &ImageConfig::default()).join(" ");
    for want in ["--diffusion-model", "--vae", "--llm", "--steps 8", "--cfg-scale 1", "-W 768 -H 1024", "--offload-to-cpu", "-o out.png"] {
        assert!(args.contains(want), "{want} missing in {args}");
    }
    assert!(args.contains("illustration, elf, silver hair, smiling, library in ruins"));
    assert!(!args.contains(" -n "), "Z-Image Turbo (CFG 1) uses no negative prompt");
}

#[test]
fn supervisor_specs_share_actor_when_no_curator_model() {
    let root = fixture("spec");
    let cfg = HubConfig { models_dir: root.clone(), ..Default::default() };
    let sup = Supervisor::new(cfg.clone(), root.join("logs"));
    assert!(sup.curator_shares_actor());
    let spec = sup.spec(Slot::Curator).unwrap();
    assert_eq!(spec.parallel, 2, "the actor also serves the curator");
    assert_eq!(spec.ctx, 6144 * 2);
    assert!(spec.args().join(" ").contains("-ctk q8_0 -ctv q8_0"), "quantised KV by default");
    assert!(spec.mmproj.is_some());
    assert_eq!(sup.base_url(Slot::Curator), sup.base_url(Slot::Actor));

    let mut cfg2 = cfg;
    cfg2.curator.model = "qwen3.5-4b".into();
    let sup2 = Supervisor::new(cfg2, root.join("logs"));
    let cur = sup2.spec(Slot::Curator).unwrap();
    assert_eq!((cur.port, cur.parallel, cur.gpu_mb), (8081, 1, 0));
    assert!(cur.args().contains(&"-ngl".to_string()));
}

#[test]
fn reader_comes_from_extra_dirs_and_falls_back_to_actor() {
    let root = fixture("reader");
    let extra = std::env::temp_dir().join(format!("model-hub-reg-{}-reader-extra", std::process::id()));
    let _ = std::fs::remove_dir_all(&extra);
    let header = |arch: &str, emb: i64| write_header(&[("general.architecture", s(arch)), (&format!("{arch}.embedding_length"), MetaValue::Int(emb))], &[]);
    put(&extra, "vision/Qwen3VL-30B-A3B-Instruct-Q4_K_M.gguf", &header("qwen3vlmoe", 2048));
    put(
        &extra,
        "mmproj/mmproj-Qwen3VL-30B-A3B-Instruct-Q8_0.gguf",
        &write_header(&[("general.architecture", s("clip")), ("general.type", s("mmproj")), ("clip.vision.projection_dim", MetaValue::Int(2048))], &[]),
    );
    // The same file in both folders counts once.
    put(&extra, "llm/gemma-4-12B-it-Q3_K_XL.gguf", &header("gemma4", 3840));

    let mut cfg = HubConfig { models_dir: root.clone(), extra_models_dirs: vec![extra.clone()], ..Default::default() };
    let sup = Supervisor::new(cfg.clone(), root.join("logs"));
    assert!(!sup.has_reader());
    assert_eq!(sup.spec(Slot::Reader).unwrap().alias, sup.spec(Slot::Actor).unwrap().alias, "without a reader, the actor describes");
    assert_eq!(sup.base_url(Slot::Reader), sup.base_url(Slot::Actor));
    assert_eq!(sup.registry().llms.iter().filter(|m| m.file_name.starts_with("gemma-4-12B")).count(), 1);

    cfg.reader.model = "qwen3vl-30b".into();
    let sup = Supervisor::new(cfg, root.join("logs"));
    let reader = sup.spec(Slot::Reader).unwrap();
    assert_eq!(reader.alias, "Qwen3VL-30B-A3B-Instruct-Q4_K_M");
    assert!(reader.mmproj.as_ref().unwrap().ends_with("mmproj-Qwen3VL-30B-A3B-Instruct-Q8_0.gguf"));
    assert_eq!(reader.port, 8082);
    let args = reader.args().join(" ");
    assert!(args.contains("--mmproj") && !args.contains("--no-mmproj-offload"), "the reader keeps the projector on the GPU: {args}");
}
