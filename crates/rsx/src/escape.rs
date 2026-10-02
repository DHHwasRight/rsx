//! HTML escaping.
//!
//! The escape set matches Go's `templ` and `html/template`: `&`, `<`, `>`, `"`
//! and `'`. Quotes use numeric references rather than `&quot;`/`&apos;` because
//! `&apos;` is not defined in HTML 4 and some legacy parsers mishandle it.

/// Lookup table mapping a byte to its replacement, or an empty string when the
/// byte needs no escaping. A 256-entry table keeps the hot loop branch-light.
static ESCAPES: [&str; 256] = {
    let mut table = [""; 256];
    table[b'&' as usize] = "&amp;";
    table[b'<' as usize] = "&lt;";
    table[b'>' as usize] = "&gt;";
    table[b'"' as usize] = "&#34;";
    table[b'\'' as usize] = "&#39;";
    table
};

/// Appends `s` to `buf`, replacing HTML-significant bytes with entities.
///
/// Runs of unescaped bytes are copied in one `extend_from_slice` rather than
/// byte by byte, so text with no special characters costs a single memcpy.
#[inline]
pub fn escape_into(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let mut last = 0;

    for (i, &b) in bytes.iter().enumerate() {
        // Multi-byte UTF-8 continuation bytes are all >= 0x80 and every escaped
        // byte is ASCII, so scanning bytes never splits a character.
        let esc = ESCAPES[b as usize];
        if esc.is_empty() {
            continue;
        }
        if i > last {
            buf.extend_from_slice(&bytes[last..i]);
        }
        buf.extend_from_slice(esc.as_bytes());
        last = i + 1;
    }

    if last < bytes.len() {
        buf.extend_from_slice(&bytes[last..]);
    }
}

/// Returns `true` when `s` contains no byte that would need escaping.
#[inline]
pub fn is_clean(s: &str) -> bool {
    s.bytes().all(|b| ESCAPES[b as usize].is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(s: &str) -> String {
        let mut buf = Vec::new();
        escape_into(&mut buf, s);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn leaves_plain_text_alone() {
        assert_eq!(esc("hello world"), "hello world");
        assert_eq!(esc(""), "");
    }

    #[test]
    fn escapes_every_significant_byte() {
        assert_eq!(esc("&<>\"'"), "&amp;&lt;&gt;&#34;&#39;");
    }

    #[test]
    fn escapes_in_context() {
        assert_eq!(
            esc("<script>alert('xss')</script>"),
            "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn preserves_multibyte_characters() {
        assert_eq!(
            esc("caf\u{e9} \u{2014} \u{1f980}"),
            "caf\u{e9} \u{2014} \u{1f980}"
        );
        assert_eq!(esc("\u{1f980}<"), "\u{1f980}&lt;");
    }

    #[test]
    fn is_clean_agrees_with_escape() {
        for s in ["", "abc", "caf\u{e9}", "a&b", "<", "'"] {
            assert_eq!(is_clean(s), esc(s) == s, "mismatch for {s:?}");
        }
    }
}
