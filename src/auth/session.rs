use rand::{TryRngCore, rngs::OsRng};
use sha2::{Digest, Sha256};

pub fn generate_token() -> String {
  let mut bytes = [0u8; 32];
  
  let _ = OsRng.try_fill_bytes(&mut bytes);

  hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
  let mut hasher = Sha256::new();
  
  hasher.update(token.as_bytes());
  
  hex::encode(hasher.finalize())
}

pub fn generate_challenge_token() -> String {
  generate_token()
}

pub fn generate_session_token() -> (String, String) {
  let raw_token = generate_token();
  let token_hash = hash_token(&raw_token);

  (raw_token, token_hash)
}