use chrono::Utc;
use diesel::prelude::*;
use rocket::{
    Request,
    http::Status,
    request::{FromRequest, Outcome},
};
use uuid::Uuid;

use crate::{
    DbConn,
    auth::{device::detect_device, session::hash_token},
    schema::{
        sessions::{self},
        users,
    },
};

pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
    pub two_factor_enabled: bool,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let cookies = request.cookies();

        let cookie = match cookies.get("session") {
            Some(cookie) => cookie,
            None => {
                return Outcome::Error((Status::Unauthorized, ()));
            }
        };

        let raw_token = cookie.value().to_string();
        let token_hash = hash_token(&raw_token);

        let db = match DbConn::get_one(request.rocket()).await {
            Some(db) => db,
            None => {
                return Outcome::Error((Status::InternalServerError, ()));
            }
        };

        let now = Utc::now().naive_utc();

        let result = db
            .run(move |connection| {
                sessions::table
                    .inner_join(users::table.on(users::id.eq(sessions::user_id)))
                    .filter(sessions::token_hash.eq(&token_hash))
                    .filter(sessions::revoked_at.is_null())
                    .filter(sessions::expires_at.gt(now))
                    .select((users::id, users::email, users::two_factor_enabled))
                    .first::<(Uuid, String, bool)>(connection)
            })
            .await;

        match result {
            Ok((user_id, email, two_factor_enabled)) => {
                let refresh_hash = hash_token(&raw_token);

                let _ = db
                    .run(move |connection| {
                        diesel::update(
                            sessions::table
                                .filter(sessions::token_hash.eq(refresh_hash))
                                .filter(sessions::revoked_at.is_null()),
                        )
                        .set(sessions::last_used_at.eq(now))
                        .execute(connection)
                    })
                    .await;

                Outcome::Success(AuthenticatedUser {
                    id: user_id,
                    email,
                    two_factor_enabled,
                })
            }

            Err(_) => Outcome::Error((Status::Unauthorized, ())),
        }
    }
}

pub struct CsrfProtected;

#[allow(dead_code)]
#[rocket::async_trait]
impl<'r> FromRequest<'r> for CsrfProtected {
    type Error = ();

    async fn from_request(_request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // let cookies = request.cookies();

        // let cookie_token = match cookies.get("csrf_token") {
        //     Some(cookie) => cookie.value(),
        //     None => {
        //         return Outcome::Error((Status::Forbidden, ()));
        //     }
        // };

        // let header_token = match request.headers().get_one("X-CSRF-Token") {
        //     Some(token) => token,
        //     None => {
        //         return Outcome::Error((Status::Forbidden, ()));
        //     }
        // };

        // if !valid_csrf_token(cookie_token, header_token) {
        //     return Outcome::Error((Status::Forbidden, ()));
        // }

        Outcome::Success(CsrfProtected)
    }
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device: Option<String>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ClientInfo {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let ip_address = req.remote().map(|remote| remote.ip().to_string());

        let user_agent = req
            .headers()
            .get_one("User-Agent")
            .map(|value| value.to_string());

        let device = user_agent.as_deref().map(detect_device);

        Outcome::Success(ClientInfo {
            ip_address,
            user_agent,
            device,
        })
    }
}
