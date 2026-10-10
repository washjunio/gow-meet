use crate::lang::Lang;
use anyhow::{Context, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

pub trait Transcriber: Send {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String>;
}

/// Frases que o Whisper inventa em silêncio, por idioma (minúsculas, comparação exata).
const HALLUCINATIONS_EN: &[&str] = &[
    "thanks for watching!", "thank you for watching.", "thank you for watching!",
    "subtitles by the amara.org community",
];
/// Português: conhecidas do Whisper e as da medição (`docs/modelos-pt.md`, seção Alucinações).
const HALLUCINATIONS_PT: &[&str] = &[
    "legendas pela comunidade amara.org", "obrigado por assistir.", "obrigado por assistir!",
    "obrigado por assistir",
];

fn hallucinations(lang: Lang) -> &'static [&'static str] {
    match lang { Lang::En => HALLUCINATIONS_EN, Lang::Pt => HALLUCINATIONS_PT }
}

/// Remove marcadores como `[BLANK_AUDIO]`, `(music)`, `*laughs*` e frases que o
/// Whisper inventa em silêncio. `None` quando não sobra fala.
pub fn clean_transcript(raw: &str, lang: Lang) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0i32;
    let mut in_star = false;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            '*' => in_star = !in_star,
            _ if depth == 0 && !in_star => out.push(c),
            _ => {}
        }
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || hallucinations(lang).contains(&text.to_lowercase().as_str()) { return None; }
    Some(text)
}

/// whisper.cpp não devolve nada para entradas menores que 1 s. Com exatamente
/// 16000 amostras ele mede 990 ms (arredondamento por quadros de 10 ms), então
/// a margem fica em 1100 ms.
const MIN_SAMPLES: usize = 17_600;

fn pad_to_min(audio: &[f32]) -> Vec<f32> {
    let mut v = audio.to_vec();
    v.resize(MIN_SAMPLES.max(v.len()), 0.0);
    v
}

pub struct WhisperTranscriber { _ctx: WhisperContext, state: WhisperState, lang: String }

impl WhisperTranscriber {
    pub fn new(model_path: &Path, lang: &str, use_gpu: bool) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().context("caminho do modelo não é UTF-8")?, params,
        ).context("carregar modelo Whisper")?;
        let state = ctx.create_state().context("criar estado Whisper")?;
        Ok(Self { _ctx: ctx, state, lang: lang.to_string() })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String> {
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some(&self.lang));
        p.set_no_context(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        let padded;
        let audio = if audio.len() < MIN_SAMPLES {
            padded = pad_to_min(audio);
            &padded[..]
        } else {
            audio
        };
        self.state.full(p, audio).context("whisper full")?;
        let n = self.state.full_n_segments()?;
        let mut text = String::new();
        for i in 0..n {
            text.push_str(&self.state.full_get_segment_text(i)?);
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_transcript, pad_to_min};
    use crate::lang::Lang;
    #[test]
    fn keeps_normal_speech() {
        assert_eq!(clean_transcript("  Let's review the timeline. ", Lang::En).as_deref(), Some("Let's review the timeline."));
        assert_eq!(clean_transcript(" Vamos revisar o cronograma. ", Lang::Pt).as_deref(), Some("Vamos revisar o cronograma."));
    }
    #[test]
    fn drops_markers_and_hallucinations() {
        for raw in ["[BLANK_AUDIO]", " (music) ", "*laughs*", "Thanks for watching!", "[ Silence ]", ""] {
            assert_eq!(clean_transcript(raw, Lang::En), None, "{raw:?}");
        }
    }
    #[test]
    fn portuguese_hallucinations_are_dropped_in_portuguese() {
        for raw in ["Legendas pela comunidade Amara.org", "Obrigado por assistir!", "[Música]"] {
            assert_eq!(clean_transcript(raw, Lang::Pt), None, "{raw:?}");
        }
    }
    #[test]
    fn strips_markers_inside_text() {
        assert_eq!(clean_transcript("[MUSIC] Good morning everyone.", Lang::En).as_deref(), Some("Good morning everyone."));
    }
    #[test]
    fn short_audio_is_padded_past_one_second() {
        assert_eq!(pad_to_min(&[0.5; 100]).len(), 17_600);
        assert_eq!(pad_to_min(&[0.5; 100])[99], 0.5);
        assert_eq!(pad_to_min(&[0.5; 20_000]).len(), 20_000);
    }
}
