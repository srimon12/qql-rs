//! BM25 text-processing languages, mirroring Qdrant's `Language` names.
//!
//! The canonical spellings are Qdrant's `snake_case` serializations
//! (`"english"`, `"spanish"`, …); [`Language::parse`] additionally accepts
//! Qdrant's two-letter aliases (`"en"`, `"es"`, …) case-insensitively.
//! Unknown names fail closed with `QQL-VALIDATION-CONFIG` — like Qdrant's
//! edge builder, which rejects unsupported languages instead of silently
//! disabling stemming and stopwords.

use qql_core::error::QqlError;
use rust_stemmers::Algorithm;

/// Text-processing language for BM25 tokenization. One variant per Qdrant
/// `Language`; serializations match Qdrant's `snake_case` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// Arabic (`"ar"`). Snowball stemming + stopwords.
    Arabic,
    /// Azerbaijani (`"az"`). No default stemmer; stopwords only.
    Azerbaijani,
    /// Basque (`"eu"`). No default stemmer; stopwords only.
    Basque,
    /// Bengali (`"bn"`). No default stemmer; stopwords only.
    Bengali,
    /// Catalan (`"ca"`). No default stemmer; stopwords only.
    Catalan,
    /// Chinese (`"zh"`). No default stemmer; stopwords only.
    Chinese,
    /// Danish (`"da"`). Snowball stemming + stopwords.
    Danish,
    /// Dutch (`"nl"`). Snowball stemming + stopwords.
    Dutch,
    /// English (`"en"`). Snowball stemming + stopwords.
    English,
    /// Finnish (`"fi"`). Snowball stemming + stopwords.
    Finnish,
    /// French (`"fr"`). Snowball stemming + stopwords.
    French,
    /// German (`"de"`). Snowball stemming + stopwords.
    German,
    /// Greek (`"el"`). Snowball stemming + stopwords.
    Greek,
    /// Hebrew (`"he"`). No default stemmer; stopwords only.
    Hebrew,
    /// Hinglish (`"hi-en"`). No default stemmer; stopwords only.
    Hinglish,
    /// Hungarian (`"hu"`). Snowball stemming + stopwords.
    Hungarian,
    /// Indonesian (`"id"`). No default stemmer; stopwords only.
    Indonesian,
    /// Italian (`"it"`). Snowball stemming + stopwords.
    Italian,
    /// Japanese (`"jp"`). No default stemmer; stopwords only.
    Japanese,
    /// Kazakh (`"kk"`). No default stemmer; stopwords only.
    Kazakh,
    /// Nepali (`"ne"`). No default stemmer; stopwords only.
    Nepali,
    /// Norwegian (`"no"`). Snowball stemming + stopwords.
    Norwegian,
    /// Portuguese (`"pt"`). Snowball stemming + stopwords.
    Portuguese,
    /// Romanian (`"ro"`). Snowball stemming + stopwords.
    Romanian,
    /// Russian (`"ru"`). Snowball stemming + stopwords.
    Russian,
    /// Slovene (`"sl"`). No default stemmer; stopwords only.
    Slovene,
    /// Spanish (`"es"`). Snowball stemming + stopwords.
    Spanish,
    /// Swedish (`"sv"`). Snowball stemming + stopwords.
    Swedish,
    /// Tajik (`"tg"`). No default stemmer; stopwords only.
    Tajik,
    /// Turkish (`"tr"`). Snowball stemming + stopwords.
    Turkish,
}

impl Default for Language {
    /// Qdrant's BM25 default: English stemming and stopwords.
    fn default() -> Self {
        Self::English
    }
}

impl Language {
    /// Canonical Qdrant spelling (`snake_case`, as Qdrant serializes it).
    pub fn name(self) -> &'static str {
        match self {
            Self::Arabic => "arabic",
            Self::Azerbaijani => "azerbaijani",
            Self::Basque => "basque",
            Self::Bengali => "bengali",
            Self::Catalan => "catalan",
            Self::Chinese => "chinese",
            Self::Danish => "danish",
            Self::Dutch => "dutch",
            Self::English => "english",
            Self::Finnish => "finnish",
            Self::French => "french",
            Self::German => "german",
            Self::Greek => "greek",
            Self::Hebrew => "hebrew",
            Self::Hinglish => "hinglish",
            Self::Hungarian => "hungarian",
            Self::Indonesian => "indonesian",
            Self::Italian => "italian",
            Self::Japanese => "japanese",
            Self::Kazakh => "kazakh",
            Self::Nepali => "nepali",
            Self::Norwegian => "norwegian",
            Self::Portuguese => "portuguese",
            Self::Romanian => "romanian",
            Self::Russian => "russian",
            Self::Slovene => "slovene",
            Self::Spanish => "spanish",
            Self::Swedish => "swedish",
            Self::Tajik => "tajik",
            Self::Turkish => "turkish",
        }
    }

    /// Parse a language name or Qdrant two-letter alias (`"es"`, `"zh"`,
    /// `"hi-en"`, …), ASCII-case-insensitively. The case-folding is a
    /// deliberate superset of Qdrant's case-sensitive serde (same accepted
    /// set, friendlier spelling). Anything else fails closed.
    pub fn parse(name: &str) -> Result<Self, QqlError> {
        // `hi-en` is the only alias outside `[a-z]`; compare lowercased.
        let lower = name.to_ascii_lowercase();
        let language = match lower.as_str() {
            "arabic" | "ar" => Self::Arabic,
            "azerbaijani" | "az" => Self::Azerbaijani,
            "basque" | "eu" => Self::Basque,
            "bengali" | "bn" => Self::Bengali,
            "catalan" | "ca" => Self::Catalan,
            "chinese" | "zh" => Self::Chinese,
            "danish" | "da" => Self::Danish,
            "dutch" | "nl" => Self::Dutch,
            "english" | "en" => Self::English,
            "finnish" | "fi" => Self::Finnish,
            "french" | "fr" => Self::French,
            "german" | "de" => Self::German,
            "greek" | "el" => Self::Greek,
            "hebrew" | "he" => Self::Hebrew,
            "hinglish" | "hi-en" => Self::Hinglish,
            "hungarian" | "hu" => Self::Hungarian,
            "indonesian" | "id" => Self::Indonesian,
            "italian" | "it" => Self::Italian,
            "japanese" | "jp" => Self::Japanese,
            "kazakh" | "kk" => Self::Kazakh,
            "nepali" | "ne" => Self::Nepali,
            "norwegian" | "no" => Self::Norwegian,
            "portuguese" | "pt" => Self::Portuguese,
            "romanian" | "ro" => Self::Romanian,
            "russian" | "ru" => Self::Russian,
            "slovene" | "sl" => Self::Slovene,
            "spanish" | "es" => Self::Spanish,
            "swedish" | "sv" => Self::Swedish,
            "tajik" | "tg" => Self::Tajik,
            "turkish" | "tr" => Self::Turkish,
            _ => {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-CONFIG",
                    format!("unsupported bm25 language: {name:?}"),
                    None,
                ));
            }
        };
        Ok(language)
    }

    /// Snowball stemmer for this language, if Qdrant defines one.
    ///
    /// Mirrors Qdrant's `Stemmer::try_default_from_language`: exactly the 17
    /// Snowball languages reachable via `language` stem (Armenian and Tamil
    /// exist in Qdrant's `SnowballLanguage` but have no `Language` variant,
    /// so they are explicit-stemmer-only — see [`Stemmer`](super::bm25_text::Stemmer)).
    /// The rest (Chinese, Japanese, Hebrew, …) have no default stemmer and
    /// pass tokens through unstemmed. `qdrant-rust-stemmers` is the same
    /// crate family Qdrant uses, so the stems are identical.
    pub fn stem_algorithm(self) -> Option<Algorithm> {
        match self {
            Self::Arabic => Some(Algorithm::Arabic),
            Self::Danish => Some(Algorithm::Danish),
            Self::Dutch => Some(Algorithm::Dutch),
            Self::English => Some(Algorithm::English),
            Self::Finnish => Some(Algorithm::Finnish),
            Self::French => Some(Algorithm::French),
            Self::German => Some(Algorithm::German),
            Self::Greek => Some(Algorithm::Greek),
            Self::Hungarian => Some(Algorithm::Hungarian),
            Self::Italian => Some(Algorithm::Italian),
            Self::Norwegian => Some(Algorithm::Norwegian),
            Self::Portuguese => Some(Algorithm::Portuguese),
            Self::Romanian => Some(Algorithm::Romanian),
            Self::Russian => Some(Algorithm::Russian),
            Self::Spanish => Some(Algorithm::Spanish),
            Self::Swedish => Some(Algorithm::Swedish),
            Self::Turkish => Some(Algorithm::Turkish),
            _ => None,
        }
    }
}
