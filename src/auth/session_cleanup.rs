use chrono::Utc;
use diesel::prelude::*;

use crate::schema::sessions;

pub fn cleanup_sessions(connection: &mut PgConnection) -> Result<usize, diesel::result::Error> {
    let now = Utc::now().naive_utc();
    let retention_cutoff = now - chrono::Duration::days(30);

    diesel::delete(
        sessions::table.filter(
            sessions::expires_at
                .le(now)
                .or(sessions::revoked_at.lt(retention_cutoff)),
        ),
    )
    .execute(connection)
}
