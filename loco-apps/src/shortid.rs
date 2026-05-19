//! Base58 codec for UUIDs at the HTTP boundary. Lake stores canonical
//! hyphenated UUIDs; the wire format uses these 22-char short-ids. Inputs that
//! don't look like UUIDs (encode) or aren't pure-base58 (decode) pass through
//! unchanged, so applying either function twice is a no-op.
//!
//! Mirrors `loco-studio/src/shortId.js` — kept here so the studio (and any
//! other client) doesn't need its own codec.

use uuid::Uuid;

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const SHORT_LEN: usize = 22;

pub fn encode(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let Ok(uuid) = Uuid::parse_str(s) else {
        return s.to_string();
    };
    let mut n = uuid.as_u128();
    let mut buf = [ALPHABET[0]; SHORT_LEN];
    let mut i = SHORT_LEN;
    while n > 0 {
        i -= 1;
        buf[i] = ALPHABET[(n % 58) as usize];
        n /= 58;
    }
    String::from_utf8(buf.to_vec()).expect("alphabet is ascii")
}

pub fn decode(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut n: u128 = 0;
    for c in s.bytes() {
        let Some(v) = ALPHABET.iter().position(|&a| a == c) else {
            return s.to_string();
        };
        let Some(scaled) = n.checked_mul(58) else {
            return s.to_string();
        };
        let Some(sum) = scaled.checked_add(v as u128) else {
            return s.to_string();
        };
        n = sum;
    }
    Uuid::from_u128(n).hyphenated().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_random_uuid() {
        let uuid = Uuid::new_v4().to_string();
        let short = encode(&uuid);
        assert_eq!(short.len(), SHORT_LEN);
        assert_eq!(decode(&short), uuid);
    }

    #[test]
    fn encode_pads_small_values() {
        // The nil UUID encodes to all-'1' (alphabet[0]).
        let nil = "00000000-0000-0000-0000-000000000000";
        assert_eq!(encode(nil), "1".repeat(SHORT_LEN));
    }

    #[test]
    fn encode_max_uuid() {
        // u128::MAX must fit in 22 chars without overflow.
        let max = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        let short = encode(max);
        assert_eq!(short.len(), SHORT_LEN);
        assert_eq!(decode(&short), max);
    }

    #[test]
    fn encode_passes_through_non_uuid() {
        assert_eq!(encode("not-a-uuid"), "not-a-uuid");
        assert_eq!(encode(""), "");
    }

    #[test]
    fn decode_passes_through_hyphenated_uuid() {
        // '-' is not in the alphabet, so decode is a no-op on canonical UUIDs.
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(decode(uuid), uuid);
    }

    #[test]
    fn encode_is_idempotent() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let short = encode(uuid);
        assert_eq!(encode(&short), short);
    }

    #[test]
    fn decode_rejects_overflow() {
        // 22 chars of 'z' (index 57) overflows u128 — should pass through.
        let too_big = "z".repeat(SHORT_LEN);
        assert_eq!(decode(&too_big), too_big);
    }
}
