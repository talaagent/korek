//! String format checks, one function per korek format. They aim to agree
//! with protovalidate and Zod on common inputs; see `VALIDATION.md`.

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn all(s: &str, f: impl Fn(char) -> bool) -> bool {
    s.chars().all(f)
}

pub fn hostname(s: &str) -> bool {
    let s = s.strip_suffix('.').unwrap_or(s);
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && all(l, |c| c.is_ascii_alphanumeric() || c == '-')
        })
        // The last label is not all digits (that would be an IPv4 address).
        && !s.rsplit('.').next().is_some_and(|l| all(l, |c| c.is_ascii_digit()))
}

pub fn email(s: &str) -> bool {
    let Some((local, domain)) = s.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && all(local, |c| {
            c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~.-".contains(c)
        })
        && domain.contains('.')
        && hostname(domain)
}

pub fn ipv4(s: &str) -> bool {
    s.parse::<Ipv4Addr>().is_ok()
}

pub fn ipv6(s: &str) -> bool {
    s.parse::<Ipv6Addr>().is_ok()
}

pub fn ip(s: &str) -> bool {
    s.parse::<IpAddr>().is_ok()
}

fn prefix(s: &str) -> Option<(IpAddr, u32)> {
    let (addr, len) = s.split_once('/')?;
    if len.is_empty() || (len.len() > 1 && len.starts_with('0')) {
        return None;
    }
    let ip: IpAddr = addr.parse().ok()?;
    let len: u32 = len.parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    (len <= max).then_some((ip, len))
}

/// An address with a prefix length (`192.168.1.5/24`).
pub fn ip_with_prefixlen(s: &str) -> bool {
    prefix(s).is_some()
}

/// A network prefix: host bits are zero (`192.168.1.0/24`).
pub fn cidr(s: &str) -> bool {
    prefix(s).is_some_and(|(ip, len)| match ip {
        IpAddr::V4(a) => len == 32 || u32::from(a).checked_shl(len).unwrap_or(0) == 0,
        IpAddr::V6(a) => len == 128 || u128::from(a).checked_shl(len).unwrap_or(0) == 0,
    })
}

fn scheme(s: &str) -> Option<&str> {
    let (scheme, rest) = s.split_once(':')?;
    let mut c = scheme.chars();
    (c.next()?.is_ascii_alphabetic() && c.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)))
        .then_some(rest)
}

fn uri_chars(s: &str) -> bool {
    !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>\"{}|\\^`".contains(c))
}

/// An absolute URI (`scheme:rest`).
pub fn uri(s: &str) -> bool {
    scheme(s).is_some() && uri_chars(s)
}

/// A URI or a relative reference.
pub fn uri_ref(s: &str) -> bool {
    s.is_empty() || uri_chars(s)
}

/// An absolute URL with an authority (`https://host/…`).
pub fn url(s: &str) -> bool {
    scheme(s).is_some_and(|rest| rest.starts_with("//") && rest.len() > 2) && uri_chars(s)
}

fn hex(s: &str) -> bool {
    all(s, |c| c.is_ascii_hexdigit())
}

pub fn uuid(s: &str) -> bool {
    let mut parts = s.split('-');
    [8, 4, 4, 4, 12]
        .iter()
        .all(|n| parts.next().is_some_and(|x| x.len() == *n && hex(x)))
        && parts.next().is_none()
}

/// A UUID without dashes.
pub fn tuuid(s: &str) -> bool {
    s.len() == 32 && hex(s)
}

pub fn ulid(s: &str) -> bool {
    s.len() == 26
        && s.as_bytes()[0] <= b'7'
        && all(s, |c| {
            c.is_ascii_digit() || (c.is_ascii_alphabetic() && !"ILOUilou".contains(c))
        })
}

fn port(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 5
        && all(s, |c| c.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'))
        && s.parse::<u32>().is_ok_and(|p| p <= 65_535)
}

/// `host:port`, `1.2.3.4:port` or `[::1]:port`.
pub fn host_and_port(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix('[') {
        return rest
            .split_once("]:")
            .is_some_and(|(h, p)| ipv6(h) && port(p));
    }
    s.rsplit_once(':')
        .is_some_and(|(h, p)| (hostname(h) || ipv4(h)) && port(p))
}

/// A hostname or an IP address.
pub fn address(s: &str) -> bool {
    hostname(s) || ip(s)
}

pub fn cuid(s: &str) -> bool {
    s.len() >= 9 && s.starts_with('c') && !s.chars().any(|c| c.is_whitespace() || c == '-')
}

pub fn cuid2(s: &str) -> bool {
    !s.is_empty() && all(s, |c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

pub fn nanoid(s: &str) -> bool {
    s.len() == 21 && all(s, |c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// RFC 3339 date-time with `Z` or an offset.
pub fn datetime(s: &str) -> bool {
    crate::time::parse_timestamp(s).is_some()
}

/// `YYYY-MM-DD`.
pub fn date(s: &str) -> bool {
    crate::time::parse_date(s).is_some()
}

/// `HH:MM[:SS[.frac]]`.
pub fn time(s: &str) -> bool {
    crate::time::is_time(s)
}

fn units(s: &str, order: &str, frac: bool) -> bool {
    let mut pos = 0;
    let mut start = 0;
    for (at, c) in s.char_indices() {
        if c.is_ascii_digit() || (frac && c == '.') {
            continue;
        }
        let Some(i) = order[pos..].find(c) else {
            return false;
        };
        let num = &s[start..at];
        if num.is_empty() || num.parse::<f64>().is_err() {
            return false;
        }
        pos += i + 1;
        start = at + c.len_utf8();
    }
    start == s.len()
}

/// ISO 8601 duration (`P1Y2M3DT4H5M6.5S`, `P2W`).
pub fn iso_duration(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('P') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    if let Some(w) = rest.strip_suffix('W') {
        return !w.is_empty() && all(w, |c| c.is_ascii_digit());
    }
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) if !t.is_empty() => (d, Some(t)),
        Some(_) => return false,
        None => (rest, None),
    };
    units(date, "YMD", false) && time.is_none_or(|t| units(t, "HMS", true))
}

fn b64(s: &str, url: bool) -> bool {
    if url {
        !s.contains('=') && crate::base64::decode_with(s, true).is_some()
    } else {
        s.len().is_multiple_of(4) && crate::base64::decode(s).is_some()
    }
}

pub fn base64(s: &str) -> bool {
    b64(s, false)
}

pub fn base64url(s: &str) -> bool {
    b64(s, true)
}

/// Three base64url segments whose header decodes to a JSON object.
pub fn jwt(s: &str) -> bool {
    let mut p = s.split('.');
    let (Some(header), Some(payload), Some(sig), None) = (p.next(), p.next(), p.next(), p.next())
    else {
        return false;
    };
    [header, payload]
        .iter()
        .all(|x| !x.is_empty() && base64url(x))
        && (sig.is_empty() || base64url(sig))
        && crate::base64::decode_with(header, true)
            .is_some_and(|h| h.trim_ascii_start().starts_with(b"{"))
}

/// `+` and 7 to 15 digits, not starting with 0.
pub fn e164(s: &str) -> bool {
    s.strip_prefix('+').is_some_and(|d| {
        (7..=15).contains(&d.len()) && !d.starts_with('0') && all(d, |c| c.is_ascii_digit())
    })
}

/// Only emoji (needs the `regex` feature; without it, any non-ASCII text).
pub fn emoji(s: &str) -> bool {
    #[cfg(feature = "regex")]
    {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        crate::matches(
            &RE,
            r"^(\p{Extended_Pictographic}|\p{Emoji_Component})+$",
            s,
        )
    }
    #[cfg(not(feature = "regex"))]
    {
        !s.is_empty() && !s.is_ascii()
    }
}

pub fn lowercase(s: &str) -> bool {
    s == s.to_lowercase()
}

pub fn uppercase(s: &str) -> bool {
    s == s.to_uppercase()
}

/// The check for a format by its korek name.
pub fn by_name(name: &str) -> Option<fn(&str) -> bool> {
    Some(match name {
        "email" => email,
        "hostname" => hostname,
        "ip" => ip,
        "ipv4" => ipv4,
        "ipv6" => ipv6,
        "ip_with_prefixlen" => ip_with_prefixlen,
        "cidr" => cidr,
        "uri" => uri,
        "uri_ref" => uri_ref,
        "url" => url,
        "uuid" => uuid,
        "tuuid" => tuuid,
        "ulid" => ulid,
        "host_and_port" => host_and_port,
        "address" => address,
        "cuid" => cuid,
        "cuid2" => cuid2,
        "nanoid" => nanoid,
        "datetime" => datetime,
        "date" => date,
        "time" => time,
        "iso_duration" => iso_duration,
        "base64" => base64,
        "base64url" => base64url,
        "jwt" => jwt,
        "e164" => e164,
        "emoji" => emoji,
        "lowercase" => lowercase,
        "uppercase" => uppercase,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert!(email("a.b+c@example.com") && !email("a@b") && !email("@x.com"));
        assert!(hostname("a-b.example.com") && !hostname("-a.com") && !hostname("1.2.3.4"));
        assert!(cidr("10.0.0.0/8") && !cidr("10.0.0.1/8") && ip_with_prefixlen("10.0.0.1/8"));
        assert!(uuid("123e4567-e89b-12d3-a456-426614174000") && !uuid("123"));
        assert!(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV") && !ulid("81ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(
            host_and_port("example.com:443")
                && host_and_port("[::1]:80")
                && !host_and_port("x:99999")
        );
        assert!(
            iso_duration("P1DT2H")
                && iso_duration("PT0.5S")
                && !iso_duration("P")
                && !iso_duration("PT")
        );
        assert!(url("https://x.test/a") && !url("mailto:a@b.c") && uri("mailto:a@b.c"));
        assert!(e164("+14155552671") && !e164("+0123"));
        assert!(emoji("👍") && !emoji("a"));
        assert!(base64("Zm9v") && !base64("Zm9") && base64url("Zm9v_-"));
    }
}
