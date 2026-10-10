//! Idiomas falados que o app entende (etapa 3). A tradução é sempre para o outro.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Pt,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Pt];

    /// Código ISO 639-1: idioma do Whisper e valor no JSON.
    pub fn code(self) -> &'static str {
        match self { Lang::En => "en", Lang::Pt => "pt" }
    }

    /// Idioma da tradução.
    pub fn other(self) -> Lang {
        match self { Lang::En => Lang::Pt, Lang::Pt => Lang::En }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        match s { "en" => Some(Lang::En), "pt" => Some(Lang::Pt), _ => None }
    }

    /// Nome em português, minúsculo, para avisos e o Markdown.
    pub fn name(self) -> &'static str {
        match self { Lang::En => "inglês", Lang::Pt => "português" }
    }
}

#[cfg(test)]
mod tests {
    use super::Lang;

    #[test]
    fn codes_names_and_other() {
        assert_eq!(Lang::ALL, [Lang::En, Lang::Pt]);
        assert_eq!((Lang::En.code(), Lang::Pt.code()), ("en", "pt"));
        assert_eq!((Lang::En.other(), Lang::Pt.other()), (Lang::Pt, Lang::En));
        assert_eq!((Lang::from_code("pt"), Lang::from_code("en"), Lang::from_code("es")), (Some(Lang::Pt), Some(Lang::En), None));
        assert_eq!((Lang::En.name(), Lang::Pt.name()), ("inglês", "português"));
    }

    #[test]
    fn json_is_the_code() {
        assert_eq!(serde_json::to_string(&Lang::Pt).unwrap(), "\"pt\"");
        assert_eq!(serde_json::from_str::<Lang>("\"en\"").unwrap(), Lang::En);
        assert_eq!(Lang::default(), Lang::En);
    }
}
