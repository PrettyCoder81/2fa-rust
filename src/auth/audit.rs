use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    models::security_event::NewSecurityEvent,
    schema::security_events,
    DbConn,
};

pub struct AuditEvent<'a> {
    pub user_id: Option<Uuid>,
    pub event_type: &'a str,
    pub ip_address: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub metadata: Option<Value>,
}

pub async fn record_event(
    db: &mut DbConn,
    event: AuditEvent<'_>,
) -> Result<(), String> {
    let new_event = NewSecurityEvent {
        id: Uuid::new_v4(),
        user_id: event.user_id,
        event_type: event.event_type.to_string(),
        ip_address: event.ip_address.map(str::to_string),
        user_agent: event.user_agent.map(str::to_string),
        metadata: event.metadata,
    };

    db.run(move |connection| {
        diesel::insert_into(security_events::table)
            .values(&new_event)
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(())
}
