//! Local model management for Chats:
//! - `registry`: scans `models/{llm,vision,mmproj,diffusion}`, reads GGUF/safetensors
//!   metadata, pairs each mmproj with its LLM and detects the family of image models.
//! - `config`: `config/models.toml` (which model is the actor, the curator, the reader
//!   and the painter).
//! - `supervisor`: starts/stops `llama-server` and `sd-cli`, sharing out the VRAM.
//! - `image`: stable-diffusion.cpp arguments per family and portrait prompts.

pub mod config;
pub mod gguf;
pub mod image;
pub mod registry;
pub mod safetensors;
pub mod supervisor;

pub use config::{GpuMode, HubConfig};
pub use image::{DiffusionKit, ImageRequest, portrait_prompt};
pub use registry::{DiffusionFamily, ModelFile, Registry, Role};
pub use supervisor::{HubError, ImageOutcome, LlamaSpec, Slot, SlotStatus, Supervisor};
