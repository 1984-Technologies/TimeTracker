#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| timetracker_auth_fuzz::check_signed_claims(data));
