use chrono::NaiveDateTime;
use diesel::{Queryable, Insertable};
use uuid::Uuid;

use crate::schema::two_factor_challenges;

#[derive(Queryable)]
pub struct TwoFactorChallenge {
  pub id: Uuid,
  pub user_id: Uuid,
  pub token_hash: String,
  pub expires_at: NaiveDateTime,
  pub created_at: NaiveDateTime
}

#[derive(Insertable)]
#[diesel(table_name = two_factor_challenges)]
pub struct NewTwoFactorChallenge {
  pub id: Uuid,
  pub user_id: Uuid,
  pub token_hash: String,
  pub expires_at: NaiveDateTime
}