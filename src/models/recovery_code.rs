use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::recovery_codes;

#[allow(dead_code)]
#[derive(Queryable)]
pub struct RecoveryCode {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code_hash: String,
    pub used_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[allow(dead_code)]
#[derive(Insertable)]
#[diesel(table_name = recovery_codes)]
pub struct NewRecoveryCode {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code_hash: String,
}
