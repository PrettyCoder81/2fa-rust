use rand::{TryRngCore, rngs::OsRng};

use crate::auth::session::hash_token;

pub fn generate_recovery_code() -> String {
    let mut bytes = [0u8; 8];
    let _ = OsRng.try_fill_bytes(&mut bytes);
    let value = u64::from_be_bytes(bytes);
    format!("{:016X}", value)
}

pub fn hash_recovery_code(code: &str) -> String {
    hash_token(code)
}
