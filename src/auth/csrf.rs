use rand::{TryRngCore, rngs::OsRng};
use subtle::ConstantTimeEq;

pub fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 32];

    let _ = OsRng.try_fill_bytes(&mut bytes);

    hex::encode(bytes)
}

pub fn valid_csrf_token(cookie_token: &str, header_token: &str) -> bool {
    cookie_token
        .as_bytes()
        .ct_eq(header_token.as_bytes())
        .into()
}
