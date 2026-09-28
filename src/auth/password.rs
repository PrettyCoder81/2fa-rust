use argon2::{
  password_hash::{
    rand_core::OsRng,
    PasswordHash,
    PasswordHasher,
    PasswordVerifier,
    SaltString,
  },
  Argon2
};

pub fn hash_password(password: &str) -> Result<String, String> {
  let salt = SaltString::generate(&mut OsRng);

  Argon2::default()
    .hash_password(password.as_bytes(), &salt)
    .map(|hash| hash.to_string())
    .map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
  let parsed_hash = match PasswordHash::new(password_hash) {
      Ok(hash) => hash,
      Err(_) => return false
  };

  Argon2::default()
    .verify_password(password.as_bytes(), &parsed_hash)
    .is_ok()
}