//! The interface languages (doc 34, section 8): the catalogs in `locales/`, the choice of language and the
//! formats that depend on it (numbers, sizes, durations, dates).

use std::fmt::Display;

/// The languages the interface speaks. `en-US` is the source and the fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    EnUs,
    PtBr,
}

/// Every language, in the order the language chooser shows them.
pub const LANGS: [Lang; 2] = [Lang::EnUs, Lang::PtBr];

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::EnUs => "en-US",
            Lang::PtBr => "pt-BR",
        }
    }

    /// The language's own name, as the chooser shows it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::EnUs => "English",
            Lang::PtBr => "Português",
        }
    }

    /// `pt`, `pt_BR`, `pt-BR.UTF-8` and `en_US.UTF-8` are all understood; anything else is `None`.
    pub fn parse(text: &str) -> Option<Lang> {
        let base = text.split(['.', '@']).next().unwrap_or("");
        let normalized = base.replace('_', "-").to_ascii_lowercase();
        match normalized.split('-').next().unwrap_or("") {
            "pt" => Some(Lang::PtBr),
            "en" => Some(Lang::EnUs),
            _ => None,
        }
    }

    pub fn next(self) -> Lang {
        match self {
            Lang::EnUs => Lang::PtBr,
            Lang::PtBr => Lang::EnUs,
        }
    }

    /// `true` when `n` takes the singular form (`one`); `pt` also counts 0 as singular.
    pub fn is_one(self, n: u64) -> bool {
        match self {
            Lang::EnUs => n == 1,
            Lang::PtBr => n <= 1,
        }
    }
}

/// Chooses the language, the first that exists: `--lang`, `TABA_CLI_LANG`, `ui.language`, the system's
/// locale (`LC_ALL`, `LC_MESSAGES`, `LANG`, then the operating system's) and `en-US`.
pub fn detect(
    flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    saved: Option<&str>,
    system: Option<&str>,
) -> Lang {
    let non_empty = |name: &str| env(name).filter(|v| !v.is_empty());
    let candidates = [
        flag.map(str::to_owned),
        non_empty("TABA_CLI_LANG"),
        saved.map(str::to_owned),
        non_empty("LC_ALL"),
        non_empty("LC_MESSAGES"),
        non_empty("LANG"),
        system.map(str::to_owned),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|c| Lang::parse(&c))
        .unwrap_or_default()
}

/// The text of a catalog key. A key missing from the language falls back to `en-US` and then to the key
/// itself: it never breaks the screen (the parity test catches it before the release).
pub fn tr(lang: Lang, key: &str) -> String {
    let text = crate::_rust_i18n_translate(lang.code(), key).into_owned();
    // A key that is nowhere comes back prefixed with the locale: give back the bare key.
    let missing = |code: &str| text == format!("{code}.{key}");
    if missing(lang.code()) || missing(Lang::EnUs.code()) {
        key.to_owned()
    } else {
        text
    }
}

/// The text of a key with its `%{name}` variables replaced.
pub fn tr_with(lang: Lang, key: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut text = tr(lang, key);
    for (name, value) in args {
        text = text.replace(&format!("%{{{name}}}"), &value.to_string());
    }
    text
}

/// The text of `key.one` or `key.other` by the count, with `%{n}` replaced.
pub fn tr_n(lang: Lang, key: &str, n: u64) -> String {
    let form = if lang.is_one(n) { "one" } else { "other" };
    tr_with(lang, &format!("{key}.{form}"), &[("n", &group(lang, n))])
}

/// The variables (`%{name}`) a catalog text uses, sorted.
pub fn variables(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("%{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else { break };
        found.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    found.sort();
    found.dedup();
    found
}

/// An integer with the language's thousands separator.
pub fn group(lang: Lang, n: u64) -> String {
    let digits = n.to_string();
    let separator = match lang {
        Lang::EnUs => ',',
        Lang::PtBr => '.',
    };
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// A number with `places` decimals and the language's decimal mark.
pub fn decimal(lang: Lang, value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    match lang {
        Lang::EnUs => text,
        Lang::PtBr => text.replace('.', ","),
    }
}

/// A size in binary units (`KiB`, `MiB`, `GiB`...), the same in both languages.
pub fn bytes(lang: Lang, n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{} {}", decimal(lang, value, 1), UNITS[unit])
    }
}

/// A length of time as `1d 02h`, `3h 05m`, `4m 07s` or `9s`.
pub fn duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h:02}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// The calendar fields of an instant, in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_ms(ms: i64) -> Civil {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Civil {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: (rem / 60 % 60) as u32,
        second: (rem % 60) as u32,
    }
}

/// An RFC 3339 instant (`2026-10-15T12:00:00.123Z`, or with an offset) as milliseconds since the epoch.
pub fn parse_instant(text: &str) -> Option<i64> {
    let (date, time) = text.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let (y, m, day) = (
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<u32>().ok()?,
        d.next()?.parse::<u32>().ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let split = time.find(['Z', 'z', '+', '-']).unwrap_or(time.len());
    let (clock, zone) = time.split_at(split);
    let mut c = clock.split(':');
    let hour = c.next()?.parse::<i64>().ok()?;
    let minute = c.next()?.parse::<i64>().ok()?;
    let seconds = c.next().unwrap_or("0");
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second = whole.parse::<i64>().ok()?;
    let millis = format!("{fraction:0<3}")[..3].parse::<i64>().ok()?;
    let offset = match zone.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let (h, m) = zone[1..].split_once(':').unwrap_or((&zone[1..], "0"));
            let minutes = h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?;
            if sign == '+' {
                minutes
            } else {
                -minutes
            }
        }
        _ => 0,
    };
    let days = days_from_civil(y, m, day);
    Some(((days * 86_400 + hour * 3600 + minute * 60 + second - offset * 60) * 1000) + millis)
}

/// A date and time in UTC, in the language's order (`2026-10-15 12:00:00` or `15/10/2026 12:00:00`).
pub fn date_time(lang: Lang, ms: i64) -> String {
    let c = civil_from_ms(ms);
    let clock = format!("{:02}:{:02}:{:02}", c.hour, c.minute, c.second);
    match lang {
        Lang::EnUs => format!("{:04}-{:02}-{:02} {clock}", c.year, c.month, c.day),
        Lang::PtBr => format!("{:02}/{:02}/{:04} {clock}", c.day, c.month, c.year),
    }
}

/// The time of day, `HH:MM:SS` in UTC.
pub fn clock(ms: i64) -> String {
    let c = civil_from_ms(ms);
    format!("{:02}:{:02}:{:02}", c.hour, c.minute, c.second)
}

#[cfg(test)]
mod tests;
