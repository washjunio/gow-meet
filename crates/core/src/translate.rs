use anyhow::{Context, Result};
use std::path::Path;

pub trait Translator: Send {
    fn translate(&mut self, text: &str) -> Result<String>;
}

pub fn prepare_source(prefix: Option<&str>, text: &str) -> String {
    match prefix {
        Some(p) => format!("{p} {}", text.trim()),
        None => text.trim().to_string(),
    }
}

/// Envolve o tokenizador do ct2rs para que o prefixo de idioma de destino
/// (ex.: `>>pob<<`) entre como UM token do vocabulário. O SentencePiece o
/// quebraria em pedaços (`>>`, `pob`, `<<`), e o modelo devolveria lixo.
struct PrefixTokenizer {
    inner: ct2rs::tokenizers::auto::Tokenizer,
    prefix: Option<String>,
}

impl ct2rs::Tokenizer for PrefixTokenizer {
    fn encode(&self, input: &str) -> Result<Vec<String>> {
        if let Some(p) = &self.prefix {
            if let Some(rest) = input.strip_prefix(p.as_str()) {
                let mut toks = self.inner.encode(rest.trim_start())?;
                toks.insert(0, p.clone());
                return Ok(toks);
            }
        }
        self.inner.encode(input)
    }

    fn decode(&self, tokens: Vec<String>) -> Result<String> {
        self.inner.decode(tokens)
    }
}

pub struct Ct2Translator {
    inner: ct2rs::Translator<PrefixTokenizer>,
    prefix: Option<String>,
}

impl Ct2Translator {
    pub fn new(model_dir: &Path, target_prefix: Option<String>) -> Result<Self> {
        let tokenizer = PrefixTokenizer {
            inner: ct2rs::tokenizers::auto::Tokenizer::new(model_dir)
                .context("carregar tokenizador de tradução")?,
            prefix: target_prefix.clone(),
        };
        let inner =
            ct2rs::Translator::with_tokenizer(model_dir, tokenizer, &ct2rs::Config::default())
                .context("carregar modelo de tradução")?;
        Ok(Self { inner, prefix: target_prefix })
    }
}

impl Translator for Ct2Translator {
    fn translate(&mut self, text: &str) -> Result<String> {
        let src = prepare_source(self.prefix.as_deref(), text);
        let opts = ct2rs::TranslationOptions::<String, String>::default();
        let out = self
            .inner
            .translate_batch(&[src], &opts, None)
            .context("traduzir")?;
        Ok(out.into_iter().next().map(|(t, _)| t.trim().to_string()).unwrap_or_default())
    }
}

