use chrono::{Duration, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    DbConn,
    auth::{
        cookie::session_cookie,
        request_metadata::RequestMetadata,
        session::{generate_session_token, hash_token},
    },
    models::session::NewSession,
    schema::sessions,
};

#[allow(dead_code)]
pub async fn rotate_session(
    db: &mut DbConn,
    current_token: &str,
    user_id: Uuid,
    _meta: RequestMetadata,
) -> Result<rocket::http::Cookie<'static>, String> {
    let current_hash = hash_token(current_token);

    let (new_raw_token, new_token_hash) = generate_session_token();

    let now = Utc::now().naive_utc();

    let expires_at = now + Duration::days(7);

    let new_session = NewSession {
        id: Uuid::new_v4(),
        user_id,
        token_hash: new_token_hash,
        expires_at,
        ip_address: None,
        user_agent: None,
        device_name: None,
    };

    db.run(move |connection| {
        connection.transaction::<(), diesel::result::Error, _>(|connection| {
            let current_session_exists = sessions::table
                .filter(sessions::token_hash.eq(&current_hash))
                .filter(sessions::user_id.eq(user_id))
                .filter(sessions::revoked_at.is_null())
                .filter(sessions::expires_at.gt(now))
                .select(sessions::id)
                .first::<Uuid>(connection)
                .optional()
                .map_err(|_| diesel::result::Error::NotFound)?;

            if current_session_exists.is_none() {
                return Err(diesel::result::Error::NotFound);
            }

            diesel::update(
                sessions::table
                    .filter(sessions::token_hash.eq(&current_hash))
                    .filter(sessions::user_id.eq(user_id))
                    .filter(sessions::revoked_at.is_null()),
            )
            .set(sessions::revoked_at.eq(now))
            .execute(connection)
            .map_err(|_| diesel::result::Error::NotFound)?;

            diesel::insert_into(sessions::table)
                .values(&new_session)
                .execute(connection)
                .map_err(|_| diesel::result::Error::NotFound)?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(session_cookie(new_raw_token))
}
