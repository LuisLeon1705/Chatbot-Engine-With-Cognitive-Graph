# GGUF models by role

> As of **October 2026**. Repos, quantisations and sizes come from Clay-Myrrh's catalogue checked against Hugging Face (`models/localai/model_catalog.py`). Models change fast: check before downloading.

## Why two models

| | Memory curator | Roleplay actor |
|---|---|---|
| Task | Read a turn and emit `{"ops":[...]}` (structured extraction) | Speak in character, describe images |
| Output | Forced by `memory_ops.gbnf` (or the JSON Schema on Ollama) | Free text |
| Latency | Does not matter: it runs in the background | Critical (the user is waiting) |
| Context | ~2–4k tokens (prompt + active memory + 1 turn) | **4–8k is enough**: memory replaces the history |
| Temperature | 0.2 | 0.8–0.9 |
| Where | CPU (`-ngl 0`) or leftover VRAM | GPU |

Since the actor no longer drags a growing history, you can launch it with `-c 8192` instead of 32k: the KV cache stays at a fraction of the usual size and that VRAM goes to model layers.

## Memory curators (1B–4B)

| Model | Suggested GGUF | Disk | Notes |
|---|---|---|---|
| Qwen3.5 0.8B | `unsloth/Qwen3.5-0.8B-GGUF` · Q8_0 | 0.76 GB | For machines without a GPU. The grammar guarantees the format, but it is poor at telling what deserves remembering. |
| Qwen3.5 2B | `unsloth/Qwen3.5-2B-GGUF` · Q8_0 | 1.87 GB | A good starting point on the CPU. |
| **Qwen3.5 4B** | `unsloth/Qwen3.5-4B-GGUF` · UD-Q4_K_XL | 2.71 GB | **Recommended.** Turn off reasoning (`disable_thinking`, already on in the `curator` preset). |
| Gemma 4 E2B | `unsloth/gemma-4-E2B-it-GGUF` · UD-Q4_K_XL | 2.97 GB | Very natural labels in Spanish. |
| Gemma 4 E4B | `unsloth/gemma-4-E4B-it-GGUF` · UD-Q4_K_XL | 4.77 GB | **Tested on this machine** (Q4_K_M, Vulkan): valid JSON in 21–29 s with 190–264 tokens. Details in `PIPELINE.md`. |

My recommendation: Qwen3.5 4B or Gemma 4 E4B on the CPU. Move to the 2B if the curator takes longer than the user takes to type.

## Roleplay actors with vision (`mmproj`)

### 7B–12B (8–12 GB of VRAM)

| Model | Suggested GGUF | Disk (+ mmproj) | Notes |
|---|---|---|---|
| **Gemma 4 12B** | `unsloth/gemma-4-12b-it-GGUF` · UD-Q4_K_XL | 6.86 GB + `mmproj-F16.gguf` | **Recommended.** Very good prose and dialogue in Spanish. You already have the Q3_K_XL variant. |
| Qwen3.5 9B | `unsloth/Qwen3.5-9B-GGUF` · UD-Q4_K_XL | 5.56 GB + 0.86 GB | All-rounder, good vision; prose a bit flatter than Gemma's. |
| Gemma 4 E4B | `unsloth/gemma-4-E4B-it-GGUF` · UD-Q4_K_XL | 4.77 GB + 0.92 GB | Light actor for laptops or for sharing the GPU with a game. |
| Qwen3-VL 8B | `Qwen/Qwen3-VL-8B-Instruct-GGUF` · Q4_K_M | 4.68 GB + 0.70 GB | The best at reading images and the text in them, but weak as a narrator. Use it as a "reader" that describes the image for the actor. |
| Mistral-Nemo 12B fine-tunes | — | ~7 GB | Widely used for roleplay, **no vision**. Only if you do not need an `mmproj`. |

### 24B–31B (16–24 GB of VRAM, or MoE with CPU offload)

| Model | Suggested GGUF | Disk (+ mmproj) | Notes |
|---|---|---|---|
| **Mistral Small 3.2 24B** | `unsloth/Mistral-Small-3.2-24B-Instruct-2506-GGUF` · UD-Q4_K_XL | 13.55 GB + 0.82 GB | **Recommended.** Balanced writing and the base of many roleplay fine-tunes. If the fine-tune did not touch the vision tower, it usually accepts this same mmproj (try it). |
| Qwen3.6 27B | `unsloth/Qwen3.6-27B-GGUF` · UD-Q4_K_XL | 16.4 GB + 0.86 GB | Dense and very coherent in long stories. Slow if it does not fit entirely in VRAM. |
| Gemma 4 31B | `unsloth/gemma-4-31B-it-GGUF` · UD-Q4_K_XL | 17.53 GB + 1.12 GB | The best prose on the list. Needs 24 GB to run smoothly. |
| Qwen3.6 35B-A3B (MoE) | `unsloth/Qwen3.6-35B-A3B-GGUF` · IQ4_XS | 16.51 GB + 0.84 GB | ~30B quality at ~3B speed. With little VRAM, send the experts to the CPU (`--n-cpu-moe`). You already have it downloaded. |

## Recommended launch (two `llama-server`s)

```bash
llama-server -m gemma-4-12b-it-UD-Q4_K_XL.gguf --mmproj mmproj-F16.gguf -c 8192 -ngl 99 --jinja --port 8080
```

```bash
llama-server -m Qwen3.5-4B-UD-Q4_K_XL.gguf -c 4096 -ngl 0 -t 6 --jinja --port 8081
```

Then, in Rust:

```rust
let actor = LlmClient::new(LlmEndpoint::actor(Backend::LlamaServer, "http://127.0.0.1:8080", "actor"))?;
let curator = LlmClient::new(LlmEndpoint::curator(Backend::LlamaServer, "http://127.0.0.1:8081", "curator"))?;
```

**Ollama** works too (`Backend::Ollama`): the curator gets the JSON Schema in `format`, since Ollama does not accept GBNF. With vision models imported by hand (GGUF + mmproj) Ollama is more work than `llama-server`, so for the actor I recommend `llama-server`.

## Downloads for this machine (RTX 3070, 8 GB)

Everything goes inside `models/`. Run them from the `Chats` folder in PowerShell; `-C -` resumes if a download is cut.

**Vision for Gemma 4 12B** (0.18 GB). The text model is already complete; only the projector is missing:

```powershell
curl.exe -L -C - -o models/mmproj/mmproj-gemma-4-12b-it-F16.gguf https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/main/mmproj-F16.gguf
```

**Image engine: stable-diffusion.cpp** (Vulkan build, 30 MB):

```powershell
curl.exe -L -o models/sd-vulkan.zip https://github.com/leejet/stable-diffusion.cpp/releases/download/master-964-1d0a58a/sd-master-1d0a58a-bin-win-vulkan-x64.zip
```

```powershell
Expand-Archive models/sd-vulkan.zip -DestinationPath models/bin
```

If `sd-cli.exe` ends up in a subfolder, put its path in `bin.sd_cli` in `config/models.toml`. There is also a CUDA 12 build, faster on NVIDIA but 338 MB plus a separate 564 MB `cudart`.

**Image model: Z-Image Turbo** (8 steps; its docs say it works with 4 GB of VRAM). It needs three files:

```powershell
curl.exe -L -C - -o models/diffusion/z_image_turbo-Q4_K.gguf https://huggingface.co/leejet/Z-Image-Turbo-GGUF/resolve/main/z_image_turbo-Q4_K.gguf
```

```powershell
curl.exe -L -C - --create-dirs -o models/diffusion/encoders/Qwen3-4B-Instruct-2507-Q4_K_M.gguf https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf
```

```powershell
curl.exe -L -C - --create-dirs -o models/diffusion/vae/ae.safetensors https://huggingface.co/Comfy-Org/z_image_turbo/resolve/main/split_files/vae/ae.safetensors
```

Sizes: 3.86 GB + 2.50 GB + 0.34 GB. Afterwards, `chats models` should show the image model as "ready". (The old folder names `difusion/` and `fusion/` are still recognised.)

**Gemma 4 12B vision:** its `mmproj` uses the `gemma4uv` projector, which needs llama.cpp b11633 or later. It is installed in `models/bin/llama/`, with its `cudart` 13.4. With an older build, the supervisor retries without vision: chat works, but `chats reference` and "Describe with vision" are disabled.

Vision settings, measured on the RTX 3070:
- **`-ub 1152`:** with the default (512) the image was cut down to 512 tokens and details were lost.
- **Projector on the CPU** (`mmproj_gpu = false`): 13.1 tokens/s, against 11.7 with the projector on the GPU and 14.3 without vision.

Gemma 4 12B at Q3 describes ears, palette and garments well, but misses details such as eye colour or hair length. For references use a dedicated reader (below) and always review the result in the Appearance tab.

## Vision reader (`[reader]`)

Describing references is not urgent and needs no prose, so a model larger than the actor can do it. The `reader` slot only loads for that: it unloads the actor first and unloads itself when done. Comparison of 15/10/2026 with Kal'tsit's 3 official references (RTX 3070, llama.cpp b11633, MoE with experts on the CPU through `--fit on`):

| Reader | Time | Result |
|---|---|---|
| Gemma 4 12B Q3_K_XL (actor) | ~40 s, 1 image | Red or closed eyes, long hair, "kimono". |
| Qwen3-VL 30B-A3B Q4_K_M | 60–77 s | Green eyes, stethoscope and watch right; "long" hair, a tail and a "WHO symbol" invented. |
| **Qwen3.6 35B-A3B UD-IQ4_XS** | **51 s** | **The best:** shoulder-length hair, one-shoulder lime dress, white coat, collar, boots, watch. With Amiya and the Doctor, 42–48 s. |

Configuration used (in `config/models.toml`):

```toml
extra_models_dirs = ["C:/Users/leonp/OneDrive/Escritorio/models/models"]

[reader]
model = "Qwen3.6-35B-A3B"
ctx = 16384
port = 8082
gpu = "auto"
vision = true
mmproj_gpu = true
extra_args = ["-ub", "2048", "--image-max-tokens", "1536", "-ctk", "q8_0", "-ctv", "q8_0"]
```

Pin `actor.model` too: with extra folders, the automatic suggestion could pick another actor.

The reader writes the aspects in the app's language (the instructions are in `crates/i18n/prompts/<lang>/appearance_reader.md`).

**Shared GPU:** if a game or Wallpaper Engine is using the GPU, the actor's speed will vary a lot; it is not a bug. Running the curator on the CPU keeps the two models from fighting over VRAM.
