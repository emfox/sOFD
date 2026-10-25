//! Localized formatting of numbers, dates, file sizes and percentages.
//! Never assemble these by hand: date order, digit grouping and the decimal mark
//! all vary by language, so the rules live here.

use crate::locale::Locale;

/// How to interpret file size units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeUnitSystem {
    /// Binary, 1 KB is 1024 bytes.
    #[default]
    Binary,
    /// Decimal, 1 kB is 1000 bytes.
    Decimal,
}

/// Locale-aware formatter.
#[derive(Debug, Clone)]
pub struct LocalizedFormat {
    locale: Locale,
    size_system: SizeUnitSystem,
}

impl LocalizedFormat {
    /// Create a formatter for a locale.
    pub fn new(locale: Locale) -> Self {
        Self {
            locale,
            size_system: SizeUnitSystem::Binary,
        }
    }

    /// Choose the unit system for file sizes.
    pub fn with_size_system(mut self, s: SizeUnitSystem) -> Self {
        self.size_system = s;
        self
    }

    /// The locale in use.
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// Format an integer with grouping separators.
    pub fn integer(&self, n: i64) -> String {
        group_digits(n, separator_for(self.locale.tag()))
    }

    /// Format a decimal with the locale decimal mark.
    pub fn decimal(&self, v: f64, fraction_digits: usize) -> String {
        let s = format!("{v:.fraction_digits$}");
        let (int_part, frac_part) = s.split_once('.').unwrap_or((s.as_str(), ""));
        let int_grouped = group_digits(
            int_part.parse::<i64>().unwrap_or(0),
            separator_for(self.locale.tag()),
        );
        let dot = decimal_point_for(self.locale.tag());
        if fraction_digits == 0 {
            int_grouped
        } else {
            format!("{int_grouped}{dot}{frac_part}")
        }
    }

    /// Format a ratio as a percentage.
    pub fn percent(&self, ratio: f64, fraction_digits: usize) -> String {
        format!(
            "{}{}",
            self.decimal(ratio * 100.0, fraction_digits),
            percent_sign_for(self.locale.tag())
        )
    }

    /// Format a byte count with an automatically chosen unit.
    pub fn file_size(&self, bytes: u64) -> String {
        let (base, units) = match self.size_system {
            SizeUnitSystem::Binary => (1024.0, ["B", "KB", "MB", "GB", "TB"]),
            SizeUnitSystem::Decimal => (1000.0, ["B", "kB", "MB", "GB", "TB"]),
        };

        let mut size = bytes as f64;
        let mut idx = 0;
        while size >= base && idx < units.len() - 1 {
            size /= base;
            idx += 1;
        }

        if idx == 0 {
            format!("{} {}", self.integer(bytes as i64), units[idx])
        } else if size < 10.0 {
            format!("{} {}", self.decimal(size, 1), units[idx])
        } else {
            format!("{} {}", self.integer(size.round() as i64), units[idx])
        }
    }

    /// Format a Unix timestamp as date and time in the locale's style.
    pub fn datetime(&self, unix_secs: i64) -> String {
        let (y, m, d, hh, mm) = civil_from_unix(unix_secs);

        match self.locale.language() {
            "zh" => {
                let (h12, ampm) = to_hour12(hh, "上午", "下午");
                format!("{y}年{m}月{d}日 {ampm}{h12}:{mm:02}")
            }
            "ja" => {
                let (h12, ampm) = to_hour12(hh, "午前", "午後");
                format!("{y}年{m}月{d}日 {ampm}{h12}:{mm:02}")
            }
            _ => {
                let mon = month_abbr(m);
                let (h12, ampm) = match hh {
                    0 => (12, "AM"),
                    h if h < 12 => (h, "AM"),
                    12 => (12, "PM"),
                    h => (h - 12, "PM"),
                };
                format!("{mon} {d}, {y}, {h12}:{mm:02} {ampm}")
            }
        }
    }

    /// Format a Unix timestamp as a date with no time.
    pub fn date(&self, unix_secs: i64) -> String {
        let (y, m, d, _, _) = civil_from_unix(unix_secs);
        match self.locale.language() {
            "zh" => format!("{y}年{m}月{d}日"),
            "ja" => format!("{y}年{m}月{d}日"),
            _ => format!("{} {d}, {y}", month_abbr(m)),
        }
    }

    /// Describe a page size, for example `210 x 297 mm`.
    pub fn page_size(&self, w_mm: f64, h_mm: f64) -> String {
        let dot = decimal_point_for(self.locale.tag());
        let fmt = |v: f64| {
            let s = format!("{v:.1}");
            if dot == '.' {
                s
            } else {
                s.replace('.', &dot.to_string())
            }
        };
        format!("{} × {} mm", fmt(w_mm), fmt(h_mm))
    }
}

/// Twelve-hour clock with an AM/PM label in the given language.
fn to_hour12<'a>(hh: u32, am: &'a str, pm: &'a str) -> (u32, &'a str) {
    match hh {
        0 => (12, am),
        h if h < 12 => (h, am),
        12 => (12, pm),
        h => (h - 12, pm),
    }
}

/// English month abbreviation, falling back to January.
fn month_abbr(m: u32) -> &'static str {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .get((m as usize).saturating_sub(1))
        .copied()
        .unwrap_or("Jan")
}

/// Digit grouping separator for a language.
fn separator_for(tag: &str) -> char {
    match tag.split('-').next().unwrap_or(tag) {
        // A narrow no-break space, which looks like a plain space in source.
        "fr" | "ru" | "sv" | "pl" | "cs" | "uk" => '\u{202F}',
        "de" | "it" | "es" | "pt" | "nl" | "da" | "tr" => '.',
        _ => ',',
    }
}

/// Decimal point for a language.
fn decimal_point_for(tag: &str) -> char {
    match tag.split('-').next().unwrap_or(tag) {
        "de" | "it" | "es" | "fr" | "pt" | "ru" | "nl" | "tr" => ',',
        _ => '.',
    }
}

/// Percent symbol; the caller decides whether it leads or trails.
fn percent_sign_for(tag: &str) -> &'static str {
    let _ = tag;
    "%"
}

/// Insert grouping separators every three digits.
fn group_digits(n: i64, sep: char) -> String {
    let neg = n < 0;
    let s = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);

    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }

    if neg { format!("-{out}") } else { out }
}

/// Convert a Unix timestamp to a civil date using Howard Hinnant's algorithm.
/// Returns `(year, month, day, hour, minute)` in UTC; callers needing local time
/// should shift the timestamp first.
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hh = (rem / 3600) as u32;
    let mm = ((rem % 3600) / 60) as u32;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };

    (y, m, d, hh, mm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> LocalizedFormat {
        LocalizedFormat::new(Locale::en_us())
    }

    fn zh() -> LocalizedFormat {
        LocalizedFormat::new(Locale::zh_cn())
    }

    #[test]
    fn integers_are_grouped_every_three_digits() {
        assert_eq!(en().integer(1234567), "1,234,567");
        assert_eq!(en().integer(999), "999");
        assert_eq!(en().integer(1000), "1,000");
    }

    #[test]
    fn grouping_uses_the_locale_separator() {
        // German uses a period as the grouping mark.
        let de = LocalizedFormat::new(Locale::new("de-DE"));
        assert_eq!(de.integer(1234567), "1.234.567");
    }

    #[test]
    fn negative_integers_keep_the_sign_outside_the_grouping() {
        assert_eq!(en().integer(-1234), "-1,234");
    }

    #[test]
    fn zero_and_single_digits_are_not_grouped() {
        assert_eq!(en().integer(0), "0");
        assert_eq!(en().integer(7), "7");
        assert_eq!(en().integer(-7), "-7");
    }

    #[test]
    fn decimals_use_the_locale_decimal_mark() {
        assert_eq!(en().decimal(1234.5, 1), "1,234.5");
        // German swaps the two marks: grouping dot, decimal comma.
        let de = LocalizedFormat::new(Locale::new("de-DE"));
        assert_eq!(de.decimal(1234.5, 1), "1.234,5");
    }

    #[test]
    fn decimal_with_zero_fraction_digits_has_no_point() {
        assert_eq!(en().decimal(12.6, 0), "13");
    }

    #[test]
    fn decimal_rounds_to_the_requested_precision() {
        assert_eq!(en().decimal(1.005, 2), "1.00");
        assert_eq!(en().decimal(2.345, 2), "2.35");
    }

    #[test]
    fn percent_multiplies_by_one_hundred_and_appends_the_sign() {
        assert_eq!(en().percent(0.5, 0), "50%");
        assert_eq!(en().percent(0.1234, 1), "12.3%");
    }

    #[test]
    fn percent_handles_zero_and_full() {
        assert_eq!(en().percent(0.0, 0), "0%");
        assert_eq!(en().percent(1.0, 0), "100%");
    }

    #[test]
    fn byte_counts_below_the_base_stay_in_bytes() {
        assert_eq!(en().file_size(0), "0 B");
        assert_eq!(en().file_size(512), "512 B");
        assert_eq!(en().file_size(1023), "1,023 B");
    }

    #[test]
    fn binary_file_sizes_use_1024() {
        assert_eq!(en().file_size(1024), "1.0 KB");
        assert_eq!(en().file_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn decimal_file_sizes_use_1000() {
        let dec = en().with_size_system(SizeUnitSystem::Decimal);
        assert_eq!(dec.file_size(1000), "1.0 kB");
        assert_eq!(dec.file_size(1_000_000), "1.0 MB");
    }

    #[test]
    fn file_sizes_at_or_above_ten_drop_the_decimal() {
        // 15 KB prints as a whole number, 1.5 KB keeps one digit.
        assert_eq!(en().file_size(15 * 1024), "15 KB");
        assert_eq!(en().file_size(1536), "1.5 KB");
    }

    #[test]
    fn file_size_stops_at_the_largest_unit() {
        let huge = 1024u64.pow(5) * 4;
        assert!(en().file_size(huge).ends_with(" TB"));
    }

    #[test]
    fn english_date_puts_the_month_first() {
        // 2024-03-05 00:00:00 UTC
        let ts = 1_709_596_800;
        assert_eq!(en().date(ts), "Mar 5, 2024");
    }

    #[test]
    fn chinese_date_uses_the_year_month_day_markers() {
        let ts = 1_709_596_800;
        assert_eq!(zh().date(ts), "2024年3月5日");
    }

    #[test]
    fn english_datetime_uses_a_twelve_hour_clock() {
        // 2024-03-05 13:30:00 UTC
        let ts = 1_709_596_800 + 13 * 3600 + 30 * 60;
        assert_eq!(en().datetime(ts), "Mar 5, 2024, 1:30 PM");
    }

    #[test]
    fn midnight_and_noon_map_to_twelve_on_the_twelve_hour_clock() {
        let base = 1_709_596_800; // 00:00 UTC
        assert!(en().datetime(base).contains("12:00 AM"));
        let noon = base + 12 * 3600;
        assert!(en().datetime(noon).contains("12:00 PM"));
    }

    #[test]
    fn chinese_datetime_uses_chinese_meridiem_markers() {
        let base = 1_709_596_800;
        // 09:05 UTC is before noon.
        let morning = zh().datetime(base + 9 * 3600 + 5 * 60);
        assert_eq!(morning, "2024年3月5日 上午9:05");
        // 15:00 UTC is after noon.
        let afternoon = zh().datetime(base + 15 * 3600);
        assert_eq!(afternoon, "2024年3月5日 下午3:00");
    }

    #[test]
    fn date_formatting_handles_a_leap_day() {
        // 2024-02-29 00:00:00 UTC
        let ts = 1_709_164_800;
        assert_eq!(en().date(ts), "Feb 29, 2024");
    }

    #[test]
    fn page_size_uses_the_multiplication_sign_and_millimetres() {
        assert_eq!(en().page_size(210.0, 297.0), "210.0 × 297.0 mm");
    }

    #[test]
    fn page_size_follows_the_locale_decimal_mark() {
        let de = LocalizedFormat::new(Locale::new("de-DE"));
        assert_eq!(de.page_size(210.0, 297.0), "210,0 × 297,0 mm");
    }

    #[test]
    fn locale_accessor_returns_what_was_configured() {
        assert_eq!(zh().locale(), &Locale::zh_cn());
    }
}
