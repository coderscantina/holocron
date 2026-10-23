//! String helpers that match upstream's truncation and URI decoding.

/// The first `max` characters.
pub fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => s[..at].to_owned(),
        None => s.to_owned(),
    }
}

/// `decodeURIComponent`, keeping the input when it is not valid percent-encoded UTF-8.
pub fn decode_component(s: &str) -> String {
    decode(s, |_| true)
}

/// `decodeURI`: like [`decode_component`] but reserved characters stay encoded.
pub fn decode_uri(s: &str) -> String {
    decode(s, |b| !b";/?:@&=+$,#".contains(&b))
}

fn decode(s: &str, decodes: impl Fn(u8) -> bool) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let byte = s
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            match byte {
                Some(b) if decodes(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                Some(_) => {}
                None => return s.to_owned(),
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_matches_the_browser_functions() {
        assert_eq!(decode_component("caf%C3%A9%2Fbar"), "café/bar");
        assert_eq!(decode_uri("caf%C3%A9%2Fbar"), "café%2Fbar");
        assert_eq!(decode_component("50% off"), "50% off");
        assert_eq!(truncate("héllo", 2), "hé");
    }
}
