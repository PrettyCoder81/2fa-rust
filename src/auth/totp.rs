use rand::Rng;
use totp_rs::{Algorithm, Secret, TOTP};

pub fn generate_totp() -> Result<(TOTP, String), String> {
  let mut rng = rand::rng();
  let mut secret_bytes = [0u8; 20];
  rng.fill(&mut secret_bytes);

  let secret = Secret::Raw(secret_bytes.to_vec());
  let secret_base32 = secret.to_encoded().to_string();

  let totp = TOTP::new(
    Algorithm::SHA1,
    6,
    1,
    30,
    secret.to_bytes().map_err(|e| e.to_string())?,
    None,
    "silver".to_string()
  ).map_err(|e| e.to_string())?;

  Ok((totp, secret_base32))
}

pub fn create_totp_from_secret(secret: &str) -> Result<TOTP, String> {
  let secret = Secret::Encoded(secret.to_string());

  TOTP::new(
    Algorithm::SHA1,
    6,
    1,
    30,
    secret.to_bytes().map_err(|e| e.to_string())?,
    None,
    "silver".to_string()
  ).map_err(|e| e.to_string())
}