//! Oracles shared by libFuzzer and deterministic seed replay on stable Rust.
mod authorization;
mod credentials;
mod jwt;

pub use authorization::check_authorization;
pub use credentials::check_credentials;
pub use jwt::{check_raw_jwt, check_signed_claims};

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    fn replay(name: &str, check: fn(&[u8])) {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("seeds")
            .join(name);
        let mut count = 0;
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                eprintln!("replaying {}", path.display());
                check(&fs::read(path).unwrap());
                count += 1;
            }
        }
        assert!(count > 0, "missing seeds for {name}");
    }

    #[test]
    fn replay_raw_jwt_seeds() {
        replay("jwt_raw", check_raw_jwt);
    }
    #[test]
    fn replay_signed_claims_seeds() {
        replay("jwt_claims", check_signed_claims);
    }
    #[test]
    fn replay_credential_seeds() {
        replay("credentials", check_credentials);
    }
    #[test]
    fn replay_authorization_seeds() {
        replay("authorization", check_authorization);
    }
}
