//! Validation rules in words, and values with their units humanized.

use alloc::{borrow::ToOwned, boxed::Box, format, string::String, string::ToString, vec::Vec};

use korek_ir::{Format, Json, Rule, RuleKind, Type, Validate, format_number};

use crate::md::code;

/// `n` and the noun, plural unless `n` is 1.
fn count(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `x` as a number with at most two decimals, if it is one.
fn short(x: f64) -> Option<String> {
    let scaled = libm::round(x * 100.0);
    ((scaled - x * 100.0).abs() < 1e-6).then(|| format_number(scaled / 100.0))
}

/// The largest of `units` (`(size, one, many)`, largest first) that `x` is
/// at least one of, with a short quotient.
fn scale(x: f64, units: &[(f64, &str, &str)]) -> Option<String> {
    units.iter().find_map(|&(size, one, many)| {
        let q = x / size;
        if q.abs() < 1.0 && size > units[units.len() - 1].0 {
            return None;
        }
        let n = short(q)?;
        Some(format!("{n} {}", if n == "1" { one } else { many }))
    })
}

const TIME: &[(f64, &str, &str)] = &[
    (604_800.0, "week", "weeks"),
    (86_400.0, "day", "days"),
    (3_600.0, "hour", "hours"),
    (60.0, "minute", "minutes"),
    (1.0, "second", "seconds"),
    (1e-3, "millisecond", "milliseconds"),
    (1e-6, "microsecond", "microseconds"),
    (1e-9, "nanosecond", "nanoseconds"),
];

const BYTES: &[(f64, &str, &str)] = &[
    (1_099_511_627_776.0, "TiB", "TiB"),
    (1_073_741_824.0, "GiB", "GiB"),
    (1_048_576.0, "MiB", "MiB"),
    (1_024.0, "KiB", "KiB"),
    (1.0, "byte", "bytes"),
];

/// Seconds in words (`600` → `10 minutes`).
pub fn seconds(s: f64) -> String {
    if s == 0.0 {
        return "0 seconds".into();
    }
    scale(s, TIME).unwrap_or_else(|| format!("{} seconds", format_number(s)))
}

/// Unit sizes, largest first: `(size, singular, plural)`.
type Units = &'static [(f64, &'static str, &'static str)];

fn unit_size(unit: &str) -> Option<(f64, Units)> {
    let u = unit.trim().to_ascii_lowercase();
    let time = |s| Some((s, TIME));
    let bytes = |s| Some((s, BYTES));
    match u.as_str() {
        "ns" | "nanosecond" | "nanoseconds" => time(1e-9),
        "us" | "µs" | "microsecond" | "microseconds" => time(1e-6),
        "ms" | "millisecond" | "milliseconds" => time(1e-3),
        "s" | "sec" | "secs" | "second" | "seconds" => time(1.0),
        "min" | "mins" | "minute" | "minutes" => time(60.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => time(3_600.0),
        "d" | "day" | "days" => time(86_400.0),
        "b" | "byte" | "bytes" => bytes(1.0),
        "kib" | "kibibyte" | "kibibytes" => bytes(1_024.0),
        "mib" | "mebibyte" | "mebibytes" => bytes(1_048_576.0),
        "gib" | "gibibyte" | "gibibytes" => bytes(1_073_741_824.0),
        "kb" | "kilobyte" | "kilobytes" => bytes(1e3),
        "mb" | "megabyte" | "megabytes" => bytes(1e6),
        "gb" | "gigabyte" | "gigabytes" => bytes(1e9),
        _ => None,
    }
}

/// `n` `unit`s in words (`30000` milliseconds → `30 seconds`), or `n unit`
/// for a unit korek does not know.
pub fn with_unit(n: f64, unit: &str) -> String {
    match unit_size(unit) {
        Some((size, units)) if n != 0.0 => {
            scale(n * size, units).unwrap_or_else(|| format!("{} {unit}", format_number(n)))
        }
        _ => format!("{} {unit}", format_number(n)),
    }
}

/// A protobuf JSON duration (`"600s"`) in seconds.
pub fn duration_secs(v: &Json) -> Option<f64> {
    v.as_str()?.strip_suffix('s')?.parse().ok()
}

/// A value as code, with a humanized form for durations and numbers with
/// a unit: `` `"600s"` (10 minutes) ``, `` `30000` (30 seconds) ``.
pub fn value(v: &Json, ty: &Type, unit: Option<&str>) -> String {
    let text = code(&v.to_string());
    if *ty == Type::Duration
        && let Some(s) = duration_secs(v)
    {
        return format!("{text} ({})", seconds(s));
    }
    match (v.as_f64(), unit) {
        (Some(n), Some(u)) => format!("{text} ({})", with_unit(n, u)),
        _ => text,
    }
}

fn values(vs: &[Json], ty: &Type, unit: Option<&str>) -> String {
    vs.iter()
        .map(|v| value(v, ty, unit))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A well-known format as a noun phrase.
pub fn format(f: Format) -> String {
    let s = match f {
        Format::Email => "an email address",
        Format::Hostname => "a hostname",
        Format::Ip => "an IP address",
        Format::Ipv4 => "an IPv4 address",
        Format::Ipv6 => "an IPv6 address",
        Format::IpWithPrefixlen => "an IP address with a prefix length (`10.0.0.1/8`)",
        Format::Cidr => "a network prefix (`10.0.0.0/8`)",
        Format::Uri => "an absolute URI",
        Format::UriRef => "a URI reference",
        Format::Url => "a URL",
        Format::Uuid => "a UUID",
        Format::Tuuid => "a UUID without dashes",
        Format::Ulid => "a ULID",
        Format::HostAndPort => "a host and port (`host:port`)",
        Format::Address => "a hostname or IP address",
        Format::Cuid => "a CUID",
        Format::Cuid2 => "a CUID2",
        Format::Nanoid => "a Nano ID",
        Format::Datetime => "an RFC 3339 date-time",
        Format::Date => "a date (`YYYY-MM-DD`)",
        Format::Time => "a time of day (`HH:MM[:SS]`)",
        Format::Duration => "an ISO 8601 duration (`P1DT2H`)",
        Format::Base64 => "base64",
        Format::Base64url => "URL-safe base64",
        Format::Jwt => "a JWT",
        Format::E164 => "an E.164 phone number",
        Format::Emoji => "emoji only",
        Format::Lowercase => "lowercase",
        Format::Uppercase => "uppercase",
        _ => return code(f.name()),
    };
    s.to_owned()
}

/// What a length counts for `ty`.
fn len_unit(ty: &Type) -> (&'static str, &'static str) {
    if *ty == Type::Bytes {
        ("byte", "bytes")
    } else {
        ("character", "characters")
    }
}

/// How a rule reads in a sentence ("at least 1 character").
#[allow(clippy::too_many_lines)]
fn rule(r: &Rule, ty: &Type, unit: Option<&str>) -> String {
    let (one, many) = len_unit(ty);
    let v = |x: &Json| value(x, ty, unit);
    match &r.kind {
        RuleKind::Const { value } => format!("exactly {}", v(value)),
        RuleKind::In { values: vs } => format!("one of {}", values(vs, ty, unit)),
        RuleKind::NotIn { values: vs } => format!("none of {}", values(vs, ty, unit)),
        RuleKind::Len { n } => format!("exactly {}", count(*n, one, many)),
        RuleKind::MinLen { n } => format!("at least {}", count(*n, one, many)),
        RuleKind::MaxLen { n } => format!("at most {}", count(*n, one, many)),
        RuleKind::LenBytes { n } => format!("exactly {} (UTF-8)", count(*n, "byte", "bytes")),
        RuleKind::MinBytes { n } => format!("at least {} (UTF-8)", count(*n, "byte", "bytes")),
        RuleKind::MaxBytes { n } => format!("at most {} (UTF-8)", count(*n, "byte", "bytes")),
        RuleKind::Pattern { regex } => format!("matches {}", code(regex)),
        RuleKind::Prefix { value } => format!("starts with {}", code(value)),
        RuleKind::Suffix { value } => format!("ends with {}", code(value)),
        RuleKind::Contains { value } => format!("contains {}", code(value)),
        RuleKind::NotContains { value } => format!("does not contain {}", code(value)),
        RuleKind::Format { format: f } => format(*f),
        RuleKind::Gt { value } => format!("greater than {}", v(value)),
        RuleKind::Gte { value } => format!("at least {}", v(value)),
        RuleKind::Lt { value } => format!("less than {}", v(value)),
        RuleKind::Lte { value } => format!("at most {}", v(value)),
        RuleKind::Finite => "finite (not NaN or infinite)".into(),
        RuleKind::MultipleOf { value } => format!("a multiple of {}", v(value)),
        RuleKind::MinItems { n } => format!("at least {}", count(*n, "item", "items")),
        RuleKind::MaxItems { n } => format!("at most {}", count(*n, "item", "items")),
        RuleKind::Unique => "no duplicate items".into(),
        RuleKind::MinPairs { n } => format!("at least {}", count(*n, "entry", "entries")),
        RuleKind::MaxPairs { n } => format!("at most {}", count(*n, "entry", "entries")),
        RuleKind::Required => "must be set".into(),
        RuleKind::DefinedOnly => "a declared variant".into(),
        RuleKind::GtNow => "in the future".into(),
        RuleKind::LtNow => "in the past".into(),
        RuleKind::Within { duration } => {
            let d = duration_secs(&Json::from(duration.as_str()))
                .map_or_else(|| code(duration), seconds);
            format!("within {d} of now")
        }
        RuleKind::Cel { id, expression, .. } => match &r.message {
            Some(m) => format!("{m} ({})", code(id)),
            None => format!("satisfies {}", code(expression)),
        },
        other => code(other.name()),
    }
}

/// Merges a lower and an upper bound into one phrase ("between 1 and 100").
fn range(rules: &[Rule], ty: &Type, unit: Option<&str>) -> Option<(usize, usize, String)> {
    let lo = rules
        .iter()
        .position(|r| matches!(r.kind, RuleKind::Gte { .. } | RuleKind::MinLen { .. }))?;
    let hi = rules.iter().position(|r| {
        matches!(
            (&rules[lo].kind, &r.kind),
            (RuleKind::Gte { .. }, RuleKind::Lte { .. })
                | (RuleKind::MinLen { .. }, RuleKind::MaxLen { .. })
        )
    })?;
    let text = match (&rules[lo].kind, &rules[hi].kind) {
        (RuleKind::Gte { value: a }, RuleKind::Lte { value: b }) => {
            format!("between {} and {}", value(a, ty, unit), value(b, ty, unit))
        }
        (RuleKind::MinLen { n: a }, RuleKind::MaxLen { n: b }) => {
            format!("{a} to {}", count(*b, len_unit(ty).0, len_unit(ty).1))
        }
        _ => return None,
    };
    Some((lo, hi, text))
}

/// Every rule of `v` for a value of type `ty`, in words, element rules
/// prefixed ("each item: an email address").
pub fn rules(v: &Validate, ty: &Type, unit: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let merged = range(&v.rules, ty, unit);
    for (i, r) in v.rules.iter().enumerate() {
        match &merged {
            Some((lo, _, text)) if *lo == i => out.push(text.clone()),
            Some((_, hi, _)) if *hi == i => {}
            _ => out.push(rule(r, ty, unit)),
        }
    }
    let nested = |what: &str, v: &Option<Box<Validate>>, ty: &Type, out: &mut Vec<String>| {
        if let Some(v) = v {
            let inner = rules(v, ty, unit);
            if !inner.is_empty() {
                out.push(format!("each {what}: {}", inner.join(", ")));
            }
        }
    };
    match ty {
        Type::List(t) => nested("item", &v.items, t, &mut out),
        Type::Map(t) => {
            nested("key", &v.keys, &Type::String, &mut out);
            nested("value", &v.values, t, &mut out);
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes() {
        assert_eq!(seconds(600.0), "10 minutes");
        assert_eq!(seconds(86_400.0), "1 day");
        assert_eq!(seconds(90.0), "1.5 minutes");
        assert_eq!(seconds(100.0), "100 seconds");
        assert_eq!(seconds(1.0), "1 second");
        assert_eq!(seconds(0.25), "250 milliseconds");
        assert_eq!(with_unit(30_000.0, "milliseconds"), "30 seconds");
        assert_eq!(with_unit(65_536.0, "bytes"), "64 KiB");
        assert_eq!(with_unit(5.0, "meters"), "5 meters");
        assert_eq!(with_unit(1.0, "ms"), "1 millisecond");
    }
}
