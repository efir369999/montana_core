//! The plain body of a letter as a phone writes and reads it: mid ‖ 0 ‖ text ‖ 0 ‖ name ‖ 0 ‖ glyph ‖ 0,
//! the fields a quote and a card fill, and an eighth field that names the author by the person's
//! words, all inside one envelope of one size, so a door cannot tell letter lengths apart.

use mt_mnemonic::hmac_sha256;

pub const ENVELOPE_SIZE: usize = 2048;
/// The fields before the author: mid, text, name, glyph, quote, quoted mid, card.
pub const AUTHOR_FIELDS: usize = 7;
pub const AUTHOR_TAG_LEN: usize = 16;
pub const LETTER_AUTHOR_DOMAIN: &[u8] = b"mt-letter-author";

/// The head an agent writes carries no quote: the phone cuts a quote at two hundred grapheme
/// clusters, a count this crate does not hold, so a quote written here could differ from the phone's.
pub fn letter_head(mid: &str, text: &str, name: &str, glyph: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(mid.len() + text.len() + name.len() + glyph.len() + 4);
    for field in [mid, text, name, glyph] {
        body.extend_from_slice(field.as_bytes());
        body.push(0);
    }
    body
}

/// Larger than the envelope is no envelope.
pub fn pad_envelope(body: &[u8]) -> Option<Vec<u8>> {
    if body.len() > ENVELOPE_SIZE {
        return None;
    }
    let mut b = body.to_vec();
    b.resize(ENVELOPE_SIZE, 0);
    Some(b)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_standard(bytes: &[u8]) -> String {
    let mut s = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i > n {
                s.push('=');
            } else {
                s.push(BASE64[((v >> (18 - 6 * i)) & 0x3f) as usize] as char);
            }
        }
    }
    s
}

/// As strict as the phone's Data(base64Encoded:): the standard alphabet, whole quads, the padding
/// only at the end.
pub fn base64_standard_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 4 != 0 {
        return None;
    }
    let quads = b.len() / 4;
    let mut out = Vec::with_capacity(quads * 3);
    for (i, quad) in b.chunks(4).enumerate() {
        let pad = quad.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != quads) {
            return None;
        }
        let mut v = 0u32;
        for c in &quad[..4 - pad] {
            v = (v << 6) | BASE64.iter().position(|x| x == c)? as u32;
        }
        v <<= 6 * pad as u32;
        out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8][..3 - pad]);
    }
    Some(out)
}

/// HMAC-SHA-256 under a branch of the seed only the author's devices derive, over the pipe's
/// secret, the letter's name and its words, cut to sixteen bytes: the correspondent holds a tag it
/// can neither open nor make.
pub fn author_tag(key: &[u8; 32], secret: &[u8], mid: &str, text: &str) -> String {
    let mut m =
        Vec::with_capacity(LETTER_AUTHOR_DOMAIN.len() + 2 + secret.len() + mid.len() + text.len());
    m.extend_from_slice(LETTER_AUTHOR_DOMAIN);
    m.push(0);
    m.extend_from_slice(secret);
    m.extend_from_slice(mid.as_bytes());
    m.push(0);
    m.extend_from_slice(text.as_bytes());
    base64_standard(&hmac_sha256(key, &m)[..AUTHOR_TAG_LEN])
}

/// Compared in constant time against the canonical tag made again under the key.
pub fn author_holds(tag: &str, key: &[u8; 32], secret: &[u8], mid: &str, text: &str) -> bool {
    let want = author_tag(key, secret, mid, text);
    if tag.len() != want.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in tag.bytes().zip(want.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Every field before the author closed and the empty ones written, so the tag stands eighth
/// whatever the letter carries; a body that would outgrow the envelope rides without it.
pub fn with_author(body: &[u8], key: &[u8; 32], secret: &[u8], mid: &str, text: &str) -> Vec<u8> {
    let mut b = body.to_vec();
    if b.last().is_some_and(|x| *x != 0) {
        b.push(0);
    }
    let mut closed = b.iter().filter(|x| **x == 0).count();
    while closed < AUTHOR_FIELDS {
        b.push(0);
        closed += 1;
    }
    if closed != AUTHOR_FIELDS {
        return body.to_vec();
    }
    b.extend_from_slice(author_tag(key, secret, mid, text).as_bytes());
    b.push(0);
    if b.len() <= ENVELOPE_SIZE {
        b
    } else {
        body.to_vec()
    }
}

/// The eight fields of a letter -- mid, text, name, glyph, quote, quoted mid, card, author -- an
/// absent one read empty, a field that is not UTF-8 read empty.
pub fn letter_fields(plain: &[u8]) -> Vec<String> {
    let mut fields = Vec::with_capacity(AUTHOR_FIELDS + 1);
    let mut rest = plain;
    while fields.len() <= AUTHOR_FIELDS {
        match rest.iter().position(|x| *x == 0) {
            Some(z) => {
                fields.push(String::from_utf8(rest[..z].to_vec()).unwrap_or_default());
                rest = &rest[z + 1..];
            },
            None => {
                fields.push(String::from_utf8(rest.to_vec()).unwrap_or_default());
                break;
            },
        }
    }
    while fields.len() <= AUTHOR_FIELDS {
        fields.push(String::new());
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = 0x20 + i as u8;
        }
        k
    }

    fn secret() -> Vec<u8> {
        (0u8..32).collect()
    }

    #[test]
    fn base64_is_rfc_4648() {
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_standard(plain.as_bytes()), enc);
        }
        assert_eq!(base64_standard(&[0xfb, 0xff]), "+/8=");
    }

    // Rejects a decoder that takes a quad cut short, padding inside the text or past two, and a
    // byte outside the alphabet.
    #[test]
    fn base64_decode_is_as_strict_as_the_phone() {
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(
                base64_standard_decode(enc).as_deref(),
                Some(plain.as_bytes())
            );
        }
        assert_eq!(base64_standard_decode("+/8="), Some(vec![0xfb, 0xff]));
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(base64_standard_decode(&base64_standard(&all)), Some(all));
        for bad in [
            "Zg=",
            "Zg=a",
            "Z===",
            "Zg==Zg==",
            "Zm9v!A==",
            "Zm9v YmFy",
            "Zm9-",
        ] {
            assert_eq!(base64_standard_decode(bad), None, "{bad}");
        }
    }

    // Derived by Python hmac apart from the phone; rejects an implementation that writes the words
    // before the name (GD8g+HmhE/h6UPgZfTaIJQ==) and one that forgets the name.
    #[test]
    fn author_tag_vectors() {
        let (k, s) = (key(), secret());
        assert_eq!(
            author_tag(&k, &s, "m-01", "Hello"),
            "B5A5n0zXsEXVLq8+nuGxBA=="
        );
        assert_eq!(
            author_tag(&k, &s, "m-02", "Hello"),
            "vYl3VVLd60pmzSfzN0lpeA=="
        );
        assert_eq!(
            author_tag(&k, &s, "Hello", "m-01"),
            "GD8g+HmhE/h6UPgZfTaIJQ=="
        );
        assert!(author_holds(
            "B5A5n0zXsEXVLq8+nuGxBA==",
            &k,
            &s,
            "m-01",
            "Hello"
        ));
        assert!(!author_holds(
            "B5A5n0zXsEXVLq8+nuGxBB==",
            &k,
            &s,
            "m-01",
            "Hello"
        ));
        assert!(!author_holds("", &k, &s, "m-01", "Hello"));
    }

    #[test]
    fn the_author_stands_eighth() {
        let (k, s) = (key(), secret());
        let head = letter_head("m-01", "Hello", "Agent", "");
        assert_eq!(head, b"m-01\0Hello\0Agent\0\0".to_vec());
        let signed = with_author(&head, &k, &s, "m-01", "Hello");
        let env = pad_envelope(&signed).expect("fits");
        assert_eq!(env.len(), ENVELOPE_SIZE);
        let f = letter_fields(&env);
        assert_eq!(f.len(), AUTHOR_FIELDS + 1);
        assert_eq!(&f[..4], &["m-01", "Hello", "Agent", ""]);
        assert_eq!(&f[4..7], &["", "", ""]);
        assert_eq!(f[7], "B5A5n0zXsEXVLq8+nuGxBA==");
        let mut card = head.clone();
        card.extend_from_slice(b"quote\0qmid\0card");
        let f = letter_fields(&with_author(&card, &k, &s, "m-01", "Hello"));
        assert_eq!(
            &f[4..],
            &["quote", "qmid", "card", "B5A5n0zXsEXVLq8+nuGxBA=="]
        );
    }

    #[test]
    fn a_body_that_cannot_carry_the_author_rides_without_it() {
        let (k, s) = (key(), secret());
        let crowded = b"a\0b\0c\0d\0e\0f\0g\0h\0".to_vec();
        assert_eq!(with_author(&crowded, &k, &s, "a", "b"), crowded);
        let mut long = letter_head("m", "", "", "");
        long.resize(ENVELOPE_SIZE - 10, b'x');
        assert_eq!(with_author(&long, &k, &s, "m", ""), long);
        assert!(pad_envelope(&vec![1u8; ENVELOPE_SIZE + 1]).is_none());
        assert_eq!(
            pad_envelope(&vec![1u8; ENVELOPE_SIZE]).map(|e| e.len()),
            Some(ENVELOPE_SIZE)
        );
    }

    #[test]
    fn fields_of_a_short_or_broken_body() {
        let f = letter_fields(b"mid");
        assert_eq!(f[0], "mid");
        assert!(f[1..].iter().all(String::is_empty));
        let f = letter_fields(&[b'm', 0, 0xff, 0xfe, 0, b'n']);
        assert_eq!(&f[..3], &["m", "", "n"]);
    }
}
