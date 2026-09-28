use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable, Selectable, pg::Pg};
use serde::Serialize;
use uuid::Uuid;

use crate::schema::users;

#[derive(Debug, Queryable, Selectable, Serialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(Pg))]
pub struct User {
  pub id: Uuid,
  pub email: String,
  pub username: String,
  pub firstname: Option<String>,
  pub lastname: Option<String>,
  pub avatar: String,
  pub two_factor_enabled: bool,

  #[serde(skip_serializing)]
  pub password_hash: String,
  
  pub created_at: NaiveDateTime
}

#[derive(Debug, Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
  pub id: Uuid,
  pub username: String,
  pub email: String,
  pub password_hash: String,
}