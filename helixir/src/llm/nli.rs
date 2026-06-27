//! Local NLI judge (#55 groundwork): a tiny DeBERTa-v3 cross-encoder that
//! classifies a sentence pair as contradiction / entailment / neutral. It is the
//! contradiction-safe judge for paraphrase merging — embeddings (and pure
//! similarity models) cannot tell "prefer dark" from "prefer light" (~0.98
//! cosine, opposite meaning), so a real 3-way NLI head makes the merge decision.
//!
//! Fully local, CPU, no Python and no API: ONNX Runtime via `ort` + a Rust
//! `tokenizers` SentencePiece tokenizer. Model is ~90 MB (arm64 int8 ONNX),
//! downloaded only for the collective/insights tiers. Label order is fixed by
//! the model config: 0=contradiction, 1=entailment, 2=neutral.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NliLabel {
    Contradiction,
    Entailment,
    Neutral,
}

impl NliLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            NliLabel::Contradiction => "contradiction",
            NliLabel::Entailment => "entailment",
            NliLabel::Neutral => "neutral",
        }
    }
}

/// A loaded NLI cross-encoder. Construct once (load is the expensive part), then
/// `classify` per pair.
pub struct NliJudge {
    session: Session,
    tokenizer: Tokenizer,
}

impl NliJudge {
    /// Default install location for the downloaded model.
    pub fn default_dir() -> std::path::PathBuf {
        dirs_home().join(".helixir/models/nli")
    }

    /// Load `model.onnx` + `tokenizer.json` from `dir`.
    pub fn load(dir: &Path) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("tokenizer load: {e}"))?;
        let session = Session::builder()
            .context("ort session builder")?
            .commit_from_file(dir.join("model.onnx"))
            .context("load model.onnx")?;
        Ok(Self { session, tokenizer })
    }

    /// The model's actual input tensor names (introspected, not assumed).
    pub fn input_names(&self) -> Vec<String> {
        self.session.inputs().iter().map(|o| o.name().to_string()).collect()
    }

    /// The model's actual output tensor names.
    pub fn output_names(&self) -> Vec<String> {
        self.session.outputs().iter().map(|o| o.name().to_string()).collect()
    }

    /// Classify the (premise, hypothesis) pair. Returns the winning label and the
    /// softmax scores `[contradiction, entailment, neutral]`.
    pub fn classify(&mut self, premise: &str, hypothesis: &str) -> Result<(NliLabel, [f32; 3])> {
        let enc = self
            .tokenizer
            .encode((premise, hypothesis), true)
            .map_err(|e| anyhow::anyhow!("encode: {e}"))?;
        let len = enc.get_ids().len();
        let ids: Vec<i64> = enc.get_ids().iter().map(|&x| i64::from(x)).collect();
        let mask: Vec<i64> = enc.get_attention_mask().iter().map(|&x| i64::from(x)).collect();

        // This model's graph declares exactly input_ids + attention_mask (no
        // token_type_ids) and a single `logits` output of shape [1, 3] — verified
        // against the ONNX graph, not assumed. See `inputs()`/`outputs()`.
        let inputs = ort::inputs! {
            "input_ids" => Tensor::from_array(([1_usize, len], ids))?,
            "attention_mask" => Tensor::from_array(([1_usize, len], mask))?,
        };

        let outputs = self.session.run(inputs)?;
        let (shape, logits) = outputs["logits"].try_extract_tensor::<f32>()?;
        anyhow::ensure!(
            logits.len() >= 3,
            "NLI output shape {shape:?} — expected 3 logits, got {}",
            logits.len()
        );
        let raw = [logits[0], logits[1], logits[2]];
        let scores = softmax3(raw);
        let label = match argmax3(&scores) {
            0 => NliLabel::Contradiction,
            1 => NliLabel::Entailment,
            _ => NliLabel::Neutral,
        };
        Ok((label, scores))
    }

    /// Symmetric "same fact?" decision for paraphrase merging. Safe by design:
    /// runs both directions and treats it as a merge candidate ONLY if neither
    /// direction is a contradiction and at least one is entailment. Never merges
    /// when contradiction fires either way (the catastrophic case).
    pub fn is_same_fact(&mut self, a: &str, b: &str) -> Result<bool> {
        let (lab_ab, _) = self.classify(a, b)?;
        let (lab_ba, _) = self.classify(b, a)?;
        if lab_ab == NliLabel::Contradiction || lab_ba == NliLabel::Contradiction {
            return Ok(false);
        }
        Ok(lab_ab == NliLabel::Entailment || lab_ba == NliLabel::Entailment)
    }
}

fn softmax3(x: [f32; 3]) -> [f32; 3] {
    let m = x.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let e = [
        (x[0] - m).exp(),
        (x[1] - m).exp(),
        (x[2] - m).exp(),
    ];
    let s = e[0] + e[1] + e[2];
    [e[0] / s, e[1] / s, e[2] / s]
}

fn argmax3(x: &[f32; 3]) -> usize {
    let mut best = 0;
    for i in 1..3 {
        if x[i] > x[best] {
            best = i;
        }
    }
    best
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_is_an_onnx_path() {
        assert!(pick_onnx_variant().ends_with(".onnx"));
        assert!(pick_onnx_variant().starts_with("onnx/"));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_picks_arm64_variant() {
        assert_eq!(pick_onnx_variant(), "onnx/model_qint8_arm64.onnx");
    }

    // Safety-critical: the paraphrase backstop must NEVER merge opposite
    // preferences ("prefer dark" vs "prefer light"). This used to be `#[ignore]`
    // and so never ran; now it runs whenever the model is present (CI installs
    // it, or a dev ran `helixir model download`) and skips cleanly otherwise —
    // a catastrophic-case guard that no longer silently sits dormant.
    #[test]
    fn nli_is_contradiction_safe() {
        if !status().installed {
            eprintln!("SKIP nli_is_contradiction_safe: NLI model not downloaded");
            return;
        }
        let mut j = NliJudge::load(&NliJudge::default_dir()).expect("load NLI model");
        let dark = "I prefer the dark theme in every editor.";
        let light = "I prefer the light theme in every editor.";
        assert_eq!(
            j.classify(dark, light).unwrap().0,
            NliLabel::Contradiction,
            "opposite preferences must be a contradiction"
        );
        assert!(
            !j.is_same_fact(dark, light).unwrap(),
            "opposites must never merge"
        );
        assert!(
            j.is_same_fact("I love pizza.", "Pizza is my favourite food.")
                .unwrap(),
            "paraphrases must be the same fact"
        );
    }
}

// ----------------------------------------------------------------------------
// Model acquisition. The repo ships the DOWNLOADER, never the weights: it picks
// the ONNX quantization variant matching the host arch/CPU and fetches it from
// HuggingFace into ~/.helixir/models/nli. All variants are the same 70M model;
// only the quantization target differs. fp32 `model.onnx` is the universal
// fallback that runs anywhere.
// ----------------------------------------------------------------------------

const NLI_REPO: &str = "cross-encoder/nli-deberta-v3-xsmall";
const HF_BASE: &str = "https://huggingface.co";

/// The remote ONNX path best matching this machine's arch / CPU features.
#[must_use]
pub fn pick_onnx_variant() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm" => "onnx/model_qint8_arm64.onnx",
        "x86_64" => x86_variant(),
        // Anything exotic: the portable fp32 graph.
        _ => "onnx/model.onnx",
    }
}

#[cfg(target_arch = "x86_64")]
fn x86_variant() -> &'static str {
    if std::arch::is_x86_feature_detected!("avx512vnni") {
        "onnx/model_qint8_avx512_vnni.onnx"
    } else if std::arch::is_x86_feature_detected!("avx512f") {
        "onnx/model_qint8_avx512.onnx"
    } else if std::arch::is_x86_feature_detected!("avx2") {
        "onnx/model_quint8_avx2.onnx"
    } else {
        "onnx/model.onnx"
    }
}
#[cfg(not(target_arch = "x86_64"))]
fn x86_variant() -> &'static str {
    "onnx/model.onnx"
}

/// Human label of the host (e.g. "aarch64/macos") for status output.
#[must_use]
pub fn host_label() -> String {
    format!("{}/{}", std::env::consts::ARCH, std::env::consts::OS)
}

pub struct ModelStatus {
    pub dir: PathBuf,
    pub installed: bool,
    pub onnx_bytes: u64,
    pub variant_for_host: &'static str,
    pub host: String,
}

/// Inspect what's installed on disk for this host.
#[must_use]
pub fn status() -> ModelStatus {
    let dir = NliJudge::default_dir();
    let onnx = dir.join("model.onnx");
    let tok = dir.join("tokenizer.json");
    let onnx_bytes = std::fs::metadata(&onnx).map(|m| m.len()).unwrap_or(0);
    ModelStatus {
        installed: onnx.exists() && tok.exists(),
        onnx_bytes,
        variant_for_host: pick_onnx_variant(),
        host: host_label(),
        dir,
    }
}

/// Download the host-appropriate ONNX variant + tokenizer + config into the
/// model dir. Skips files already present unless `force`. Returns bytes fetched.
/// The platform-specific ONNX is always saved locally as `model.onnx` so the
/// loader path is uniform.
pub async fn download(force: bool) -> Result<u64> {
    let dir = NliJudge::default_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let variant = pick_onnx_variant();
    // (remote path on HF, local filename)
    let files = [
        (variant, "model.onnx"),
        ("tokenizer.json", "tokenizer.json"),
        ("config.json", "config.json"),
    ];
    // Async client — the CLI runs inside a tokio runtime, so reqwest::blocking
    // would panic ("cannot drop a runtime in an async context").
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .context("http client")?;
    let mut total = 0u64;
    for (remote, local) in files {
        let dest = dir.join(local);
        if dest.exists() && !force {
            continue;
        }
        let url = format!("{HF_BASE}/{NLI_REPO}/resolve/main/{remote}");
        let bytes = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("download {remote}"))?
            .bytes()
            .await
            .context("read body")?;
        std::fs::write(&dest, &bytes).with_context(|| format!("write {}", dest.display()))?;
        total += bytes.len() as u64;
    }
    Ok(total)
}
