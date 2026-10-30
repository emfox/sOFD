//! Localization message layer.
//! Engine crates emit structured errors and data only; this layer turns them into
//! text, keeping the engine reusable from C, JS and Python.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bundle;
pub mod format;
pub mod locale;
pub mod messages;

pub use bundle::{I18n, I18nBuilder};
/// A value passed into a message: text or a number. Own alias so callers
/// never name the underlying engine crate in their signatures.
pub type Value<'a> = fluent::FluentValue<'a>;
pub use locale::{Locale, LocaleNegotiation};
pub use messages::MessageId;

/// Locale used whenever a translation is missing.
/// English rather than Chinese, so a gap degrades to readable text instead of a
/// raw message key.
pub const FALLBACK_LOCALE: &str = "en-US";

/// Locales compiled into this build.
pub const BUILTIN_LOCALES: &[&str] = &["en-US", "zh-CN"];
