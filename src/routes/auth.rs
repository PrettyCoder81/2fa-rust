use chrono::NaiveDateTime;
use chrono::{Duration, Utc};
use diesel::prelude::*;
use rocket::{
    http::{Cookie, CookieJar, SameSite, Status},
    request::Request,
    serde::json::Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::guards::ClientInfo;
use crate::{
    DbConn,
    auth::{
        audit, audit_events,
        cookie::{remove_session_cookie, session_cookie},
        csrf::generate_csrf_token,
        guards::{AuthenticatedUser, CsrfProtected},
        password::{hash_password, verify_password},
        recovery::{generate_recovery_code, hash_recovery_code},
        session::{generate_challenge_token, generate_session_token, hash_token},
        totp::{create_totp_from_secret, generate_totp},
    },
    models::{
        recovery_code::{NewRecoveryCode, RecoveryCode},
        session::NewSession,
        two_factor::NewTwoFactorSecret,
        two_factor_challenge::{NewTwoFactorChallenge, TwoFactorChallenge},
        user::{NewUser, User},
    },
    schema::{recovery_codes, sessions, two_factor_challenges, two_factor_secrets, users},
};

// register
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub id: Uuid,
    pub email: String,
}

#[post("/register", data = "<request>")]
pub async fn register(
    db: DbConn,
    request: Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, String> {
    let password_hash = hash_password(&request.password)?;

    let new_user = NewUser {
        id: Uuid::new_v4(),
        username: request.username.clone(),
        email: request.email.to_lowercase(),
        password_hash,
    };

    let userid = new_user.id;
    let email = new_user.email.clone();

    let _result = db
        .run(|conn| {
            diesel::insert_into(users::table)
                .values(new_user)
                .execute(conn)
        })
        .await
        .map_err(|error| error.to_string());

    Ok(Json(RegisterResponse { id: userid, email }))
}

// login
#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub message: String,
    pub requires_2fa: bool,
    pub challenge_token: Option<String>,
}

#[options("/<_..>")]
pub fn options() -> Status {
    Status::NoContent
}

#[post("/login", data = "<request>")]
pub async fn login(
    db: DbConn,
    cookies: &CookieJar<'_>,
    request: Json<LoginRequest>,
    client: ClientInfo,
) -> Result<Json<LoginResponse>, String> {
    let email = request.email.clone();
    let password = request.password.clone();

    let user: User = db
        .run(move |connection| {
            users::table
                .select(User::as_select())
                .filter(users::email.eq(&email))
                .first::<User>(connection)
        })
        .await
        .map_err(|_| "Invalid email or password".to_string())?;

    let password_valid = verify_password(&password, &user.password_hash);

    if !password_valid {
        return Err("Invalid email or password".to_string());
    }

    if user.two_factor_enabled {
        let raw_challenge = generate_challenge_token();
        let challenge_hash = hash_token(&raw_challenge);

        let challenge = NewTwoFactorChallenge {
            id: Uuid::new_v4(),
            user_id: user.id,
            token_hash: challenge_hash,
            expires_at: (Utc::now() + Duration::minutes(5)).naive_utc(),
        };

        let _ = db
            .run(move |connection| {
                diesel::insert_into(two_factor_challenges::table)
                    .values(&challenge)
                    .execute(connection)
            })
            .await
            .map(|error| error.to_string());

        return Ok(Json(LoginResponse {
            message: "Two-factor authentication required".to_string(),
            requires_2fa: true,
            challenge_token: Some(raw_challenge),
        }));
    }

    let (raw_token, token_hash) = generate_session_token();

    let session_id = Uuid::new_v4();

    let expires_at = (Utc::now() + Duration::days(7)).naive_utc();

    let new_session = NewSession {
        id: session_id,
        user_id: user.id,
        token_hash,
        expires_at,

        ip_address: client.ip_address.clone(),
        user_agent: client.user_agent.clone(),
        device_name: client.device.clone(),
    };

    db.run(move |connection| {
        diesel::insert_into(sessions::table)
            .values(new_session)
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie = session_cookie(raw_token);

    cookies.add(cookie);
    Ok(Json(LoginResponse {
        message: "Login successful".to_string(),
        requires_2fa: false,
        challenge_token: None,
    }))
}

// profile
#[derive(Serialize)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub email: String,
    pub two_factor_enabled: bool,
}

#[get("/profile")]
pub async fn profile(user: AuthenticatedUser) -> Json<ProfileResponse> {
    Json(ProfileResponse {
        id: user.id,
        email: user.email,
        two_factor_enabled: user.two_factor_enabled,
    })
}

// logout
#[post("/logout")]
pub async fn logout(_csrf: CsrfProtected, db: DbConn, cookies: &CookieJar<'_>) -> Status {
    let cookie = match cookies.get("session") {
        Some(cookie) => cookie,
        None => return Status::NoContent,
    };

    let raw_token = cookie.value().to_string();
    let token_hash = hash_token(&raw_token);

    let result = db
        .run(move |connection| {
            diesel::update(
                sessions::table
                    .filter(sessions::token_hash.eq(token_hash))
                    .filter(sessions::revoked_at.is_null()),
            )
            .set(sessions::revoked_at.eq(Utc::now().naive_utc()))
            .execute(connection)
        })
        .await;

    cookies.remove(remove_session_cookie());

    match result {
        Ok(_) => Status::NoContent,
        Err(_) => Status::InternalServerError,
    }
}

// logout all
#[post("/logout-all")]
pub async fn logout_all(
    _csrf: CsrfProtected,
    db: DbConn,
    user: AuthenticatedUser,
    cookies: &CookieJar<'_>,
) -> Status {
    let user_id = user.id;

    let result = db
        .run(move |connection| {
            diesel::update(
                sessions::table
                    .filter(sessions::user_id.eq(user_id))
                    .filter(sessions::revoked_at.is_null()),
            )
            .set(sessions::revoked_at.eq(Utc::now().naive_utc()))
            .execute(connection)
        })
        .await;

    cookies.remove(remove_session_cookie());

    match result {
        Ok(_) => Status::NoContent,
        Err(_) => Status::InternalServerError,
    }
}

// Two factor authentication
// setup 2fa
#[derive(Serialize)]
pub struct TwoFactorSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
}

#[post("/2fa/setup")]
pub async fn setup_2fa(
    _csrf: CsrfProtected,
    db: DbConn,
    user: AuthenticatedUser,
) -> Result<Json<TwoFactorSetupResponse>, String> {
    let (totp, secret) = generate_totp()?;
    let otpauth_url = totp.get_url();

    let new_secret = NewTwoFactorSecret {
        id: Uuid::new_v4(),
        user_id: user.id,
        secret: secret.clone(),
    };
    let _result = db
        .run(move |connection| {
            diesel::insert_into(two_factor_secrets::table)
                .values(&new_secret)
                .on_conflict(two_factor_secrets::user_id)
                .do_update()
                .set(two_factor_secrets::secret.eq(&new_secret.secret))
                .execute(connection)
        })
        .await
        .map_err(|error| error.to_string());

    Ok(Json(TwoFactorSetupResponse {
        secret,
        otpauth_url,
    }))
}

// confirm 2fa
#[derive(Deserialize)]
pub struct TwoFactorCodeRequest {
    pub code: String,
}

#[derive(Serialize)]
pub struct TwoFactorConfirmResponse {
    pub message: String,
    pub recovery_codes: Vec<String>,
}

#[post("/2fa/confirm", data = "<request>")]
pub async fn confirm_2fa(
    _csrf: CsrfProtected,
    db: DbConn,
    user: AuthenticatedUser,
    request: Json<TwoFactorCodeRequest>,
) -> Result<Json<TwoFactorConfirmResponse>, String> {
    let user_id = user.id;

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(two_factor_secrets::user_id.eq(user_id))
                .select(two_factor_secrets::secret)
                .first::<String>(connection)
        })
        .await
        .map_err(|_| "2FA setup not found".to_string())?;

    let totp = create_totp_from_secret(&secret)?;

    let valid = totp
        .check_current(&request.code)
        .map_err(|error| error.to_string())?;

    if !valid {
        return Err("Invalid authentication code".to_string());
    }

    // recovery key
    let mut recovery_codes_plain = Vec::new();
    let mut recovery_code_rows = Vec::new();

    for _ in 0..10 {
        let code = generate_recovery_code();
        let code_hash = hash_recovery_code(&code);

        recovery_codes_plain.push(code);

        recovery_code_rows.push(NewRecoveryCode {
            id: Uuid::new_v4(),
            user_id,
            code_hash,
        });
    }

    let _ = db
        .run(move |connection| {
            diesel::delete(recovery_codes::table.filter(recovery_codes::user_id.eq(user_id)))
                .execute(connection)
        })
        .await
        .map_err(|error| error.to_string());

    let _ = db
        .run(move |connection| {
            diesel::insert_into(recovery_codes::table)
                .values(&recovery_code_rows)
                .execute(connection)
        })
        .await
        .map_err(|error| error.to_string());

    let _result = db
        .run(move |connection| {
            diesel::update(
                two_factor_secrets::table
                    .filter(two_factor_secrets::user_id.eq(user_id))
                    .filter(two_factor_secrets::confirmed_at.is_null()),
            )
            .set(two_factor_secrets::confirmed_at.eq(Utc::now().naive_utc()))
            .execute(connection)
        })
        .await;
    let _result = db
        .run(move |connection| {
            diesel::update(users::table.filter(users::id.eq(user_id)))
                .set(users::two_factor_enabled.eq(true))
                .execute(connection)
        })
        .await;

    Ok(Json(TwoFactorConfirmResponse {
        message: "Two-factor authentication enabled".to_string(),
        recovery_codes: recovery_codes_plain,
    }))
}

// verify 2fa
#[derive(Deserialize)]
pub struct TwoFactorVerifyRequest {
    pub challenge_token: String,
    pub code: String,
}

#[post("/2fa/verify", data = "<request>")]
pub async fn verify_2fa(
    client: ClientInfo,
    db: DbConn,
    request: Json<TwoFactorVerifyRequest>,
    cookies: &CookieJar<'_>,
) -> Result<Json<LoginResponse>, String> {
    let challenge_hash = hash_token(&request.challenge_token);

    let now = Utc::now().naive_utc();

    let challenge = db
        .run(move |connection| {
            two_factor_challenges::table
                .filter(two_factor_challenges::token_hash.eq(&challenge_hash))
                .filter(two_factor_challenges::expires_at.gt(now))
                .first::<TwoFactorChallenge>(connection)
        })
        .await
        .map_err(|_| "Invalid or expired challenge".to_string())?;

    let user_id = challenge.user_id;

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(two_factor_secrets::user_id.eq(user_id))
                .filter(two_factor_secrets::confirmed_at.is_not_null())
                .select(two_factor_secrets::secret)
                .first::<String>(connection)
        })
        .await
        .map_err(|_| "Two-factor authentication is not configured".to_string())?;

    let totp = create_totp_from_secret(&secret)?;

    let valid = totp
        .check_current(&request.code)
        .map_err(|error| error.to_string())?;

    if !valid {
        return Err("Invalid authentication code".to_string());
    }

    let (raw_token, token_hash) = generate_session_token();

    let session_expires_at = now + Duration::days(7);

    let new_session = NewSession {
        id: Uuid::new_v4(),
        user_id,
        token_hash,
        expires_at: session_expires_at,

        ip_address: client.ip_address.clone(),
        user_agent: client.user_agent.clone(),
        device_name: client.device.clone(),
    };

    let challenge_id = challenge.id;

    db.run(move |connection| {
        connection.transaction::<(), diesel::result::Error, _>(|connection| {
            /*
             * Re-check the challenge inside the
             * transaction to prevent replay.
             */
            let challenge_exists = two_factor_challenges::table
                .filter(two_factor_challenges::id.eq(challenge_id))
                .filter(two_factor_challenges::expires_at.gt(now))
                .select(two_factor_challenges::id)
                .first::<Uuid>(connection)
                .optional()
                .map_err(|_| diesel::result::Error::NotFound)?;

            if challenge_exists.is_none() {
                return Err(diesel::result::Error::NotFound);
            }

            diesel::insert_into(sessions::table)
                .values(&new_session)
                .execute(connection)
                .map_err(|_| diesel::result::Error::NotFound)?;

            diesel::delete(
                two_factor_challenges::table.filter(two_factor_challenges::id.eq(challenge_id)),
            )
            .execute(connection)
            .map_err(|_| diesel::result::Error::NotFound)?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie = session_cookie(raw_token);

    cookies.add(cookie);

    Ok(Json(LoginResponse {
        message: "Login successful".to_string(),
        requires_2fa: false,
        challenge_token: None,
    }))
}

// disable 2fa

#[derive(Deserialize)]
pub struct DisableTwoFactorRequest {
    pub password: String,
    pub code: String,
}

#[post("/2fa/disable", data = "<request>")]
pub async fn disable_2fa(
    _csrf: CsrfProtected,
    db: DbConn,
    user: AuthenticatedUser,
    request: Json<DisableTwoFactorRequest>,
) -> Result<Status, String> {
    let user_id = user.id;

    let password_hash = db
        .run(move |connection| {
            users::table
                .filter(users::id.eq(user_id))
                .select(users::password_hash)
                .first::<String>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    let password_valid = verify_password(&request.password, &password_hash);

    if !password_valid {
        return Err("Invalid password".to_string());
    }

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(two_factor_secrets::user_id.eq(user_id))
                .filter(two_factor_secrets::confirmed_at.is_not_null())
                .select(two_factor_secrets::secret)
                .first::<String>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    let totp = create_totp_from_secret(&secret)?;

    let code_valid = totp
        .check_current(&request.code)
        .map_err(|error| error.to_string())?;

    if !code_valid {
        return Err("Invalid authentication code".to_string());
    }

    let _ = db
        .run(move |connection| {
            connection.transaction::<_, diesel::result::Error, _>(|connection| {
                let _ = diesel::update(users::table.filter(users::id.eq(user_id)))
                    .set(users::two_factor_enabled.eq(false))
                    .execute(connection);

                let _ = diesel::delete(
                    two_factor_secrets::table.filter(two_factor_secrets::user_id.eq(user_id)),
                )
                .execute(connection);

                let _ = diesel::delete(
                    recovery_codes::table.filter(recovery_codes::user_id.eq(user_id)),
                )
                .execute(connection);

                Ok(())
            })
        })
        .await
        .map_err(|error| error.to_string());

    Ok(Status::NoContent)
}

#[derive(Deserialize)]
pub struct RecoveryCodeVerifyRequest {
    pub challenge_token: String,
    pub code: String,
}

#[post("/2fa/recovery", data = "<request>")]
pub async fn verify_recovery_code(
    client: ClientInfo,
    db: DbConn,
    cookies: &CookieJar<'_>,
    request: Json<RecoveryCodeVerifyRequest>,
) -> Result<Json<LoginResponse>, String> {
    let challenge_hash = hash_token(&request.challenge_token);
    let recovery_hash = hash_recovery_code(&request.code);

    let (raw_token, session_token_hash) = generate_session_token();

    let now = Utc::now().naive_utc();
    let session_expires_at = now + Duration::days(7);

    let cookie = session_cookie(raw_token);

    db.run(move |connection| {
        connection.transaction::<(), diesel::result::Error, _>(|connection| {
            // Find the challenge.
            let challenge = two_factor_challenges::table
                .filter(two_factor_challenges::token_hash.eq(&challenge_hash))
                .first::<TwoFactorChallenge>(connection)
                .map_err(|_| diesel::result::Error::NotFound)?;

            // Check challenge expiration.
            if challenge.expires_at < now {
                return Err(diesel::result::Error::NotFound);
            }

            let user_id = challenge.user_id;

            // Find an unused recovery code.
            let recovery_code = recovery_codes::table
                .filter(recovery_codes::user_id.eq(user_id))
                .filter(recovery_codes::code_hash.eq(&recovery_hash))
                .filter(recovery_codes::used_at.is_null())
                .first::<RecoveryCode>(connection)
                .map_err(|_| diesel::result::Error::NotFound)?;

            // Mark recovery code as used.
            let updated = diesel::update(
                recovery_codes::table
                    .filter(recovery_codes::id.eq(recovery_code.id))
                    .filter(recovery_codes::used_at.is_null()),
            )
            .set(recovery_codes::used_at.eq(now))
            .execute(connection)
            .map_err(|_| diesel::result::Error::NotFound)?;

            if updated != 1 {
                return Err(diesel::result::Error::NotFound);
            }

            // Create authenticated session.
            let new_session = NewSession {
                id: Uuid::new_v4(),
                user_id,
                token_hash: session_token_hash,
                expires_at: session_expires_at,

                ip_address: client.ip_address.clone(),
                user_agent: client.user_agent.clone(),
                device_name: client.device.clone(),
            };

            diesel::insert_into(sessions::table)
                .values(&new_session)
                .execute(connection)
                .map_err(|_| diesel::result::Error::NotFound)?;

            // Delete the challenge so it cannot be reused.
            let deleted = diesel::delete(
                two_factor_challenges::table.filter(two_factor_challenges::id.eq(challenge.id)),
            )
            .execute(connection)
            .map_err(|_| diesel::result::Error::NotFound)?;

            if deleted != 1 {
                return Err(diesel::result::Error::NotFound);
            }

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    cookies.add(cookie);

    Ok(Json(LoginResponse {
        message: "Login successful".to_string(),
        requires_2fa: false,
        challenge_token: None,
    }))
}

// regenerate recovery codes
#[derive(Deserialize)]
pub struct RegenerateRecoveryCodesRequest {
    pub password: String,
    pub code: String,
}

#[derive(Serialize)]
pub struct RecoveryCodesResponse {
    pub recovery_codes: Vec<String>,
}

#[post("/2fa/recovery-codes/regenerate", data = "<request>")]
pub async fn regenerate_recovery_codes(
    _csrf: CsrfProtected,
    db: DbConn,
    user: AuthenticatedUser,
    request: Json<RegenerateRecoveryCodesRequest>,
) -> Result<Json<RecoveryCodesResponse>, String> {
    let user_id = user.id;

    let password_hash = db
        .run(move |connection| {
            users::table
                .filter(users::id.eq(user_id))
                .select(users::password_hash)
                .first::<String>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    if !verify_password(&request.password, &password_hash) {
        return Err("Invalid password".to_string());
    }

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(two_factor_secrets::user_id.eq(user_id))
                .filter(two_factor_secrets::confirmed_at.is_not_null())
                .select(two_factor_secrets::secret)
                .first::<String>(connection)
        })
        .await
        .map_err(|_| "Two-factor authentication is not enabled".to_string())?;

    let totp = create_totp_from_secret(&secret)?;

    if !totp
        .check_current(&request.code)
        .map_err(|error| error.to_string())?
    {
        return Err("Invalid authentication code".to_string());
    }

    let mut raw_codes = Vec::with_capacity(10);
    for _ in 0..10 {
        raw_codes.push(generate_recovery_code());
    }

    let new_codes: Vec<NewRecoveryCode> = raw_codes
        .iter()
        .map(|code| NewRecoveryCode {
            id: Uuid::new_v4(),
            user_id,
            code_hash: hash_recovery_code(code),
        })
        .collect();

    db.run(move |connection| {
        connection.transaction::<_, diesel::result::Error, _>(|connection| {
            diesel::delete(recovery_codes::table.filter(recovery_codes::user_id.eq(user_id)))
                .execute(connection)?;

            diesel::insert_into(recovery_codes::table)
                .values(new_codes)
                .execute(connection)?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Json(RecoveryCodesResponse {
        recovery_codes: raw_codes,
    }))
}

#[get("/csrf")]
pub fn csrf_token(cookies: &CookieJar<'_>) {
    let token = generate_csrf_token();

    cookies.add(
        Cookie::build(("csrf_token", token))
            .path("/")
            .http_only(false)
            .secure(false)
            .same_site(SameSite::Lax)
            .build(),
    );
}

#[derive(Serialize)]
pub struct ActiveSessionResponse {
    pub id: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub current: bool,
}

#[get("/sessions")]
pub async fn list_sessions(
    client: ClientInfo,
    db: DbConn,
    user: AuthenticatedUser,
    cookies: &CookieJar<'_>,
) -> Result<Json<Vec<ActiveSessionResponse>>, String> {
    let user_id = user.id;

    // Hash the current session token so we can identify it.
    let current_session_hash = cookies
        .get("session")
        .map(|cookie| hash_token(cookie.value()));

    let now = Utc::now().naive_utc();

    let sessions = db
        .run(move |connection| {
            sessions::table
                .filter(sessions::user_id.eq(user_id))
                .filter(sessions::revoked_at.is_null())
                .filter(sessions::expires_at.gt(now))
                .select((
                    sessions::id,
                    sessions::token_hash,
                    sessions::created_at,
                    sessions::last_used_at,
                    sessions::expires_at,
                ))
                .order(sessions::last_used_at.desc())
                .load::<(Uuid, String, NaiveDateTime, NaiveDateTime, NaiveDateTime)>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    let result = sessions
        .into_iter()
        .map(|(id, token_hash, created_at, last_used_at, expires_at)| {
            let current = current_session_hash.as_deref() == Some(token_hash.as_str());

            ActiveSessionResponse {
                id,
                ip_address: client.ip_address.clone(),
                user_agent: client.user_agent.clone(),
                device_name: client.device.clone(),
                created_at,
                last_used_at,
                expires_at,
                current,
            }
        })
        .collect();

    Ok(Json(result))
}

#[delete("/sessions/<session_id>")]
pub async fn revoke_session(
    db: DbConn,
    user: AuthenticatedUser,
    session_id: String,
    cookies: &CookieJar<'_>,
) -> Result<Status, Status> {
    let session_id = Uuid::parse_str(&session_id).map_err(|_| Status::BadRequest)?;

    let user_id = user.id;

    let current_session_hash = cookies
        .get("session")
        .map(|cookie| hash_token(cookie.value()));

    let target_token_hash = db
        .run(move |connection| {
            sessions::table
                .filter(sessions::id.eq(session_id))
                .filter(sessions::user_id.eq(user_id))
                .filter(sessions::revoked_at.is_null())
                .select(sessions::token_hash)
                .first::<String>(connection)
        })
        .await
        .map_err(|_| Status::NotFound)?;

    db.run(move |connection| {
        diesel::update(
            sessions::table
                .filter(sessions::id.eq(session_id))
                .filter(sessions::user_id.eq(user_id))
                .filter(sessions::revoked_at.is_null()),
        )
        .set(sessions::revoked_at.eq(Utc::now().naive_utc()))
        .execute(connection)
    })
    .await
    .map_err(|_| Status::InternalServerError)?;

    if current_session_hash.as_deref() == Some(target_token_hash.as_str()) {
        cookies.remove(remove_session_cookie());
    }

    Ok(Status::NoContent)
}

// change password

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[post("/password/change", data = "<request>")]
pub async fn change_password(
    db: DbConn,
    user: AuthenticatedUser,
    _csrf: CsrfProtected,
    cookies: &CookieJar<'_>,
    request: Json<ChangePasswordRequest>,
) -> Result<Status, String> {
    if request.current_password == request.new_password {
        return Err("New password must be different from current password".to_string());
    }

    let user_id = user.id;

    let password_hash = db
        .run(move |connection| {
            users::table
                .filter(users::id.eq(user_id))
                .select(users::password_hash)
                .first::<String>(connection)
        })
        .await
        .map_err(|_| "Unable to load account".to_string())?;

    let valid = verify_password(&request.current_password, &password_hash);

    if !valid {
        return Err("Current password is incorrect".to_string());
    }

    let new_password_hash = hash_password(&request.new_password)?;

    // let current_session_hash = cookies
    //     .get("session")
    //     .map(|cookie| {
    //         hash_token(cookie.value())
    //     });

    let now = Utc::now().naive_utc();

    db.run(move |connection| {
        connection.transaction::<(), diesel::result::Error, _>(|connection| {
            diesel::update(users::table.filter(users::id.eq(user_id)))
                .set((users::password_hash.eq(&new_password_hash),))
                .execute(connection)?;

            /*
             * Revoke every existing session.
             */
            diesel::update(
                sessions::table
                    .filter(sessions::user_id.eq(user_id))
                    .filter(sessions::revoked_at.is_null()),
            )
            .set(sessions::revoked_at.eq(now))
            .execute(connection)?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    cookies.remove(remove_session_cookie());

    Ok(Status::NoContent)
}
