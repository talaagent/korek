//! Standard base64 (with padding), the JSON form of `bytes`, and a serde
//! adapter: `#[serde(with = "korek_runtime::base64")]`.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn sextet(c: u8, url: bool) -> Option<u32> {
    Some(u32::from(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' if !url => 62,
        b'/' if !url => 63,
        b'-' if url => 62,
        b'_' if url => 63,
        _ => return None,
    }))
}

/// Decodes base64 (`url`: the URL-safe alphabet). Padding is optional.
pub fn decode_with(s: &str, url: bool) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= sextet(*c, url)? << (18 - 6 * i);
        }
        #[allow(clippy::cast_possible_truncation)]
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Some(out)
}

/// Decodes standard base64.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    decode_with(s, false)
}

/// Serializes `Vec<u8>` as base64.
///
/// # Errors
/// From the serializer.
pub fn serialize<S: serde::Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&encode(bytes))
}

/// Deserializes base64 into `Vec<u8>`.
///
/// # Errors
/// When the input is not base64.
pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    decode(&s).ok_or_else(|| serde::de::Error::custom("invalid base64"))
}

/// The same for `Option<Vec<u8>>`.
pub mod option {
    /// # Errors
    /// From the serializer.
    pub fn serialize<S: serde::Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_some(&super::encode(b)),
            None => s.serialize_none(),
        }
    }

    /// # Errors
    /// When the input is not base64.
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        let s = <Option<String> as serde::Deserialize>::deserialize(d)?;
        s.map(|s| super::decode(&s).ok_or_else(|| serde::de::Error::custom("invalid base64")))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trips() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let e = super::encode(s.as_bytes());
            assert_eq!(super::decode(&e).unwrap(), s.as_bytes(), "{e}");
        }
        assert_eq!(super::encode(b"foobar"), "Zm9vYmFy");
        assert!(super::decode("a").is_none());
    }
}
