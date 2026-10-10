//! Impressão de voz de uma fala: vetor de `f32` com norma 1. Fica só na memória da
//! reunião e em `voices.json`; nunca em sessões, logs ou na interface.
use crate::fbank::{cmn, Fbank, NUM_BINS};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::Arc;
use tract_onnx::prelude::*;

pub trait SpeakerEmbedder: Send {
    /// `audio`: 16 kHz mono em [-1, 1]. Devolve a impressão normalizada (norma 1).
    fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>>;
}

/// Normaliza para norma 1. `false` (vetor intocado) se a norma é zero ou não finita.
pub fn normalize(v: &mut [f32]) -> bool {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !n.is_finite() || n < 1e-6 { return false; }
    for x in v.iter_mut() { *x /= n; }
    true
}

/// Semelhança de cosseno; -1 para tamanhos diferentes ou vetores nulos.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() { return -1.0; }
    let (mut ab, mut aa, mut bb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa <= 0.0 || bb <= 0.0 { return -1.0; }
    ab / (aa.sqrt() * bb.sqrt())
}

/// Modelo escolhido na Task 1 (id em `models.json`).
pub const DEFAULT_SPEAKER_MODEL: &str = "wespeaker-ecapa512-lm";

/// Abaixo de 0,5 s o modelo não tem quadros suficientes para uma impressão confiável.
pub const MIN_EMBED_SAMPLES: usize = 8_000;

/// Modelo WeSpeaker em ONNX (entrada `feats` [1, T, 80], saída [1, D]) rodando no tract.
pub struct TractEmbedder {
    model: Arc<TypedRunnableModel>,
    fbank: Fbank,
}

impl TractEmbedder {
    pub fn new(model_path: &Path) -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(model_path)
            .context("ler o modelo de voz")?
            .into_optimized()
            .context("otimizar o modelo de voz")?
            .into_runnable()
            .context("preparar o modelo de voz")?;
        Ok(Self { model, fbank: Fbank::new() })
    }
}

impl SpeakerEmbedder for TractEmbedder {
    fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
        if audio.len() < MIN_EMBED_SAMPLES { bail!("fala curta demais para impressão de voz"); }
        let mut feats = self.fbank.compute(audio);
        cmn(&mut feats);
        let frames = feats.len();
        let flat: Vec<f32> = feats.into_iter().flatten().collect();
        let input = Tensor::from_shape(&[1, frames, NUM_BINS], &flat)?;
        let out = self.model.run(tvec!(input.into()))?;
        let mut v: Vec<f32> = out[0].to_plain_array_view::<f32>()?.iter().copied().collect();
        if !normalize(&mut v) { bail!("impressão de voz inválida"); }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_cosine() {
        let mut v = vec![3.0, 4.0];
        assert!(normalize(&mut v));
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert!(!normalize(&mut [0.0, 0.0]));
        assert!(!normalize(&mut [f32::NAN, 1.0]));
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), -1.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), -1.0);
    }

    #[test]
    fn embedder_is_send() {
        fn send<T: Send>() {}
        send::<TractEmbedder>();
    }
}
