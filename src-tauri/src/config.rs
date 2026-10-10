use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub source_lang: String,
    pub target_lang: String,
    pub whisper_model: String,
    pub opus_model: String,
    pub opus_prefix: Option<String>,
    pub models_base_url: String,
    /// Modelo de impressão de voz (id do manifesto).
    pub speaker_model: String,
    /// Semelhança mínima (cosseno) para reconhecer uma voz guardada.
    pub speaker_library_threshold: f32,
    /// Semelhança mínima para juntar a fala a um grupo da reunião.
    pub speaker_group_threshold: f32,
    /// Falas mais curtas herdam o último falante do sistema.
    pub speaker_min_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source_lang: "en".into(),
            target_lang: "pt".into(),
            // Único modelo que cumpre p90 <= 3 s no M2; o spike do Windows pode mudar.
            whisper_model: "whisper-small-en".into(),
            opus_model: "opus-mt-en-pt".into(),
            opus_prefix: Some(">>pob<<".into()),
            models_base_url: gow_meet_core::models::DEFAULT_BASE_URL.into(),
            speaker_model: gow_meet_core::speaker::DEFAULT_SPEAKER_MODEL.into(),
            speaker_library_threshold: 0.70,
            speaker_group_threshold: 0.65,
            speaker_min_ms: 1_000,
        }
    }
}

impl Config {
    pub fn tracker(&self) -> gow_meet_core::tracker::TrackerConfig {
        gow_meet_core::tracker::TrackerConfig {
            library_threshold: self.speaker_library_threshold,
            group_threshold: self.speaker_group_threshold,
            min_speech_ms: self.speaker_min_ms,
        }
    }

    /// Arquivo ausente: padrões. Arquivo inválido: aviso (sem o conteúdo) e padrões.
    pub fn load(path: &Path) -> Config {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Config::default(),
            Err(e) => {
                tracing::warn!(kind = ?e.kind(), "config.json ilegível; usando padrões");
                return Config::default();
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(line = e.line(), column = e.column(), "config.json inválido; usando padrões");
                Config::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        let c = Config::load(&d.path().join("config.json"));
        assert_eq!(c, Config::default());
        assert_eq!(c.opus_prefix.as_deref(), Some(">>pob<<"));
        assert_eq!(c.whisper_model, "whisper-small-en");
    }

    #[test]
    fn partial_override_keeps_other_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"whisper_model":"whisper-turbo","models_base_url":"http://espelho.local"}"#).unwrap();
        let c = Config::load(&p);
        assert_eq!(c.whisper_model, "whisper-turbo");
        assert_eq!(c.models_base_url, "http://espelho.local");
        assert_eq!(c.opus_model, "opus-mt-en-pt");
        assert_eq!(c.target_lang, "pt");
    }

    #[test]
    fn speaker_defaults_and_override() {
        let d = tempfile::tempdir().unwrap();
        let c = Config::load(&d.path().join("config.json"));
        assert_eq!(c.speaker_model, "wespeaker-ecapa512-lm");
        assert_eq!(c.tracker(), gow_meet_core::tracker::TrackerConfig::default());
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"speaker_group_threshold":0.6,"speaker_min_ms":1500}"#).unwrap();
        let c = Config::load(&p);
        assert_eq!((c.speaker_group_threshold, c.speaker_min_ms, c.speaker_library_threshold), (0.6, 1_500, 0.70));
    }

    #[test]
    fn invalid_file_falls_back_to_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, "{ não é json").unwrap();
        assert_eq!(Config::load(&p), Config::default());
        std::fs::write(&p, r#"{"whisper_model": 3}"#).unwrap();
        assert_eq!(Config::load(&p), Config::default());
    }
}
