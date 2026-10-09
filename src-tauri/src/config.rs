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
        }
    }
}

impl Config {
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
    fn invalid_file_falls_back_to_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, "{ não é json").unwrap();
        assert_eq!(Config::load(&p), Config::default());
        std::fs::write(&p, r#"{"whisper_model": 3}"#).unwrap();
        assert_eq!(Config::load(&p), Config::default());
    }
}
