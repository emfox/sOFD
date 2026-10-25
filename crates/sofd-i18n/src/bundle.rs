//! Fluent message bundles keyed by locale.

use std::collections::HashMap;
use std::sync::Arc;

use fluent::{FluentArgs, FluentResource};
use fluent_bundle::concurrent::FluentBundle as ConcurrentFluentBundle;

use crate::BUILTIN_LOCALES;
use crate::locale::{Locale, LocaleNegotiation};
use crate::messages::MessageId;

// Catalogues are embedded at compile time: no runtime file scanning on desktop,
// no fetch on WASM, and a missing file fails the build rather than a user.
const ZH_CN_CORE: &str = include_str!("../locales/zh-CN/core.ftl");
const ZH_CN_CONVERT: &str = include_str!("../locales/zh-CN/convert.ftl");
const ZH_CN_SIGN: &str = include_str!("../locales/zh-CN/sign.ftl");
const ZH_CN_ERROR: &str = include_str!("../locales/zh-CN/error.ftl");
const ZH_CN_EDIT: &str = include_str!("../locales/zh-CN/edit.ftl");
const ZH_CN_SEARCH: &str = include_str!("../locales/zh-CN/search.ftl");
const ZH_CN_CLI: &str = include_str!("../locales/zh-CN/cli.ftl");

const EN_US_CORE: &str = include_str!("../locales/en-US/core.ftl");
const EN_US_CONVERT: &str = include_str!("../locales/en-US/convert.ftl");
const EN_US_SIGN: &str = include_str!("../locales/en-US/sign.ftl");
const EN_US_ERROR: &str = include_str!("../locales/en-US/error.ftl");
const EN_US_EDIT: &str = include_str!("../locales/en-US/edit.ftl");
const EN_US_SEARCH: &str = include_str!("../locales/en-US/search.ftl");
const EN_US_CLI: &str = include_str!("../locales/en-US/cli.ftl");

/// All resources for one locale.
struct LocaleResources {
    #[allow(dead_code)]
    locale: Locale,
    /// A concurrent bundle takes every resource at construction, so custom
    /// catalogues are merged into the source list and the bundle rebuilt.
    bundle: Arc<ConcurrentFluentBundle<FluentResource>>,
    /// Original FTL sources in order, so custom catalogues can be appended.
    sources: Vec<String>,
}

/// Localization context.
/// Thread safe: concurrent bundles behind an `Arc`, so a server shares one
/// instance across requests instead of rebuilding per call.
pub struct I18n {
    current: Locale,
    available: Vec<Locale>,
    resources: HashMap<String, LocaleResources>,
}

impl std::fmt::Debug for I18n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("I18n")
            .field("current", &self.current)
            .field("available", &self.available)
            .finish()
    }
}

impl I18n {
    /// Start building a context.
    pub fn builder() -> I18nBuilder {
        I18nBuilder::default()
    }

    /// Use the system locale with every built-in catalogue.
    pub fn with_system_locale() -> Self {
        Self::builder().build()
    }

    /// Use an explicit locale.
    pub fn with_locale(locale: Locale) -> Self {
        Self::builder().locale(locale).build()
    }

    /// The locale currently in effect.
    pub fn locale(&self) -> &Locale {
        &self.current
    }

    /// Switch locale, ignoring unknown ones.
    pub fn set_locale(&mut self, locale: Locale) {
        if self.resources.contains_key(locale.tag()) {
            self.current = locale;
        } else {
            tracing::warn!(
                requested = locale.tag(),
                "locale resources not built in, keeping current"
            );
        }
    }

    /// Locales available in this build.
    pub fn available_locales(&self) -> &[Locale] {
        &self.available
    }

    /// Render a message with no arguments.
    pub fn text(&self, id: MessageId) -> String {
        self.text_with(id, None)
    }

    /// Render a message with arguments.
    pub fn text_with<'a, I>(&self, id: MessageId, args: I) -> String
    where
        I: IntoIterator<Item = (&'a str, fluent::FluentValue<'a>)>,
    {
        self.text_key_with(id.key(), args)
    }

    /// Render a message by its raw key, for a catalogue the caller owns.
    /// A crate that keeps its own `.ftl` (the reader does) holds keys rather
    /// than [`MessageId`]s, and merges them with [`I18nBuilder::add_resource`].
    pub fn text_key(&self, key: &str) -> String {
        self.text_key_with(key, None)
    }

    /// Render a message by its raw key, with arguments.
    pub fn text_key_with<'a, I>(&self, key: &str, args: I) -> String
    where
        I: IntoIterator<Item = (&'a str, fluent::FluentValue<'a>)>,
    {
        let mut fa = FluentArgs::new();
        for (k, v) in args {
            fa.set(k, v);
        }

        if let Some(text) = self.lookup(&self.current, key, &fa) {
            return text;
        }

        let fallback = Locale::fallback();
        if self.current != fallback
            && let Some(text) = self.lookup(&fallback, key, &fa)
        {
            tracing::debug!(key, "translation missing, using fallback locale");
            return text;
        }

        tracing::warn!(key, "translation missing with no fallback");
        key.to_string()
    }

    /// Format a key in one locale.
    fn lookup(&self, locale: &Locale, key: &str, args: &FluentArgs<'_>) -> Option<String> {
        let res = self.resources.get(locale.tag())?;
        let msg = res.bundle.get_message(key)?;
        let pattern = msg.value()?;

        let mut errors = Vec::new();
        let formatted = res.bundle.format_pattern(pattern, Some(args), &mut errors);

        if !errors.is_empty() {
            tracing::warn!(key, ?errors, "Fluent formatting reported errors");
        }

        Some(formatted.into_owned())
    }

    /// Keys this locale lacks, for CI consistency checks.
    pub fn missing_keys(&self, locale: &Locale) -> Vec<&'static str> {
        let Some(res) = self.resources.get(locale.tag()) else {
            return Vec::new();
        };
        all_keys()
            .iter()
            .filter(|k| res.bundle.get_message(k).is_none())
            .copied()
            .collect()
    }
}

impl I18n {
    /// Number of locales loaded.
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// Whether this is an English-only build, used to trim binary size.
    pub fn is_english_only(&self) -> bool {
        self.available.len() == 1 && self.available[0].tag() == "en-US"
    }
}

/// Builder for [`I18n`].
#[derive(Default)]
pub struct I18nBuilder {
    locale: Option<Locale>,
    negotiation: LocaleNegotiation,
    extra: Vec<(Locale, String)>,
    allow_fallback_merge: bool,
}

impl I18nBuilder {
    /// Request a specific locale.
    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// Set the negotiation strategy.
    pub fn negotiation(mut self, n: LocaleNegotiation) -> Self {
        self.negotiation = n;
        self
    }

    /// Inject a custom catalogue, which overrides built-in keys.
    pub fn add_resource(mut self, locale: Locale, ftl: impl Into<String>) -> Self {
        self.extra.push((locale, ftl.into()));
        self
    }

    /// Allow fallback keys to be merged into the current locale.
    /// Off by default; Fluent's own fallback suffices and enabling this defeats
    /// `missing_keys` reporting.
    pub fn allow_fallback_merge(mut self, yes: bool) -> Self {
        self.allow_fallback_merge = yes;
        self
    }

    /// Build the context.
    pub fn build(self) -> I18n {
        let mut resources: HashMap<String, LocaleResources> = HashMap::new();

        #[cfg(feature = "locales-en-us")]
        {
            resources.insert(
                "en-US".to_string(),
                build_bundle(
                    Locale::en_us(),
                    &[
                        EN_US_CORE,
                        EN_US_CONVERT,
                        EN_US_SIGN,
                        EN_US_ERROR,
                        EN_US_EDIT,
                        EN_US_SEARCH,
                        EN_US_CLI,
                    ],
                ),
            );
        }

        #[cfg(feature = "locales-zh-cn")]
        {
            resources.insert(
                "zh-CN".to_string(),
                build_bundle(
                    Locale::zh_cn(),
                    &[
                        ZH_CN_CORE,
                        ZH_CN_CONVERT,
                        ZH_CN_SIGN,
                        ZH_CN_ERROR,
                        ZH_CN_EDIT,
                        ZH_CN_SEARCH,
                        ZH_CN_CLI,
                    ],
                ),
            );
        }

        // Rebuild rather than append: Fluent resolves a key from the first
        // resource that defines it, so custom sources are prepended and win.
        for (locale, ftl) in &self.extra {
            let mut sources: Vec<String> = resources
                .get(locale.tag())
                .map(|r| r.sources.clone())
                .unwrap_or_default();
            sources.insert(0, ftl.clone());

            let rebuilt = build_bundle_from_sources(locale.clone(), &sources);
            resources.insert(locale.tag().to_string(), rebuilt);
        }

        let available: Vec<Locale> = BUILTIN_LOCALES
            .iter()
            .filter(|t| resources.contains_key(**t))
            .map(|t| Locale::new(t))
            .collect();

        let current = match self.locale {
            Some(l) => {
                if resources.contains_key(l.tag()) {
                    l
                } else {
                    tracing::warn!(
                        requested = l.tag(),
                        "requested locale unavailable, negotiating"
                    );
                    self.negotiation.negotiate(&available)
                }
            }
            None => self.negotiation.negotiate(&available),
        };

        I18n {
            current,
            available,
            resources,
        }
    }
}

/// Build resources for one locale from borrowed sources.
fn build_bundle(locale: Locale, sources: &[&str]) -> LocaleResources {
    let owned: Vec<String> = sources.iter().map(|s| (*s).to_string()).collect();
    build_bundle_from_sources(locale, &owned)
}

/// Build resources from an ordered source list; later entries win.
fn build_bundle_from_sources(locale: Locale, sources: &[String]) -> LocaleResources {
    let langid = locale.tag().parse().unwrap_or_default();
    let mut bundle = ConcurrentFluentBundle::new_concurrent(vec![langid]);
    bundle.set_use_isolating(false);

    for src in sources {
        match FluentResource::try_new(src.clone()) {
            Ok(res) => {
                if let Err(errs) = bundle.add_resource(res) {
                    tracing::error!(locale = locale.tag(), ?errs, "failed to load FTL resource");
                }
            }
            Err((_, errs)) => {
                tracing::error!(locale = locale.tag(), ?errs, "FTL syntax error");
            }
        }
    }

    LocaleResources {
        locale,
        bundle: Arc::new(bundle),
        sources: sources.to_vec(),
    }
}

/// Every message key, in one static list.
/// Mirrors [`MessageId`] one to one; adding a variant without updating this list
/// fails the consistency test.
pub fn all_keys() -> &'static [&'static str] {
    &[
        "app-name",
        "app-ready",
        "app-loading",
        "app-cancelled",
        "common-confirm",
        "common-cancel",
        "common-save",
        "common-close",
        "common-retry",
        "common-delete",
        "locale-name-en",
        "locale-name-zh",
        "convert-start",
        "convert-progress",
        "convert-done",
        "convert-failed",
        "convert-output-format",
        "convert-page-range",
        "convert-dpi",
        "convert-keep-bookmarks",
        "convert-keep-signatures",
        "sign-valid-until",
        "sign-covers",
        "sign-seal-text",
        "sign-seal-details",
        "sign-valid",
        "sign-invalid",
        "sign-unverified",
        "sign-untrusted",
        "sign-expired",
        "sign-integrity-ok",
        "sign-tampered",
        "sign-count",
        "sign-signer",
        "sign-signed-at",
        "sign-algorithm",
        "sign-issuer",
        "sign-export-report",
        "sign-panel-title",
        "annot-panel-title",
        "annot-count",
        "annot-none",
        "compare-panel-title",
        "compare-open-second",
        "compare-identical",
        "compare-changed",
        "compare-page",
        "edit-confirm-delete-pages",
        "edit-warn-invalidate-signature",
        "edit-undo",
        "edit-redo",
        "search-result-count",
        "search-placeholder",
        "err-not-ofd",
        "err-password-required",
        "err-page-out-of-range",
        "err-package-corrupted",
        "err-font-missing",
        "err-dangling-reference",
        "err-resource-limit",
        "err-io",
        "err-unsupported",
        "cli-pages",
        "cli-page-size",
        "cli-version",
        "cli-embedded-fonts",
        "cli-font-substituted",
        "cli-font-missing",
        "cli-missing-glyphs",
        "cli-missing-images",
        "cli-signatures",
        "cli-no-pages",
        "cli-wrote",
        "cli-rendered-page",
        "cli-converted",
        "cli-search-hit",
        "cli-audit-clean",
        "cli-audit-finding",
        "cli-print-plan",
        "cli-sign-entry",
        "cli-cert-validity",
        "cli-sign-provider",
        "cli-sign-coverage",
        "cli-sign-stamps",
        "cli-sign-seal",
        "cli-sign-none",
        "cli-error",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english() -> I18n {
        I18n::with_locale(Locale::en_us())
    }

    fn chinese() -> I18n {
        I18n::with_locale(Locale::zh_cn())
    }

    #[test]
    fn every_message_id_key_appears_in_the_static_list() {
        // `all_keys` drives the missing-key check, so drift here would silently
        // disable it. The list has no duplicates and matches MessageId one to one.
        let keys = all_keys();
        let mut sorted = keys.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "all_keys contains duplicates");

        for key in keys {
            let id = MessageId::from_key(key)
                .unwrap_or_else(|| panic!("{key} has no MessageId variant"));
            assert_eq!(id.key(), *key, "MessageId round trip failed for {key}");
        }
        // The reverse direction also has to hold, or a variant was forgotten.
        assert_eq!(MessageId::ALL.len(), keys.len());
    }

    #[test]
    fn built_catalogues_cover_every_key() {
        let i18n = english();
        for locale in i18n.available_locales() {
            let missing = i18n.missing_keys(locale);
            assert!(
                missing.is_empty(),
                "{} is missing {missing:?}",
                locale.tag()
            );
        }
    }

    #[test]
    fn plain_messages_render_per_locale() {
        assert_eq!(english().text(MessageId::AppReady), "Ready");
        assert_eq!(chinese().text(MessageId::AppReady), "就绪");
        // The product name is a proper noun and identical in both catalogues.
        assert_eq!(english().text(MessageId::AppName), "sOFD");
        assert_eq!(chinese().text(MessageId::AppName), "sOFD");
    }

    #[test]
    fn arguments_are_substituted_in_the_message() {
        let en = english().text_with(
            MessageId::AppLoading,
            [("percent", fluent::FluentValue::from(42))],
        );
        assert_eq!(en, "Loading… (42%)");

        let zh = chinese().text_with(
            MessageId::AppLoading,
            [("percent", fluent::FluentValue::from(42))],
        );
        assert_eq!(zh, "加载中…（42%）");
    }

    #[test]
    fn argument_order_follows_the_catalogue_not_the_call_site() {
        // Chinese and English place a converter's progress and total in the same
        // order, but the values are still passed separately so a catalogue is
        // free to move them.
        let args = [
            ("done", fluent::FluentValue::from(3)),
            ("total", fluent::FluentValue::from(12)),
        ];
        assert_eq!(
            english().text_with(MessageId::ConvertProgress, args.clone()),
            "Converting: 3 / 12"
        );
        assert_eq!(
            chinese().text_with(MessageId::ConvertProgress, args),
            "正在转换：3 / 12"
        );
    }

    #[test]
    fn string_arguments_are_quoted_where_the_catalogue_says_so() {
        let text = english().text_with(
            MessageId::ErrFontMissing,
            [("family", fluent::FluentValue::from("SimSun"))],
        );
        assert_eq!(
            text,
            "Font \"SimSun\" is missing; text may not match the original"
        );
    }

    #[test]
    fn missing_arguments_do_not_panic() {
        // A caller that forgets a variable should get a degraded string, never a
        // crash in the UI thread.
        let text = english().text(MessageId::ErrPageOutOfRange);
        assert!(text.contains("out of range"), "unexpected text: {text}");
    }

    #[test]
    fn unknown_keys_fall_through_to_the_key_itself() {
        // A key no catalogue defines must degrade to the key, not to an empty
        // string, so a missing translation is visible rather than invisible.
        let i18n = english();
        assert_eq!(i18n.text_key("no-such-key"), "no-such-key");
    }

    #[test]
    fn a_raw_key_reaches_the_bundle() {
        // The reader owns its catalogue and holds keys, not ids, so the
        // key-based entry point has to reach the same bundle.
        let i18n = english();
        assert_eq!(i18n.text_key("app-name"), "sOFD");
    }

    #[test]
    fn set_locale_switches_between_builtin_catalogues() {
        let mut i18n = english();
        assert_eq!(i18n.locale(), &Locale::en_us());

        i18n.set_locale(Locale::zh_cn());
        assert_eq!(i18n.locale(), &Locale::zh_cn());
        assert_eq!(i18n.text(MessageId::CommonCancel), "取消");
    }

    #[test]
    fn set_locale_ignores_a_locale_without_resources() {
        let mut i18n = english();
        i18n.set_locale(Locale::new("fr-FR"));
        assert_eq!(
            i18n.locale(),
            &Locale::en_us(),
            "an unavailable locale must not take effect"
        );
        assert_eq!(i18n.text(MessageId::CommonSave), "Save");
    }

    #[test]
    fn available_locales_lists_the_compiled_catalogues() {
        let i18n = english();
        let tags: Vec<&str> = i18n.available_locales().iter().map(Locale::tag).collect();
        assert!(tags.contains(&"en-US"));
        assert!(tags.contains(&"zh-CN"));
        assert_eq!(i18n.resource_count(), tags.len());
    }

    #[test]
    fn a_normal_build_is_not_english_only() {
        assert!(!english().is_english_only());
    }

    #[test]
    fn builder_negotiates_when_the_requested_locale_is_absent() {
        // Requesting fr-FR with only en-US and zh-CN built must land on a real
        // catalogue rather than an empty one.
        let i18n = I18n::builder().locale(Locale::new("fr-FR")).build();
        let picked = i18n.locale().tag().to_string();
        assert!(
            picked == "en-US" || picked == "zh-CN",
            "negotiated to an unknown locale: {picked}"
        );
        assert!(!i18n.text(MessageId::CommonConfirm).is_empty());
    }

    #[test]
    fn missing_keys_reports_nothing_for_an_unbuilt_locale() {
        let i18n = english();
        assert!(i18n.missing_keys(&Locale::new("ja-JP")).is_empty());
    }

    #[test]
    fn a_custom_catalogue_overrides_the_builtin_key() {
        let i18n = I18n::builder()
            .locale(Locale::en_us())
            .add_resource(Locale::en_us(), "common-save = Store\n")
            .build();
        assert_eq!(i18n.text(MessageId::CommonSave), "Store");
        // Untouched keys still come from the built-in catalogue.
        assert_eq!(i18n.text(MessageId::CommonCancel), "Cancel");
    }

    #[test]
    fn the_last_custom_catalogue_added_wins_over_earlier_ones() {
        // Fluent takes a key from the first resource that defines it, and custom
        // sources are prepended, so the most recently added one is consulted first.
        let i18n = I18n::builder()
            .locale(Locale::en_us())
            .add_resource(Locale::en_us(), "common-save = First\n")
            .add_resource(Locale::en_us(), "common-save = Second\n")
            .build();
        assert_eq!(i18n.text(MessageId::CommonSave), "Second");
    }

    #[test]
    fn a_custom_catalogue_can_supply_a_key_for_an_unbuilt_locale() {
        let i18n = I18n::builder()
            .locale(Locale::new("de-DE"))
            .add_resource(Locale::new("de-DE"), "common-save = Sichern\n")
            .build();
        assert_eq!(i18n.locale().tag(), "de-DE");
        assert_eq!(i18n.text(MessageId::CommonSave), "Sichern");
    }

    #[test]
    fn a_broken_custom_catalogue_is_skipped_not_fatal() {
        let i18n = I18n::builder()
            .locale(Locale::en_us())
            .add_resource(Locale::en_us(), "this is not = = valid ftl {\n")
            .build();
        // Built-in messages survive; only the injected source is discarded.
        assert_eq!(i18n.text(MessageId::CommonSave), "Save");
    }

    #[test]
    fn debug_output_lists_locale_without_dumping_catalogues() {
        let text = format!("{:?}", english());
        assert!(text.contains("I18n"), "unexpected debug output: {text}");
        assert!(text.contains("en-US"));
    }
}
