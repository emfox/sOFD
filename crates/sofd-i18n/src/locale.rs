//! Locale identifiers and negotiation.
//! BCP 47 tags such as `zh-CN` and `en-US`, parsed by `unic-langid`, with
//! negotiation from user preference to the closest available locale.

use std::fmt;
use std::str::FromStr;

use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use crate::BUILTIN_LOCALES;

/// A BCP 47 language tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Locale(String);

impl Locale {
    /// Parse a tag, falling back to the default when malformed.
    pub fn new(tag: &str) -> Self {
        match unic_langid::LanguageIdentifier::from_str(tag) {
            Ok(id) => Self(id.to_string()),
            Err(_) => {
                tracing::warn!(tag, "invalid language tag, falling back to default");
                Self(crate::FALLBACK_LOCALE.to_string())
            }
        }
    }

    /// Simplified Chinese.
    pub fn zh_cn() -> Self {
        Self("zh-CN".to_string())
    }

    /// English, United States.
    pub fn en_us() -> Self {
        Self("en-US".to_string())
    }

    /// The fallback locale.
    pub fn fallback() -> Self {
        Self(crate::FALLBACK_LOCALE.to_string())
    }

    /// The full tag.
    pub fn tag(&self) -> &str {
        &self.0
    }

    /// The tag as a parsed identifier, which is the form the negotiator reads.
    /// A tag that parsed when this locale was built parses here too, so the
    /// fallback is unreachable rather than a second guess at the input.
    fn parsed(&self) -> LanguageIdentifier {
        LanguageIdentifier::from_str(&self.0).unwrap_or_default()
    }

    /// Language subtag only, so `zh-CN` yields `zh`.
    pub fn language(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }

    /// Whether the language runs right to left, which UI layout must mirror.
    pub fn is_rtl(&self) -> bool {
        matches!(self.language(), "ar" | "he" | "fa" | "ur")
    }

    /// Whether the language is CJK, affecting font choice and line breaking.
    pub fn is_cjk(&self) -> bool {
        matches!(self.language(), "zh" | "ja" | "ko")
    }

    /// Whether this locale is compiled into the current build.
    pub fn is_builtin(&self) -> bool {
        BUILTIN_LOCALES.contains(&self.0.as_str())
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::fallback()
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Locale {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// How a locale is chosen.
#[derive(Debug, Clone, Default)]
pub enum LocaleNegotiation {
    /// Use an explicit locale, ignoring the system.
    Fixed(Locale),
    /// Follow the system, defaulting when detection fails.
    #[default]
    System,
    /// Walk a preference list, as with HTTP `Accept-Language`.
    PreferenceList(Vec<Locale>),
}

impl LocaleNegotiation {
    /// Pick the best available locale.
    /// The matching itself is `fluent-langneg`'s: it walks the requested tags in
    /// order and narrows region and variants the way Unicode TR35 does, so a
    /// caller's `zh-Hant-HK` still lands on `zh-CN` when that is what ships.
    pub fn negotiate(&self, available: &[Locale]) -> Locale {
        let requested: Vec<LanguageIdentifier> = match self {
            Self::Fixed(l) => vec![l.parsed()],
            Self::PreferenceList(list) => list.iter().map(Locale::parsed).collect(),
            Self::System => vec![detect_system_locale().parsed()],
        };
        let offered: Vec<LanguageIdentifier> = available.iter().map(Locale::parsed).collect();

        let picked = negotiate_languages(&requested, &offered, None, NegotiationStrategy::Lookup);
        if let Some(first) = picked.first() {
            return Locale(first.to_string());
        }

        // Nothing matched: the fallback is only usable when it shipped, and a
        // build that carries no catalogue at all answers with the tag anyway.
        available
            .iter()
            .find(|a| a.0 == crate::FALLBACK_LOCALE)
            .cloned()
            .unwrap_or_else(Locale::fallback)
    }
}

/// Detect the system locale from environment variables.
#[cfg(not(target_arch = "wasm32"))]
fn detect_system_locale() -> Locale {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(v) = std::env::var(key)
            && !v.is_empty()
            && v != "C"
            && v != "POSIX"
        {
            let tag = v.split('.').next().unwrap_or(&v).replace('_', "-");
            return Locale::new(&tag);
        }
    }
    Locale::fallback()
}

/// On WASM the host supplies the locale explicitly.
#[cfg(target_arch = "wasm32")]
fn detect_system_locale() -> Locale {
    Locale::fallback()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tags_are_stored_as_given() {
        assert_eq!(Locale::new("zh-CN").tag(), "zh-CN");
        assert_eq!(Locale::new("en-US").tag(), "en-US");
    }

    #[test]
    fn malformed_tags_fall_back_to_the_default() {
        let l = Locale::new("not a language tag!!");
        assert_eq!(l, Locale::fallback());
    }

    #[test]
    fn language_subtag_strips_the_region() {
        assert_eq!(Locale::new("zh-CN").language(), "zh");
        assert_eq!(Locale::new("en-US").language(), "en");
        assert_eq!(Locale::new("zh").language(), "zh");
    }

    #[test]
    fn script_subtag_survives_in_the_tag() {
        let l = Locale::new("zh-Hans-CN");
        assert_eq!(l.language(), "zh");
        assert!(l.tag().starts_with("zh"));
    }

    #[test]
    fn rtl_languages_are_recognized() {
        assert!(Locale::new("ar").is_rtl());
        assert!(Locale::new("he").is_rtl());
        assert!(!Locale::new("en-US").is_rtl());
        assert!(!Locale::new("zh-CN").is_rtl());
    }

    #[test]
    fn cjk_languages_are_recognized() {
        assert!(Locale::new("zh-CN").is_cjk());
        assert!(Locale::new("ja").is_cjk());
        assert!(Locale::new("ko").is_cjk());
        assert!(!Locale::new("en-US").is_cjk());
    }

    #[test]
    fn builtin_locales_are_reported() {
        assert!(Locale::en_us().is_builtin());
        assert!(Locale::zh_cn().is_builtin());
        assert!(!Locale::new("fr-FR").is_builtin());
    }

    #[test]
    fn default_is_the_fallback_locale() {
        assert_eq!(Locale::default(), Locale::fallback());
    }

    #[test]
    fn display_prints_the_tag() {
        assert_eq!(Locale::zh_cn().to_string(), "zh-CN");
    }

    #[test]
    fn fixed_negotiation_picks_the_exact_tag() {
        let available = [Locale::en_us(), Locale::zh_cn()];
        let picked = LocaleNegotiation::Fixed(Locale::zh_cn()).negotiate(&available);
        assert_eq!(picked, Locale::zh_cn());
    }

    #[test]
    fn fixed_negotiation_falls_back_when_the_tag_is_absent() {
        let available = [Locale::en_us()];
        let picked = LocaleNegotiation::Fixed(Locale::new("fr-FR")).negotiate(&available);
        assert_eq!(picked, Locale::fallback());
    }

    #[test]
    fn negotiation_matches_same_language_when_the_region_differs() {
        // zh-TW is not available, but zh-CN shares the language subtag.
        let available = [Locale::en_us(), Locale::zh_cn()];
        let picked = LocaleNegotiation::Fixed(Locale::new("zh-TW")).negotiate(&available);
        assert_eq!(picked, Locale::zh_cn());
    }

    #[test]
    fn preference_list_walks_in_order() {
        let available = [Locale::en_us(), Locale::zh_cn()];
        let list = LocaleNegotiation::PreferenceList(vec![
            Locale::new("fr-FR"),
            Locale::zh_cn(),
            Locale::en_us(),
        ]);
        assert_eq!(list.negotiate(&available), Locale::zh_cn());
    }

    #[test]
    fn preference_list_falls_through_to_the_next_candidate() {
        // fr is unavailable, so the list moves on to en-US.
        let available = [Locale::en_us(), Locale::zh_cn()];
        let list = LocaleNegotiation::PreferenceList(vec![Locale::new("fr-FR"), Locale::en_us()]);
        assert_eq!(list.negotiate(&available), Locale::en_us());
    }

    #[test]
    fn exact_tag_beats_a_language_only_match() {
        let available = [Locale::zh_cn(), Locale::new("zh-TW")];
        let picked = LocaleNegotiation::Fixed(Locale::new("zh-TW")).negotiate(&available);
        assert_eq!(picked.tag(), "zh-TW");
    }

    #[test]
    fn empty_available_list_yields_the_fallback() {
        let picked = LocaleNegotiation::Fixed(Locale::zh_cn()).negotiate(&[]);
        assert_eq!(picked, Locale::fallback());
    }

    #[test]
    fn system_negotiation_returns_an_available_locale() {
        let available = [Locale::en_us(), Locale::zh_cn()];
        let picked = LocaleNegotiation::System.negotiate(&available);
        assert!(
            available.contains(&picked),
            "system negotiation returned {picked}, which is not available"
        );
    }

    #[test]
    fn from_str_never_fails() {
        let l: Locale = "zh-CN".parse().expect("infallible");
        assert_eq!(l, Locale::zh_cn());
    }
}
