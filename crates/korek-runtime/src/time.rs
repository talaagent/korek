//! `timestamp` (RFC 3339) and `duration` (`"1.5s"`) in their JSON forms, and
//! [`Duration`](core::time::Duration) codecs for integers with a [`Unit`].

/// Seconds since the Unix epoch of an RFC 3339 timestamp
/// (`2024-01-02T03:04:05.5Z`, `…+02:00`).
pub fn parse_timestamp(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't') {
        return None;
    }
    let days = parse_date(&s[..10])?;
    let (secs, rest) = parse_clock(&s[11..])?;
    let offset = match rest.as_bytes().first()? {
        b'Z' | b'z' if rest.len() == 1 => 0,
        b'+' | b'-' if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let h: i64 = rest[1..3].parse().ok()?;
            let m: i64 = rest[4..6].parse().ok()?;
            let o = h * 3600 + m * 60;
            if rest.starts_with('-') { -o } else { o }
        }
        _ => return None,
    };
    #[allow(clippy::cast_precision_loss)]
    Some((days * 86_400 - offset) as f64 + secs)
}

/// Days since the epoch of `YYYY-MM-DD`, if it is a real date.
pub fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = s[..4].parse().ok()?;
    let m: i64 = s[5..7].parse().ok()?;
    let d: i64 = s[8..10].parse().ok()?;
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > days_in[usize::try_from(m - 1).ok()?] {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Parses `HH:MM:SS[.frac]`, returning seconds into the day and the rest.
fn parse_clock(s: &str) -> Option<(f64, &str)> {
    let b = s.as_bytes();
    if b.len() < 8 || b[2] != b':' || b[5] != b':' {
        return None;
    }
    let h: u32 = s[..2].parse().ok()?;
    let m: u32 = s[3..5].parse().ok()?;
    let sec: u32 = s[6..8].parse().ok()?;
    if h > 23 || m > 59 || sec > 60 {
        return None;
    }
    let mut end = 8;
    let mut frac = 0.0;
    if b.get(8) == Some(&b'.') {
        end = 9 + b[9..].iter().take_while(|c| c.is_ascii_digit()).count();
        if end == 9 {
            return None;
        }
        frac = format!("0{}", &s[8..end]).parse().ok()?;
    }
    Some((f64::from(h * 3600 + m * 60 + sec) + frac, &s[end..]))
}

/// Whether `s` is `HH:MM[:SS[.frac]]` (no offset).
pub fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() == 5 {
        return b[2] == b':'
            && s[..2].parse::<u32>().is_ok_and(|h| h < 24)
            && s[3..].parse::<u32>().is_ok_and(|m| m < 60);
    }
    parse_clock(s).is_some_and(|(_, rest)| rest.is_empty())
}

/// Seconds of a duration in the Protobuf JSON form (`"1.5s"`, `"-3s"`).
pub fn parse_duration(s: &str) -> Option<f64> {
    let n = s.strip_suffix('s')?;
    if n.is_empty() || n.starts_with('+') || n.contains(['e', 'E']) {
        return None;
    }
    let v: f64 = n.parse().ok()?;
    (v.abs() <= 315_576_000_000.0).then_some(v)
}

/// Seconds since the epoch, now.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// A time unit: integer fields and keys with `unit` (`seconds`, …) map to
/// [`Duration`](core::time::Duration) under the Rust option
/// `rust_duration`, keeping the integer JSON form.
pub trait Unit {
    /// Nanoseconds in one unit.
    const NANOS: u128;
}

/// The whole number of `U` in `d`, for rules (saturating).
pub fn to_int<U: Unit>(d: &core::time::Duration) -> i128 {
    i128::try_from(d.as_nanos() / U::NANOS).unwrap_or(i128::MAX)
}

/// `n` units as a `Duration`, if it fits.
pub fn from_int<U: Unit>(n: u64) -> Option<core::time::Duration> {
    let nanos = u128::from(n).checked_mul(U::NANOS)?;
    let secs = u64::try_from(nanos / 1_000_000_000).ok()?;
    #[allow(clippy::cast_possible_truncation)]
    Some(core::time::Duration::new(
        secs,
        (nanos % 1_000_000_000) as u32,
    ))
}

macro_rules! units {
    ($($(#[$m:meta])* $name:ident = $nanos:expr;)*) => {$(
        $(#[$m])*
        pub struct $name;

        impl Unit for $name {
            const NANOS: u128 = $nanos;
        }

        impl crate::with::Codec<core::time::Duration> for $name {
            fn serialize<S: serde::Serializer>(
                v: &core::time::Duration,
                s: S,
            ) -> Result<S::Ok, S::Error> {
                let n = u64::try_from(to_int::<Self>(v))
                    .map_err(|_| serde::ser::Error::custom("duration out of range"))?;
                s.serialize_u64(n)
            }

            fn deserialize<'de, D: serde::Deserializer<'de>>(
                d: D,
            ) -> Result<core::time::Duration, D::Error> {
                let n = <u64 as serde::Deserialize>::deserialize(d)?;
                from_int::<Self>(n)
                    .ok_or_else(|| serde::de::Error::custom("duration out of range"))
            }
        }
    )*};
}

units! {
    /// `unit "nanoseconds"`.
    Nanoseconds = 1;
    /// `unit "microseconds"`.
    Microseconds = 1_000;
    /// `unit "milliseconds"`.
    Milliseconds = 1_000_000;
    /// `unit "seconds"`.
    Seconds = 1_000_000_000;
    /// `unit "minutes"`.
    Minutes = 60_000_000_000;
    /// `unit "hours"`.
    Hours = 3_600_000_000_000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_timestamp("1970-01-02T00:00:01.5Z"), Some(86_401.5));
        assert_eq!(parse_timestamp("1970-01-01T01:00:00+01:00"), Some(0.0));
        assert!(parse_timestamp("2023-02-29T00:00:00Z").is_none());
        assert!(parse_timestamp("2024-02-29T00:00:00").is_none());
        assert_eq!(parse_duration("1.5s"), Some(1.5));
        assert!(parse_duration("1.5").is_none());
        assert!(is_time("12:30") && is_time("12:30:01.25") && !is_time("25:00"));
    }
}
