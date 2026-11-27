use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use timetracker_backend::fuzz_support;
use tonic::metadata::{MetadataMap, MetadataValue};

fn reference(value: &str) -> Option<&str> {
    let parts: Vec<_> = value.split_ascii_whitespace().collect();
    if parts.len() != 2
        || !parts[0].eq_ignore_ascii_case("Bearer")
        || format!("{} {}", parts[0], parts[1]) != value
    {
        return None;
    }
    Some(parts[1])
}

fn check_value(bytes: &[u8]) {
    if let Ok(value) = std::str::from_utf8(bytes) {
        assert_eq!(fuzz_support::bearer(value).ok(), reference(value));
    }
    if let Ok(value) = HeaderValue::from_bytes(bytes) {
        let expected = value.to_str().ok().and_then(reference).map(str::to_owned);
        let mut headers = HeaderMap::new();
        headers.append(AUTHORIZATION, value.clone());
        assert_eq!(
            fuzz_support::http_bearer(&headers).ok(),
            expected.as_deref()
        );
        headers.append(AUTHORIZATION, value);
        assert!(
            fuzz_support::http_bearer(&headers).is_err(),
            "duplicate HTTP authorization accepted"
        );
    }
    if let Ok(value) = MetadataValue::try_from(bytes) {
        let expected = value.to_str().ok().and_then(reference).map(str::to_owned);
        let mut metadata = MetadataMap::new();
        metadata.append("authorization", value.clone());
        assert_eq!(
            fuzz_support::grpc_bearer(&metadata).ok(),
            expected.as_deref()
        );
        metadata.append("authorization", value);
        assert!(
            fuzz_support::grpc_bearer(&metadata).is_err(),
            "duplicate gRPC authorization accepted"
        );
    }
}

pub fn check_authorization(data: &[u8]) {
    assert!(fuzz_support::http_bearer(&HeaderMap::new()).is_err());
    assert!(fuzz_support::grpc_bearer(&MetadataMap::new()).is_err());
    check_value(data);
    let token = format!("t{}", URL_SAFE_NO_PAD.encode(data));
    for value in [
        format!("Bearer {token}"),
        format!("bEaReR {token}"),
        format!("Bearer  {token}"),
        format!("Bearer {token} extra"),
        format!("Bearer\t{token}"),
        format!(" Bearer {token}"),
    ] {
        check_value(value.as_bytes());
    }
}
