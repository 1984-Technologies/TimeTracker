use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use timetracker_backend::fuzz_support;

pub fn check_credentials(data: &[u8]) {
    if let Ok(text) = std::str::from_utf8(data) {
        let decoded = URL_SAFE_NO_PAD.decode(text);
        let expected_digest = decoded
            .as_ref()
            .ok()
            .filter(|bytes| bytes.len() == 32)
            .map(Sha256::digest);
        let actual = fuzz_support::credential_digest(text);
        assert_eq!(actual.is_ok(), expected_digest.is_some());
        if let (Ok(actual), Some(expected)) = (actual, expected_digest) {
            assert_eq!(actual.as_slice(), expected.as_slice());
        }
        let valid_config = decoded.as_ref().is_ok_and(|bytes| bytes.len() >= 32);
        assert_eq!(fuzz_support::auth(text.into()).is_ok(), valid_config);

        let decoded_hash = fuzz_support::decode_credential_hash(text);
        let canonical = text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        assert_eq!(decoded_hash.is_ok(), canonical);
        if let Ok(bytes) = decoded_hash {
            let roundtrip = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(roundtrip, text);
        }
    }

    // Exercise successful decoding/comparison and a guaranteed mismatch on
    // every input, rather than spending the run rejecting malformed encodings.
    let mut secret = [0; 32];
    let chunk = &data[..data.len().min(32)];
    secret[..chunk.len()].copy_from_slice(chunk);
    let digest: [u8; 32] = Sha256::digest(secret).into();
    let encoded = URL_SAFE_NO_PAD.encode(secret);
    assert_eq!(fuzz_support::credential_digest(&encoded).unwrap(), digest);
    assert!(fuzz_support::auth(encoded).is_ok());
    let correct_hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        fuzz_support::decode_credential_hash(&correct_hash).unwrap(),
        digest
    );
    fuzz_support::verify_device_secret(&digest, &correct_hash).unwrap();
    let mut wrong_hash = correct_hash.as_bytes().to_vec();
    wrong_hash[0] = if wrong_hash[0] == b'0' { b'1' } else { b'0' };
    assert!(
        fuzz_support::verify_device_secret(&digest, std::str::from_utf8(&wrong_hash).unwrap())
            .is_err()
    );
    if let Ok(text) = std::str::from_utf8(data) {
        assert_eq!(
            fuzz_support::verify_device_secret(&digest, text).is_ok(),
            text == correct_hash
        );
    }
}
