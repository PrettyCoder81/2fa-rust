use chrono::NaiveDateTime;
use diesel::prelude::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::two_factor_secrets;

#[derive(Queryable)]
pub struct TwoFactorSecret {
    pub id: Uuid,
    pub user_id: Uuid,
    pub secret: String,
    pub created_at: NaiveDateTime,
    pub confirmed_at: Option<NaiveDateTime>,
}

#[derive(Insertable)]
#[diesel(table_name = two_factor_secrets)]
pub struct NewTwoFactorSecret {
    pub id: Uuid,
    pub user_id: Uuid,
    pub secret: String,
}
