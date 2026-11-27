use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Sha256, Sha384};
use std::sync::OnceLock;
use timetracker_backend::{auth::AuthService, fuzz_support};

// Public, fixed test material. No production configuration is read by fuzzing.
const TEST_KEY: [u8; 32] = [0xa5; 32];
const OTHER_KEY: [u8; 32] = [0x5a; 32];
const FIELDS: [&str; 6] = ["sub", "kind", "iss", "aud", "iat", "exp"];

fn validator() -> &'static AuthService {
    static VALIDATOR: OnceLock<AuthService> = OnceLock::new();
    VALIDATOR.get_or_init(|| fuzz_support::auth(URL_SAFE_NO_PAD.encode(TEST_KEY)).unwrap())
}

// Deliberately independent of JwtClaims and jsonwebtoken's Validation. Typed
// deserialization also rejects duplicate required fields and incorrect types.
#[derive(Deserialize)]
struct ReferenceClaims {
    sub: String,
    kind: String,
    iss: String,
    aud: String,
    iat: u64,
    exp: u64,
    #[serde(default)]
    nbf: Option<Value>,
}

fn reference_payload(payload: &[u8], now: u64) -> Option<bool> {
    let Ok(claims) = serde_json::from_slice::<ReferenceClaims>(payload) else {
        return Some(false);
    };
    if uuid::Uuid::parse_str(&claims.sub).is_err()
        || !matches!(claims.kind.as_str(), "admin" | "device")
        || claims.iss != "timetracker"
        || claims.aud != "timetracker-api"
        || claims.iat > now
        || claims.exp <= now
        || claims.exp <= claims.iat
    {
        return Some(false);
    }
    if let Some(nbf) = claims.nbf {
        // nbf is an extra claim outside our model. For noninteger nbf values,
        // exercise the decoder without asserting jsonwebtoken's coercion policy.
        return nbf.as_u64().map(|nbf| nbf <= now);
    }
    Some(true)
}

fn sign(payload: &[u8], algorithm: &str, key: &[u8]) -> String {
    let header = URL_SAFE_NO_PAD.encode(json!({"alg":algorithm,"typ":"JWT"}).to_string());
    let message = format!("{header}.{}", URL_SAFE_NO_PAD.encode(payload));
    let signature = match algorithm {
        "HS256" => {
            let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
            mac.update(message.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }
        "HS384" => {
            let mut mac = Hmac::<Sha384>::new_from_slice(key).unwrap();
            mac.update(message.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }
        "none" => Vec::new(),
        _ => unreachable!(),
    };
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn reference_accepts_in_window(payload: &[u8], before: u64, after: u64) -> bool {
    let Ok(claims) = serde_json::from_slice::<ReferenceClaims>(payload) else {
        return false;
    };
    let mut earliest = before.min(after).max(claims.iat);
    if let Some(nbf) = claims.nbf.as_ref().and_then(Value::as_u64) {
        earliest = earliest.max(nbf);
    }
    earliest <= before.max(after) && reference_payload(payload, earliest) != Some(false)
}

pub fn check_raw_jwt(data: &[u8]) {
    let Ok(token) = std::str::from_utf8(data) else {
        return;
    };
    let before = jsonwebtoken::get_current_timestamp();
    let result = validator().validate_token(token);
    let after = jsonwebtoken::get_current_timestamp();
    if result.is_ok() {
        let parts: Vec<_> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "HS256");
        let mut mac = Hmac::<Sha256>::new_from_slice(&TEST_KEY).unwrap();
        mac.update(format!("{}.{}", parts[0], parts[1]).as_bytes());
        mac.verify_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap())
            .expect("accepted invalid signature");
        let payload = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        assert!(
            reference_accepts_in_window(&payload, before, after),
            "accepted invalid claims"
        );
    }
}

fn check_payload(payload: &[u8], algorithm: &str, key: &[u8], corrupt: bool) {
    let mut token = sign(payload, algorithm, key);
    if corrupt {
        let signature_start = token.rfind('.').unwrap() + 1;
        let mut bytes = token.into_bytes();
        bytes[signature_start] = if bytes[signature_start] == b'A' {
            b'B'
        } else {
            b'A'
        };
        token = String::from_utf8(bytes).unwrap();
    }
    let before = jsonwebtoken::get_current_timestamp();
    let accepted = validator().validate_token(&token).is_ok();
    let after = jsonwebtoken::get_current_timestamp();
    if algorithm != "HS256" || key != TEST_KEY || corrupt {
        assert!(
            !accepted,
            "accepted wrong algorithm, signing key, or token format"
        );
    } else {
        let expected = reference_payload(payload, before);
        // Expiry can cross a second boundary during the call. Assert an exact
        // verdict only when the reference result is stable across the call.
        if let Some(expected) = expected
            && Some(expected) == reference_payload(payload, after)
            && (expected || !reference_accepts_in_window(payload, before, after))
        {
            assert_eq!(
                accepted, expected,
                "JWT verdict disagrees with the independent claims oracle"
            );
        }
    }
    check_raw_jwt(token.as_bytes());
}

fn time_value(data: &[u8], now: u64) -> u64 {
    let mut bytes = [0; 8];
    let chunk = &data[..data.len().min(8)];
    bytes[..chunk.len()].copy_from_slice(chunk);
    match data.first().copied().unwrap_or(0) % 9 {
        0 => 0,
        1 => 1,
        2 => u64::MAX,
        3 => now.saturating_sub(1),
        4 => now,
        5 => now.saturating_add(1),
        6 => now.saturating_add(900),
        7 => u64::MAX - 1,
        _ => u64::from_le_bytes(bytes),
    }
}

pub fn check_signed_claims(data: &[u8]) {
    // Sign arbitrary payload bytes, including invalid JSON, duplicate keys and
    // wrong claim types, so mutations always get past signature verification.
    check_payload(data, "HS256", &TEST_KEY, false);

    // Also generate a valid base on every iteration. Random bytes therefore
    // reach the claims checks even before libFuzzer discovers JSON structure.
    let now = jsonwebtoken::get_current_timestamp();
    let mut claims = json!({"sub":"00000000-0000-0000-0000-000000000001", "kind":"admin",
        "iss":"timetracker", "aud":"timetracker-api", "iat":now.saturating_sub(1), "exp":now+900});
    let mode = data.first().copied().unwrap_or(0) % 16;
    let rest = data.get(2..).unwrap_or_default();
    match mode {
        0 => {}
        1 => claims["kind"] = json!("device"),
        2 => {
            claims
                .as_object_mut()
                .unwrap()
                .remove(FIELDS[data.get(1).copied().unwrap_or(0) as usize % FIELDS.len()]);
        }
        3..=8 => {
            claims[FIELDS[(mode - 3) as usize]] = serde_json::from_slice(rest)
                .unwrap_or_else(|_| json!(String::from_utf8_lossy(rest)));
        }
        9 => claims["iat"] = json!(time_value(rest, now)),
        10 => claims["exp"] = json!(time_value(rest, now)),
        11 => claims["nbf"] = json!(time_value(rest, now)),
        _ => {}
    }
    let algorithm = match mode {
        13 => "HS384",
        14 => "none",
        _ => "HS256",
    };
    let key = if mode == 12 { &OTHER_KEY } else { &TEST_KEY };
    check_payload(
        &serde_json::to_vec(&claims).unwrap(),
        algorithm,
        key,
        mode == 15,
    );
}
