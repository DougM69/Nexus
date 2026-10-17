//! The internet road's account side, this computer's half: what Nexus on another computer says to
//! the Remote service before the pipe through the station's room opens (`remote/src/index.ts`, THE
//! NATIVE CLIENT), with nothing of HTTP in it, so a phone's shell can carry the same.
//!
//! - **Signed requests** ([`ComputerKey::sign_request`]): every request that acts as this computer's
//!   device for a station is signed by its device key for that station over a one-use challenge the
//!   service gave, the method, the path and the body exactly as sent (`signedBy`). The label starts
//!   what is signed, so a signature made for a request is never one for anything else the key signs.
//! - **The first sign-in** ([`Pkce`], [`authorize_url`], [`callback`]): through the system browser to
//!   the service's Native application, with PKCE (RFC 7636, S256 only) and a redirect to this
//!   computer's loopback address (RFC 8252 §7.3). The issuer, the application and the API are the
//!   service's to say (`GET /api/remote/config`), so none of them is built in here. The token it
//!   gives is kept in memory only, and only for as long as the service says.
//! - **Reopening** needs no sign-in: an approved device's own signature opens its station
//!   again (`stations/:id/client/challenge` and the signed verbs after it), so no account
//!   credential is ever kept on this computer.
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};

use crate::{hex, ComputerKey};

/// What every native request's signature covers begins with: the service's `NATIVE_REQUEST`.
pub const NATIVE_REQUEST: &str = "nexus-native-request/1";
/// The path this computer's loopback listener takes the browser's redirect on.
pub const CALLBACK_PATH: &str = "/callback";
/// What the first sign-in asks for: who the operator is, and the Remote service's API.
const SCOPE: &str = "openid profile email";

/// What a native request's signature covers: the label, the challenge, the method, the path, and
/// SHA-256 of the body exactly as sent, lowercase hex, one to a line.
pub fn signed_lines(challenge: &str, method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    [
        NATIVE_REQUEST,
        challenge,
        method,
        path,
        &hex(digest(&SHA256, body).as_ref()),
    ]
    .join("\n")
    .into_bytes()
}

impl ComputerKey {
    /// This device's signature over a native request ([`signed_lines`]): ECDSA P-256 over SHA-256,
    /// r||s, lowercase hex, as the service checks it. `None` only when the system has no randomness.
    pub fn sign_request(
        &self,
        challenge: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Option<String> {
        let signed = signed_lines(challenge, method, path, body);
        let signature = self.pair.sign(&SystemRandom::new(), &signed).ok()?;
        Some(hex(signature.as_ref()))
    }
}

/// Bytes as base64url with no padding (RFC 4648 §5), PKCE's alphabet.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

/// One sign-in's secrets (RFC 7636): the verifier this computer keeps until the code comes back, the
/// challenge the browser carries (S256), and the state the redirect must bring back unchanged. Made
/// for one sign-in, never stored, and no `Debug` or `Clone`.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

impl Pkce {
    /// A new sign-in's: 32 random bytes for the verifier and 16 for the state. `None` when the
    /// system has no randomness, when there is no safe sign-in to start.
    pub fn new() -> Option<Self> {
        let random = SystemRandom::new();
        let mut verifier = [0u8; 32];
        let mut state = [0u8; 16];
        random.fill(&mut verifier).ok()?;
        random.fill(&mut state).ok()?;
        Some(Self::of(&base64url(&verifier), base64url(&state)))
    }

    /// The challenge a verifier makes: base64url of its SHA-256.
    pub fn of(verifier: &str, state: String) -> Self {
        Self {
            verifier: verifier.to_string(),
            challenge: base64url(digest(&SHA256, verifier.as_bytes()).as_ref()),
            state,
        }
    }
}

/// A query value, percent-encoded: every byte but RFC 3986's unreserved ones.
fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A query value decoded, or `None` for one that is not percent-encoded UTF-8.
fn decoded(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let pair = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(pair, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// What the service says its sign-in is (`GET /api/remote/config`): the issuer, the Native
/// application's client id and the API its tokens are for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignIn {
    pub issuer: String,
    pub client_id: String,
    pub audience: String,
}

impl SignIn {
    /// The service's config, or `None` when it names no Native application (the service has none
    /// yet) or an issuer that is not an https address ending in `/`, as the service holds its own.
    pub fn from_config(config: &serde_json::Value) -> Option<Self> {
        let text = |name: &str| {
            config
                .get(name)
                .and_then(serde_json::Value::as_str)
                .filter(|v| !v.is_empty() && v.len() <= 512)
                .map(str::to_string)
        };
        let issuer = text("issuer")?;
        let plain = issuer.starts_with("https://")
            && issuer.ends_with('/')
            && !issuer.contains(['?', '#', ' ']);
        plain.then_some(Self {
            issuer,
            client_id: text("nativeClientId")?,
            audience: text("audience")?,
        })
    }
}

/// The address the system browser opens for the first sign-in: the issuer's authorize endpoint,
/// with the code returned to `redirect` (this computer's loopback listener) under `pkce`.
pub fn authorize_url(sign_in: &SignIn, redirect: &str, pkce: &Pkce) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", &sign_in.client_id),
        ("redirect_uri", redirect),
        ("scope", SCOPE),
        ("audience", &sign_in.audience),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("state", &pkce.state),
    ]
    .iter()
    .map(|(name, value)| format!("{name}={}", encoded(value)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{}authorize?{query}", sign_in.issuer)
}

/// The form this computer posts to the issuer's token endpoint for the code the redirect brought.
pub fn token_form(sign_in: &SignIn, redirect: &str, pkce: &Pkce, code: &str) -> String {
    [
        ("grant_type", "authorization_code"),
        ("client_id", sign_in.client_id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
        ("code", code),
        ("redirect_uri", redirect),
    ]
    .iter()
    .map(|(name, value)| format!("{name}={}", encoded(value)))
    .collect::<Vec<_>>()
    .join("&")
}

/// The redirect's request target as the loopback listener read it (`/callback?code=…&state=…`): the
/// code, when it came back with this sign-in's state. `signInRefused` when the issuer said no (the
/// operator cancelled, or the application refused them), `signInFailed` for anything else: another
/// path, another state, no code.
pub fn callback(target: &str, state: &str) -> Result<String, &'static str> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return Err("signInFailed");
    }
    let mut code = None;
    let mut came_back = None;
    let mut refused = false;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        match name {
            "code" => code = decoded(value),
            "state" => came_back = decoded(value),
            "error" => refused = true,
            _ => {}
        }
    }
    // The state first: a redirect this sign-in did not start says nothing, refusal or code.
    if came_back.as_deref() != Some(state) {
        return Err("signInFailed");
    }
    if refused {
        return Err("signInRefused");
    }
    code.filter(|code| !code.is_empty() && code.len() <= 2048)
        .ok_or("signInFailed")
}

/// The token the issuer's answer carries, and how long it lasts (seconds, at most an hour, as the
/// service holds a token to), or `None` for an answer that is not one.
pub fn token(answer: &serde_json::Value) -> Option<(String, u64)> {
    let token = answer.get("access_token")?.as_str()?;
    let bearer = answer
        .get("token_type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"));
    let shaped = bearer
        && !token.is_empty()
        && token.len() <= 8192
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.~+/=".contains(&b));
    let lasts = answer.get("expires_in")?.as_u64()?.min(3600);
    (shaped && lasts > 0).then(|| (token.to_string(), lasts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};

    /// ★ RFC 7636's own example (Appendix B): the verifier makes the challenge the RFC gives, so the
    /// issuer computes the same one. CONTROL: one character of the verifier changed makes another.
    #[test]
    fn the_challenge_is_rfc_7636s_s256() {
        let pkce = Pkce::of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "s".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let other = Pkce::of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl", "s".into());
        assert_ne!(other.challenge, pkce.challenge);
        let fresh = Pkce::new().unwrap();
        assert_eq!(fresh.verifier.len(), 43);
        assert_eq!(fresh.state.len(), 22);
        assert_ne!(fresh.verifier, Pkce::new().unwrap().verifier);
    }

    /// RFC 4648's test vectors, base64url with no padding.
    #[test]
    fn base64url_is_rfc_4648s() {
        for (bytes, text) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64url(bytes.as_bytes()), text);
        }
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    /// ★ A signed request is the service's five lines, signed by this device's key: its public key
    /// verifies it, over exactly those lines. CONTROL: the same signature over a body one byte
    /// different, another path, or another challenge does not verify.
    #[test]
    fn a_request_is_signed_over_the_services_five_lines() {
        let (key, _) = ComputerKey::generate().unwrap();
        let challenge = "ab".repeat(32);
        let body = br#"{"name":"Desk","kind":"computer","platform":"windows"}"#;
        let path = "/api/remote/stations/60000000-0000-4000-8000-000000000001/client/device";
        let signature = key.sign_request(&challenge, "POST", path, body).unwrap();
        assert_eq!(signature.len(), 128);
        let lines = String::from_utf8(signed_lines(&challenge, "POST", path, body)).unwrap();
        let sha = hex(digest(&SHA256, body).as_ref());
        assert_eq!(
            lines,
            format!("nexus-native-request/1\n{challenge}\nPOST\n{path}\n{sha}")
        );
        let spki = tempo_stream::protocol::hex_bytes(key.public_key()).unwrap();
        let point = &spki[spki.len() - 65..];
        let verifier = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point);
        let sig = tempo_stream::protocol::hex_bytes(&signature).unwrap();
        assert!(verifier.verify(lines.as_bytes(), &sig).is_ok());
        for (c, p, b) in [
            (challenge.as_str(), path, &b"{}"[..]),
            (challenge.as_str(), "/api/remote/client/session", &body[..]),
            (&"cd".repeat(32), path, &body[..]),
        ] {
            assert!(
                verifier
                    .verify(&signed_lines(c, "POST", p, b), &sig)
                    .is_err(),
                "a signature passed for another request"
            );
        }
    }

    /// ★ The redirect's code is taken only with this sign-in's state; a refusal is told as one,
    /// and only under the same state. CONTROL: the same redirect with another state is refused.
    #[test]
    fn the_code_comes_back_only_with_this_sign_ins_state() {
        assert_eq!(
            callback("/callback?code=a%2Fb&state=xyz", "xyz"),
            Ok("a/b".into())
        );
        assert_eq!(
            callback("/callback?state=xyz&code=c", "xyz"),
            Ok("c".into())
        );
        assert_eq!(
            callback("/callback?code=c&state=xyy", "xyz"),
            Err("signInFailed")
        );
        assert_eq!(callback("/callback?code=c", "xyz"), Err("signInFailed"));
        assert_eq!(
            callback("/callback?error=access_denied&state=xyz", "xyz"),
            Err("signInRefused")
        );
        assert_eq!(
            callback("/callback?error=access_denied&state=other", "xyz"),
            Err("signInFailed")
        );
        assert_eq!(
            callback("/other?code=c&state=xyz", "xyz"),
            Err("signInFailed")
        );
        assert_eq!(
            callback("/callback?code=&state=xyz", "xyz"),
            Err("signInFailed")
        );
    }

    /// The authorize address carries the service's Native application, its API, the loopback
    /// redirect and S256, every value encoded; the token form carries the verifier.
    #[test]
    fn the_authorize_address_and_the_token_form() {
        let sign_in = SignIn::from_config(&serde_json::json!({
            "issuer": "https://login.example/", "nativeClientId": "native-id",
            "audience": "https://api.example/remote", "clientId": "spa-id"
        }))
        .unwrap();
        let pkce = Pkce::of("verifier-verifier-verifier-verifier-verifi", "st".into());
        let url = authorize_url(&sign_in, "http://127.0.0.1:5123/callback", &pkce);
        assert!(url.starts_with(
            "https://login.example/authorize?response_type=code&client_id=native-id&"
        ));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2Fcallback"));
        assert!(url.contains("audience=https%3A%2F%2Fapi.example%2Fremote"));
        assert!(url.contains("scope=openid%20profile%20email"));
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        assert!(url.contains("code_challenge_method=S256&state=st"));
        assert!(!url.contains("spa-id"));
        let form = token_form(
            &sign_in,
            "http://127.0.0.1:5123/callback",
            &pkce,
            "the/code",
        );
        assert_eq!(
            form,
            "grant_type=authorization_code&client_id=native-id&code_verifier=verifier-verifier-verifier-verifier-verifi&code=the%2Fcode&redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2Fcallback"
        );
    }

    /// The service's config is taken only with a Native application and a plain https issuer.
    #[test]
    fn a_service_with_no_native_application_has_no_sign_in() {
        let config = |issuer: &str, native: Option<&str>| {
            SignIn::from_config(&serde_json::json!({
                "issuer": issuer, "nativeClientId": native, "audience": "https://api.example/"
            }))
        };
        assert!(config("https://login.example/", Some("n")).is_some());
        assert!(config("https://login.example/", None).is_none());
        assert!(config("https://login.example/", Some("")).is_none());
        assert!(config("http://login.example/", Some("n")).is_none());
        assert!(config("https://login.example", Some("n")).is_none());
        assert!(config("https://login.example/?x=1/", Some("n")).is_none());
    }

    /// The issuer's answer: a bearer token, its life held to an hour.
    #[test]
    fn the_token_lasts_at_most_an_hour() {
        let answer = |token: &str, kind: &str, lasts: u64| {
            token_of(
                serde_json::json!({"access_token": token, "token_type": kind, "expires_in": lasts}),
            )
        };
        fn token_of(v: serde_json::Value) -> Option<(String, u64)> {
            token(&v)
        }
        assert_eq!(
            answer("a.b.c", "Bearer", 86400),
            Some(("a.b.c".into(), 3600))
        );
        assert_eq!(answer("a.b.c", "bearer", 60), Some(("a.b.c".into(), 60)));
        assert_eq!(answer("a.b.c", "mac", 60), None);
        assert_eq!(answer("", "Bearer", 60), None);
        assert_eq!(answer("a b", "Bearer", 60), None);
        assert_eq!(answer("a.b.c", "Bearer", 0), None);
    }
}
