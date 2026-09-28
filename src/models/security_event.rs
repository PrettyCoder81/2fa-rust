use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::schema::security_events;

#[derive(Queryable, Serialize)]
pub struct SecurityEvent {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = security_events)]
pub struct NewSecurityEvent {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Option<Value>,
}
