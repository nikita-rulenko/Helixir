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

use std::path::Path;

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

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
