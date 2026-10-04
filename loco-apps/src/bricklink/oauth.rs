//! OAuth 1.0a request signing, HMAC-SHA1, as the BrickLink store API takes it.
//!
//! BrickLink issues all four credentials up front (consumer key and secret,
//! token value and secret), so there is no token dance: each request is
//! signed and sent with an `Authorization: OAuth …` header.
//!
//! The signature base string and key follow RFC 5849 §3.4. Query parameters
//! are part of the base string. There is no request body on a GET.

use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::Url;
use sha1::Sha1;

/// The four BrickLink credentials.
pub struct Credentials<'a> {
    pub consumer_key: &'a str,
    pub consumer_secret: &'a str,
    pub token_value: &'a str,
    pub token_secret: &'a str,
}

/// `Authorization` header value for `method url`. `nonce` and `timestamp`
/// are arguments so a test can pin them; [`authorization`] draws fresh ones.
pub fn authorization_with(
    method: &str,
    url: &Url,
    creds: &Credentials<'_>,
    nonce: &str,
    timestamp: &str,
) -> String {
    let signature = signature(method, url, creds, nonce, timestamp);
    let params = [
        ("oauth_consumer_key", creds.consumer_key),
        ("oauth_token", creds.token_value),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_signature", signature.as_str()),
        ("oauth_timestamp", timestamp),
        ("oauth_nonce", nonce),
        ("oauth_version", "1.0"),
    ];
    let mut header = String::from("OAuth realm=\"\"");
    for (key, value) in params {
        header.push_str(&format!(", {key}=\"{}\"", encode(value)));
    }
    header
}

/// [`authorization_with`] with a random nonce and the current Unix time.
pub fn authorization(method: &str, url: &Url, creds: &Credentials<'_>) -> String {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
        .to_string();
    authorization_with(method, url, creds, &nonce, &timestamp)
}

/// Base64 HMAC-SHA1 of the signature base string (RFC 5849 §3.4.2).
pub fn signature(
    method: &str,
    url: &Url,
    creds: &Credentials<'_>,
    nonce: &str,
    timestamp: &str,
) -> String {
    let base = base_string(method, url, creds, nonce, timestamp);
    let key = format!(
        "{}&{}",
        encode(creds.consumer_secret),
        encode(creds.token_secret)
    );
    let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes()).expect("HMAC takes any key length");
    mac.update(base.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// `METHOD&base-uri&normalized-params`, each part percent-encoded (§3.4.1).
pub fn base_string(
    method: &str,
    url: &Url,
    creds: &Credentials<'_>,
    nonce: &str,
    timestamp: &str,
) -> String {
    let mut params: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (encode(&key), encode(&value)))
        .collect();
    for (key, value) in [
        ("oauth_consumer_key", creds.consumer_key),
        ("oauth_token", creds.token_value),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_timestamp", timestamp),
        ("oauth_nonce", nonce),
        ("oauth_version", "1.0"),
    ] {
        params.push((encode(key), encode(value)));
    }
    params.sort();
    let normalized = params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!(
        "{}&{}&{}",
        method.to_ascii_uppercase(),
        encode(&base_uri(url)),
        encode(&normalized)
    )
}

/// Scheme and host lowercased, a default port dropped, no query or fragment
/// (§3.4.1.2). [`Url`] already lowercases and drops a default port.
fn base_uri(url: &Url) -> String {
    let mut out = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    out.push_str(url.path());
    out
}

/// RFC 3986 percent-encoding: every byte but `A-Z a-z 0-9 - . _ ~`, with
/// uppercase hex (§3.6).
pub fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OAuth Core 1.0, Appendix A (Protocol Example), A.5.1 and A.5.2: the
    /// photos.example.net protected-resource request, https://oauth.net/core/1.0/.
    /// OAuth 1.0a (Core 1.0 Revision A) did not change signing, so the vector
    /// still holds. Independently re-derived with Python's `hmac` module.
    fn photos() -> (Url, Credentials<'static>) {
        (
            Url::parse("http://photos.example.net/photos?file=vacation.jpg&size=original").unwrap(),
            Credentials {
                consumer_key: "dpf43f3p2l4k3l03",
                consumer_secret: "kd94hf93k423kf44",
                token_value: "nnch734d00sl2jdk",
                token_secret: "pfkkdhi9sl3r4s00",
            },
        )
    }

    #[test]
    fn base_string_matches_oauth_core_appendix_a_5_1() {
        let (url, creds) = photos();
        assert_eq!(
            base_string("GET", &url, &creds, "kllo9940pd9333jh", "1191242096"),
            "GET&http%3A%2F%2Fphotos.example.net%2Fphotos&file%3Dvacation.jpg\
             %26oauth_consumer_key%3Ddpf43f3p2l4k3l03%26oauth_nonce%3Dkllo9940pd9333jh\
             %26oauth_signature_method%3DHMAC-SHA1%26oauth_timestamp%3D1191242096\
             %26oauth_token%3Dnnch734d00sl2jdk%26oauth_version%3D1.0%26size%3Doriginal"
        );
    }

    #[test]
    fn signature_matches_oauth_core_appendix_a_5_2() {
        let (url, creds) = photos();
        assert_eq!(
            signature("GET", &url, &creds, "kllo9940pd9333jh", "1191242096"),
            "tR3+Ty81lMeYAr/Fid0kMTYa/WM="
        );
    }

    #[test]
    fn header_carries_the_encoded_signature() {
        let (url, creds) = photos();
        let header = authorization_with("GET", &url, &creds, "kllo9940pd9333jh", "1191242096");
        assert!(header.starts_with("OAuth realm=\"\""));
        assert!(header.contains("oauth_signature=\"tR3%2BTy81lMeYAr%2FFid0kMTYa%2FWM%3D\""));
        assert!(header.contains("oauth_consumer_key=\"dpf43f3p2l4k3l03\""));
        assert!(header.contains("oauth_token=\"nnch734d00sl2jdk\""));
        assert!(!header.contains("kd94hf93k423kf44"));
        assert!(!header.contains("pfkkdhi9sl3r4s00"));
    }

    #[test]
    fn a_default_port_is_not_signed_and_another_port_is() {
        let creds = photos().1;
        let https = Url::parse("https://API.bricklink.com:443/api/store/v1/orders").unwrap();
        assert!(base_string("GET", &https, &creds, "n", "1")
            .starts_with("GET&https%3A%2F%2Fapi.bricklink.com%2Fapi%2Fstore%2Fv1%2Forders&"));
        let local = Url::parse("http://127.0.0.1:8080/orders").unwrap();
        assert!(base_string("GET", &local, &creds, "n", "1")
            .starts_with("GET&http%3A%2F%2F127.0.0.1%3A8080%2Forders&"));
    }

    /// RFC 5849 §3.6 rules. The strings are the examples in Twitter's
    /// "Percent encoding parameters" guide for OAuth 1.0a.
    #[test]
    fn encode_is_rfc_3986() {
        assert_eq!(encode("Ladies + Gentlemen"), "Ladies%20%2B%20Gentlemen");
        assert_eq!(encode("An encoded string!"), "An%20encoded%20string%21");
        assert_eq!(encode("Dogs, Cats & Mice"), "Dogs%2C%20Cats%20%26%20Mice");
        assert_eq!(encode("☃"), "%E2%98%83");
        assert_eq!(encode("a-b.c_d~e"), "a-b.c_d~e");
    }
}
