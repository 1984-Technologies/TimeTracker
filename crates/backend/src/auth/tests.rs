use super::*;
use crate::test_support::*;
use serde_json::json;

#[test]
fn jwt_configuration_rejects_plaintext_padding_and_short_keys() {
    for key in [
        String::new(),
        "a weak plaintext key".into(),
        URL_SAFE_NO_PAD.encode([1; 31]),
        format!("{}=", URL_SAFE_NO_PAD.encode([1; 32])),
    ] {
        let store = Arc::new(Store::default());
        assert!(matches!(
            AuthService::new(store.clone(), store, key),
            Err(BackendError::Validation(_))
        ));
    }
    let store = Arc::new(Store::default());
    assert!(AuthService::new(store.clone(), store, URL_SAFE_NO_PAD.encode([1; 64])).is_ok());
}

#[tokio::test]
async fn both_logins_issue_hs256_tokens_for_exactly_fifteen_minutes() {
    let (_, auth) = fixture().await;
    let admin = auth
        .login_admin(AdminLoginRequest {
            username: "admin".into(),
            password: "correct-password".into(),
        })
        .await
        .unwrap();
    let device = auth
        .login_device(DeviceCredentials {
            device_id: DEVICE_ID,
            secret: URL_SAFE_NO_PAD.encode([3; 32]),
        })
        .await
        .unwrap();
    for (response, subject, kind) in [
        (admin, ADMIN_ID, PrincipalKind::Admin),
        (device, DEVICE_ID.0, PrincipalKind::Device),
    ] {
        assert_eq!(response.token_type, "Bearer");
        assert_eq!(response.expires_in, 900);
        assert_eq!(
            jsonwebtoken::decode_header(&response.access_token)
                .unwrap()
                .alg,
            Algorithm::HS256
        );
        let claims = auth.validate_token(&response.access_token).unwrap();
        assert_eq!(claims.sub, subject);
        assert_eq!(claims.kind, kind);
        assert_eq!(claims.iss, JWT_ISSUER);
        assert_eq!(claims.aud, JWT_AUDIENCE);
        assert_eq!(claims.exp - claims.iat, 900);
        auth.authorize(&claims).await.unwrap();
    }
}

#[tokio::test]
async fn incorrect_credentials_and_malformed_stored_hashes_never_issue_tokens() {
    let (store, auth) = fixture().await;
    for (username, password) in [("admin", "wrong"), ("missing", "correct-password")] {
        assert!(matches!(
            auth.login_admin(AdminLoginRequest {
                username: username.into(),
                password: password.into()
            })
            .await,
            Err(AuthError::InvalidCredentials)
        ));
    }
    store
        .admins
        .lock()
        .unwrap()
        .get_mut(&ADMIN_ID)
        .unwrap()
        .password_hash = "not a PHC hash".into();
    assert!(matches!(
        auth.login_admin(AdminLoginRequest {
            username: "admin".into(),
            password: "correct-password".into()
        })
        .await,
        Err(AuthError::InvalidCredentials)
    ));
    for id in [DEVICE_ID, DeviceId(uuid::Uuid::new_v4())] {
        assert!(matches!(
            auth.login_device(DeviceCredentials {
                device_id: id,
                secret: URL_SAFE_NO_PAD.encode([4; 32])
            })
            .await,
            Err(AuthError::InvalidCredentials)
        ));
    }
    for stored in ["AB".repeat(32), "broken".into(), "00".repeat(32)] {
        store
            .devices
            .lock()
            .unwrap()
            .get_mut(&DEVICE_ID)
            .unwrap()
            .credential_hash = stored;
        assert!(matches!(
            auth.login_device(DeviceCredentials {
                device_id: DEVICE_ID,
                secret: URL_SAFE_NO_PAD.encode([3; 32])
            })
            .await,
            Err(AuthError::InvalidCredentials)
        ));
    }
}

#[tokio::test]
async fn registration_is_read_only_and_distinguishes_unknown_and_wrong_secret() {
    let (store, auth) = fixture().await;
    for (id, expected) in [(DEVICE_ID, true), (DeviceId(uuid::Uuid::new_v4()), false)] {
        assert_eq!(
            auth.check_registration(DeviceCredentials {
                device_id: id,
                secret: URL_SAFE_NO_PAD.encode([3; 32])
            })
            .await
            .unwrap()
            .registered,
            expected
        );
    }
    assert!(matches!(
        auth.check_registration(DeviceCredentials {
            device_id: DEVICE_ID,
            secret: URL_SAFE_NO_PAD.encode([5; 32])
        })
        .await,
        Err(AuthError::InvalidCredentials)
    ));
    for secret in [
        "".into(),
        "++//".into(),
        URL_SAFE_NO_PAD.encode([3; 31]),
        format!("{}=", URL_SAFE_NO_PAD.encode([3; 32])),
    ] {
        assert!(matches!(
            auth.check_registration(DeviceCredentials {
                device_id: DEVICE_ID,
                secret
            })
            .await,
            Err(AuthError::Backend(BackendError::Validation(_)))
        ));
    }
    assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(store.devices.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn all_required_claims_are_validated_and_tampering_is_rejected() {
    let (_, auth) = fixture().await;
    let now = jsonwebtoken::get_current_timestamp();
    let good = json!({"sub":ADMIN_ID, "kind":"admin", "iss":JWT_ISSUER, "aud":JWT_AUDIENCE, "iat":now, "exp":now+900});
    for name in ["sub", "kind", "iss", "aud", "iat", "exp"] {
        let mut claims = good.clone();
        claims.as_object_mut().unwrap().remove(name);
        assert!(
            matches!(
                auth.validate_token(&sign(claims, Algorithm::HS256, &JWT_KEY)),
                Err(AuthError::InvalidToken)
            ),
            "missing {name}"
        );
    }
    for (name, value) in [
        ("sub", json!("not-a-uuid")),
        ("kind", json!("root")),
        ("kind", json!("Admin")),
        ("iss", json!("other")),
        ("aud", json!("other")),
        ("exp", json!(now - 1)),
        ("exp", json!(now)),
        ("iat", json!(now + 60)),
        ("nbf", json!(now + 60)),
        ("exp", json!("tomorrow")),
        ("sub", json!(null)),
    ] {
        let mut claims = good.clone();
        claims[name] = value;
        assert!(
            matches!(
                auth.validate_token(&sign(claims, Algorithm::HS256, &JWT_KEY)),
                Err(AuthError::InvalidToken)
            ),
            "invalid {name}"
        );
    }
    for invalid in [
        "not.a.token".into(),
        String::new(),
        sign(good.clone(), Algorithm::HS256, &[8; 32]),
        sign(good.clone(), Algorithm::HS384, &JWT_KEY),
        format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(good.to_string())
        ),
    ] {
        assert!(matches!(
            auth.validate_token(&invalid),
            Err(AuthError::InvalidToken)
        ));
    }
    let valid = sign(good.clone(), Algorithm::HS256, &JWT_KEY);
    let mut parts: Vec<_> = valid.split('.').map(str::to_owned).collect();
    let mut tampered = good;
    tampered["kind"] = json!("device");
    parts[1] = URL_SAFE_NO_PAD.encode(tampered.to_string());
    assert!(auth.validate_token(&parts.join(".")).is_err());
}

#[tokio::test]
async fn deleted_subjects_and_repository_failures_are_rechecked() {
    let (store, auth) = fixture().await;
    for (subject, kind) in [
        (ADMIN_ID, PrincipalKind::Admin),
        (DEVICE_ID.0, PrincipalKind::Device),
    ] {
        let claims = auth.validate_token(&token(subject, kind)).unwrap();
        auth.authorize(&claims).await.unwrap();
        store.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(
            auth.authorize(&claims).await,
            Err(AuthError::Backend(_))
        ));
        store.fail.store(false, std::sync::atomic::Ordering::SeqCst);
        match kind {
            PrincipalKind::Admin => {
                store.admins.lock().unwrap().remove(&subject);
            }
            PrincipalKind::Device => {
                store.devices.lock().unwrap().remove(&DeviceId(subject));
            }
        }
        assert!(matches!(
            auth.authorize(&claims).await,
            Err(AuthError::InvalidToken)
        ));
    }
}

#[tokio::test]
async fn passwords_are_salted_argon2id_and_input_is_bounded() {
    let a = hash_password("test password".into()).await.unwrap();
    let b = hash_password("test password".into()).await.unwrap();
    assert_ne!(a, b);
    assert!(a.starts_with("$argon2id$v=19$"));
    assert!(
        Argon2::default()
            .verify_password(b"test password", &PasswordHash::new(&a).unwrap())
            .is_ok()
    );
    for password in [String::new(), "x".repeat(1025)] {
        assert!(matches!(
            hash_password(password).await,
            Err(BackendError::Validation(_))
        ));
    }
}

#[test]
fn login_budget_resets_only_after_the_window() {
    let now = Instant::now();
    let mut budget = LoginBudget {
        start: now,
        attempts: 0,
    };
    for _ in 0..LOGIN_LIMIT {
        budget.acquire(now).unwrap();
    }
    assert!(matches!(
        budget.acquire(now + Duration::from_secs(59)),
        Err(AuthError::RateLimited)
    ));
    budget.acquire(now + LOGIN_WINDOW).unwrap();
    assert_eq!(budget.attempts, 1);
}
