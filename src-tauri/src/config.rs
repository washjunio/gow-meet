use gow_meet_core::lang::Lang;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Inglês: transcrição e tradução para o português (ids do manifesto).
    pub whisper_model: String,
    pub opus_model: String,
    pub opus_prefix: Option<String>,
    /// Português: Whisper multilíngue e tradução para o inglês (ids do manifesto).
    pub whisper_model_pt: String,
    pub opus_model_pt: String,
    pub opus_prefix_pt: Option<String>,
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
            // Único modelo que cumpre p90 <= 3 s no M2; o spike do Windows pode mudar.
            whisper_model: "whisper-small-en".into(),
            opus_model: "opus-mt-en-pt".into(),
            opus_prefix: Some(">>pob<<".into()),
            whisper_model_pt: "whisper-small".into(),
            opus_model_pt: "opus-mt-pt-en".into(),
            // Convertido do opus-mt-itc-en: só traduz para o inglês, dispensa prefixo.
            opus_prefix_pt: None,
            models_base_url: gow_meet_core::models::DEFAULT_BASE_URL.into(),
            speaker_model: gow_meet_core::speaker::DEFAULT_SPEAKER_MODEL.into(),
            speaker_library_threshold: 0.70,
            speaker_group_threshold: 0.65,
            speaker_min_ms: 1_000,
        }
    }
}

/// Modelos de um idioma falado (ids do manifesto) e o prefixo de destino do Opus-MT.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair { pub whisper: String, pub opus: String, pub prefix: Option<String> }

impl Config {
    pub fn pair(&self, lang: Lang) -> Pair {
        match lang {
            Lang::En => Pair { whisper: self.whisper_model.clone(), opus: self.opus_model.clone(),
                               prefix: self.opus_prefix.clone() },
            Lang::Pt => Pair { whisper: self.whisper_model_pt.clone(), opus: self.opus_model_pt.clone(),
                               prefix: self.opus_prefix_pt.clone() },
        }
    }

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
        assert_eq!(c.whisper_model_pt, Config::default().whisper_model_pt);
    }

    #[test]
    fn pair_for_each_language() {
        let c = Config::default();
        assert_eq!(c.pair(Lang::En), Pair { whisper: "whisper-small-en".into(), opus: "opus-mt-en-pt".into(),
                                            prefix: Some(">>pob<<".into()) });
        let pt = c.pair(Lang::Pt);
        assert_eq!((pt.whisper.as_str(), pt.opus.as_str()), (c.whisper_model_pt.as_str(), "opus-mt-pt-en"));
        assert_eq!(pt.prefix, c.opus_prefix_pt);
    }

    #[test]
    fn old_config_with_language_keys_still_loads() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"source_lang":"en","target_lang":"pt","whisper_model":"whisper-turbo"}"#).unwrap();
        assert_eq!(Config::load(&p).whisper_model, "whisper-turbo");
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
