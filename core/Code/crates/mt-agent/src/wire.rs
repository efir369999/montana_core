//! The keys and tags of the node wire: what a phone derives from a pipe's secret to seal a letter,
//! to name its conversation at a door and to stand for its sender there.

use core::ops::RangeInclusive;

use mt_messenger_e2e::media::{open_blob, seal_blob};
use mt_mnemonic::sha256_raw;

pub const PIPE_KEY_DOMAIN: &[u8] = b"mt-pipe-key";
pub const WAKE_CONV_DOMAIN: &[u8] = b"mt-wake-conv";
pub const WAKE_SUB_DOMAIN: &[u8] = b"mt-wake-sub";
/// A node keeps only the minute it received a letter, never the minute it was sealed, and a seal is
/// never made after the node took it: the receiver walks back a day from the node's minute.
pub const OPEN_BACK_MINUTES: u64 = 1440;
pub const NONCE_LEN: usize = 12;

fn windowed(domain: &[u8], secret: &[u8], window: u64) -> [u8; 32] {
    let mut m = Vec::with_capacity(domain.len() + 1 + secret.len() + 8);
    m.extend_from_slice(domain);
    m.push(0);
    m.extend_from_slice(secret);
    m.extend_from_slice(&window.to_le_bytes());
    sha256_raw(&m)
}

pub fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    s
}

/// SHA-256("mt-pipe-key" ‖ 0x00 ‖ secret ‖ W_8B_LE), W the minute of the seal.
pub fn body_key(secret: &[u8], minute: u64) -> [u8; 32] {
    windowed(PIPE_KEY_DOMAIN, secret, minute)
}

/// The conversation's tag of a day at a door: both sides derive it from the pipe's secret.
pub fn conv_tag(secret: &[u8], day: u64) -> String {
    hex_lower(&windowed(WAKE_CONV_DOMAIN, secret, day))
}

/// The tag that stands for the sender at a door in place of its Montana address.
pub fn sub_tag(conv: &str, reference: &str) -> String {
    let mut m = Vec::with_capacity(WAKE_SUB_DOMAIN.len() + conv.len() + reference.len() + 2);
    m.extend_from_slice(WAKE_SUB_DOMAIN);
    m.push(0);
    m.extend_from_slice(conv.as_bytes());
    m.push(0);
    m.extend_from_slice(reference.as_bytes());
    hex_lower(&sha256_raw(&m))
}

pub fn minute_window(unix_seconds: u64) -> u64 {
    unix_seconds / 60
}

pub fn day_window(unix_seconds: u64) -> u64 {
    unix_seconds / 86_400
}

/// A letter lies in a box a day from its last knock, under the day its knock was sealed in: the read
/// asks that day and the one before, a day more for a sender's clock behind and one ahead.
pub fn box_days(day: u64) -> RangeInclusive<u64> {
    day.saturating_sub(2)..=day.saturating_add(1)
}

/// The second a letter was sealed: the node's second less the walk back its opening took.
pub fn sealed_second(node_unix_seconds: u64, back_minutes: i64) -> u64 {
    let s = (i128::from(node_unix_seconds) - i128::from(back_minutes) * 60).max(0);
    u64::try_from(s).unwrap_or(u64::MAX)
}

/// The nonce is the caller's: a knock draws a fresh one for every door it knocks on.
pub fn seal_letter(
    secret: &[u8],
    minute: u64,
    nonce: &[u8; NONCE_LEN],
    envelope: &[u8],
) -> Vec<u8> {
    seal_blob(&body_key(secret, minute), nonce, envelope)
}

/// The plaintext of a boxed letter and how many minutes before the node's minute it was sealed
/// (negative: the sender's clock stood ahead). The common case first: sealed and taken in one breath.
pub fn open_boxed(sealed: &[u8], secret: &[u8], node_unix_seconds: u64) -> Option<(Vec<u8>, i64)> {
    let w0 = minute_window(node_unix_seconds);
    let back = |w: u64| w0.wrapping_sub(w) as i64;
    for w in [w0, w0.wrapping_sub(1), w0.wrapping_add(1)] {
        if let Some(p) = open_blob(&body_key(secret, w), sealed) {
            return Some((p, back(w)));
        }
    }
    let mut w = w0.wrapping_sub(2);
    for _ in 2..=OPEN_BACK_MINUTES {
        if let Some(p) = open_blob(&body_key(secret, w), sealed) {
            return Some((p, back(w)));
        }
        w = w.wrapping_sub(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distinct() -> Vec<u8> {
        (0u8..32).collect()
    }

    // The phone's frozen vectors (MTPipe.swift agreesWithCanon), derived again by Python hashlib apart
    // from both: thirty-two distinct bytes and a window with bits at both ends reject an implementation
    // without the separator, one laying the window most-significant first and one reversing the secret.
    #[test]
    fn body_key_is_the_phones() {
        let s = distinct();
        assert_eq!(
            hex::encode(body_key(&s, 0x0102_0304_0506_0708)),
            "e55fbc1e0f95c4fe012b476c4dd1a51c9455e24641248f4f1557ed57f8c14f35"
        );
        assert_eq!(
            hex::encode(body_key(&s, 1000)),
            "e25542e65a84d1a5b5c72a13172ecb62319c18f21a59978a53e7dac2c22a966a"
        );
    }

    // Rejects an implementation that drops the separator (046fa1bf...) or takes the key domain.
    #[test]
    fn conv_tag_vectors() {
        let s = distinct();
        assert_eq!(
            conv_tag(&s, 0x0102_0304_0506_0708),
            "cd50b60cfa55cbbdc8fa860cb233900b5823695725d1572f070aa5ebc44e54ea"
        );
        let day = conv_tag(&s, 20_382);
        assert_eq!(
            day,
            "69cb9441fdf7f675c7a8762099ee4c52bbbadfd080f818f3e828b726e5c80551"
        );
        assert_ne!(
            day,
            "046fa1bf6e78e4908747ad80c4e283250694649d86acddd91350413a1682b90f"
        );
        assert_ne!(day, hex::encode(body_key(&s, 20_382)));
    }

    // Rejects an implementation that writes the reference before the conversation (6e2e60e6...).
    #[test]
    fn sub_tag_vectors() {
        let conv = conv_tag(&distinct(), 20_382);
        let r = "f4f72dafb742470e49defbd3c2326c27";
        assert_eq!(
            sub_tag(&conv, r),
            "ec0673e3c59253723e817938a1b32f90c2a26c759c8c0444746f2331b3a1fd54"
        );
        assert_eq!(
            sub_tag(r, &conv),
            "6e2e60e6c4bf932ee20056f72e3253e256f0e301ba3c4576e5da2f1af2733643"
        );
    }

    #[test]
    fn windows_and_box_days() {
        assert_eq!(minute_window(1_761_000_059), 29_350_000);
        assert_eq!(minute_window(1_761_000_060), 29_350_001);
        assert_eq!(day_window(1_760_918_399), 20_380);
        assert_eq!(day_window(1_760_918_400), 20_381);
        assert_eq!(box_days(20_381), 20_379..=20_382);
        assert_eq!(box_days(1), 0..=2);
    }

    #[test]
    fn a_boxed_letter_opens_at_the_minute_it_was_sealed() {
        let s = distinct();
        let nonce = [7u8; NONCE_LEN];
        let env = b"m-01\0Hello\0Agent\0\0".to_vec();
        let sealed_at = 29_350_000u64;
        let sealed = seal_letter(&s, sealed_at, &nonce, &env);
        assert_eq!(&sealed[..NONCE_LEN], &nonce);
        for (taken, back) in [
            (sealed_at, 0i64),
            (sealed_at + 3, 3),
            (sealed_at - 1, -1),
            (sealed_at + 1440, 1440),
        ] {
            let (plain, b) = open_boxed(&sealed, &s, taken * 60 + 17).expect("opens");
            assert_eq!(plain, env);
            assert_eq!(b, back);
        }
        assert!(open_boxed(&sealed, &s, (sealed_at + 1441) * 60).is_none());
        assert!(open_boxed(&sealed, &s, (sealed_at - 2) * 60).is_none());
        let mut other = s.clone();
        other.reverse();
        assert!(open_boxed(&sealed, &other, sealed_at * 60).is_none());
    }
}
