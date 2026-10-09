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

/// Separa o prefixo de idioma (se `text` começa com ele) do restante do texto.
fn split_prefix<'a>(text: &'a str, prefix: Option<&str>) -> (Option<&'a str>, &'a str) {
    match prefix.and_then(|p| text.strip_prefix(p).map(|rest| (p, rest))) {
        Some((p, rest)) => (Some(&text[..p.len()]), rest.trim_start()),
        None => (None, text),
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
        let (prefix, rest) = split_prefix(input, self.prefix.as_deref());
        let mut toks = self.inner.encode(rest)?;
        if let Some(p) = prefix {
            toks.insert(0, p.to_string());
        }
        Ok(toks)
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


#[cfg(test)]
mod tests {
    use super::{prepare_source, split_prefix};

    #[test]
    fn adds_target_prefix_when_configured() {
        assert_eq!(prepare_source(Some(">>pob<<"), " Hello team. "), ">>pob<< Hello team.");
    }
    #[test]
    fn no_prefix_just_trims() {
        assert_eq!(prepare_source(None, " Hello team. "), "Hello team.");
    }
    #[test]
    fn split_prefix_present() {
        assert_eq!(split_prefix(">>pob<< Hello team.", Some(">>pob<<")), (Some(">>pob<<"), "Hello team."));
    }
    #[test]
    fn split_prefix_none_configured() {
        assert_eq!(split_prefix(">>pob<< Hello", None), (None, ">>pob<< Hello"));
    }
    #[test]
    fn split_prefix_text_does_not_start_with_it() {
        assert_eq!(split_prefix("Hello >>pob<< team", Some(">>pob<<")), (None, "Hello >>pob<< team"));
    }
    #[test]
    fn split_prefix_empty_rest() {
        assert_eq!(split_prefix(">>pob<<", Some(">>pob<<")), (Some(">>pob<<"), ""));
        assert_eq!(split_prefix(">>pob<<  ", Some(">>pob<<")), (Some(">>pob<<"), ""));
    }
}
