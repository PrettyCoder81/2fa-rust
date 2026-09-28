Yep. Let's build it **incrementally in your existing Rocket + Diesel + PostgreSQL project**, so you can understand each piece instead of dropping a giant auth system on you at once.

We'll use:

* Rocket `0.5.1`
* Diesel `2.2`
* PostgreSQL
* Argon2id for passwords
* Server-side sessions
* HttpOnly cookies
* TOTP for 2FA
* Recovery codes later

## Step 1 — Add authentication dependencies

Your `Cargo.toml` can start like this:

```toml
[dependencies]
rocket = { version = "0.5.1", features = ["json"] }
rocket_sync_db_pools = { version = "0.1.0", features = ["diesel_postgres_pool"] }

diesel = { version = "2.2", features = ["postgres", "uuid", "chrono"] }
dotenvy = "0.15"

serde = { version = "1", features = ["derive"] }
serde_json = "1"

uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

argon2 = "0.5"
rand = "0.8"
```

One important correction for your existing project: with `rocket_sync_db_pools`, make sure the feature name matches the version you're actually using. If Cargo complains about `diesel_postgres_pool`, we'll adjust it to the feature exposed by your installed crate version.

---

# Step 2 — Create the users table

Create a Diesel migration:

```text
migrations/
└── 00000000000001_create_users/
    ├── up.sql
    └── down.sql
```

### `up.sql`

```sql
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    two_factor_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### `down.sql`

```sql
DROP TABLE users;
```

Run:

```bash
diesel migration run
```

Then regenerate your schema:

```bash
diesel print-schema > src/schema.rs
```

You should get something similar to:

```rust
diesel::table! {
    users (id) {
        id -> Uuid,
        email -> Varchar,
        password_hash -> Varchar,
        two_factor_enabled -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}
```

---

# Step 3 — Create the User model

Create:

```text
src/models/user.rs
```

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use serde::Serialize;
use uuid::Uuid;

use crate::schema::users;

#[derive(Queryable, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub two_factor_enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
}
```

Then:

```text
src/models/mod.rs
```

```rust
pub mod user;
```

And in `main.rs`:

```rust
mod models;
```

---

# Step 4 — Password hashing

Create:

```text
src/auth/password.rs
```

```rust
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash,
        PasswordHasher,
        PasswordVerifier,
        SaltString,
    },
    Argon2,
};

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);

    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| error.to_string())
}

pub fn verify_password(
    password: &str,
    password_hash: &str,
) -> Result<bool, String> {
    let parsed_hash =
        PasswordHash::new(password_hash)
            .map_err(|error| error.to_string())?;

    Ok(
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    )
}
```

Create:

```text
src/auth/mod.rs
```

```rust
pub mod password;
```

And:

```rust
mod auth;
```

in `main.rs`.

---

# Step 5 — Registration endpoint

Now we'll create:

```text
POST /auth/register
```

Create:

```text
src/auth/routes.rs
```

```rust
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use diesel::prelude::*;

use crate::{
    auth::password::hash_password,
    models::user::NewUser,
    schema::users,
    DbConn,
};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub id: Uuid,
    pub email: String,
}

#[post("/register", data = "<request>")]
pub async fn register(
    mut db: DbConn,
    request: Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, String> {

    let password_hash = hash_password(&request.password)?;

    let new_user = NewUser {
        id: Uuid::new_v4(),
        email: request.email.to_lowercase(),
        password_hash,
    };

    let user_id = new_user.id;
    let email = new_user.email.clone();

    db.run(move |connection| {
        diesel::insert_into(users::table)
            .values(&new_user)
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Json(RegisterResponse {
        id: user_id,
        email,
    }))
}
```

---

# Step 6 — Mount the route

In `main.rs`:

```rust
mod auth;
mod models;
mod schema;

use rocket::launch;

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register
            ],
        )
}
```

And update `auth/mod.rs`:

```rust
pub mod password;
pub mod routes;
```

Now:

```bash
cargo run
```

Test:

```http
POST http://localhost:8000/auth/register
Content-Type: application/json

{
    "email": "test@example.com",
    "password": "MyStrongPassword123!"
}
```

You should get something like:

```json
{
    "id": "some-uuid",
    "email": "test@example.com"
}
```

---

# Step 7 — Create the sessions table

Now we get to the important part.

Create another migration:

```text
migrations/
└── 00000000000002_create_sessions/
    ├── up.sql
    └── down.sql
```

### `up.sql`

```sql
CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at TIMESTAMP NULL
);

CREATE INDEX sessions_user_id_idx
ON sessions(user_id);

CREATE INDEX sessions_token_hash_idx
ON sessions(token_hash);
```

### `down.sql`

```sql
DROP TABLE sessions;
```

Run:

```bash
diesel migration run
```

Then:

```bash
diesel print-schema > src/schema.rs
```

---

# Step 8 — Session model

Create:

```text
src/models/session.rs
```

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::sessions;

#[derive(Queryable)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

#[derive(Insertable)]
#[diesel(table_name = sessions)]
pub struct NewSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
}
```

Update:

```text
src/models/mod.rs
```

```rust
pub mod session;
pub mod user;
```

---

# Step 9 — Generate secure session tokens

Create:

```text
src/auth/session.rs
```

```rust
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];

    OsRng.fill_bytes(&mut bytes);

    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();

    hasher.update(token.as_bytes());

    hex::encode(hasher.finalize())
}
```

You'll need two dependencies:

```toml
sha2 = "0.10"
hex = "0.4"
```

The flow is:

```text
Browser
   │
   │ raw session token
   ▼
Cookie
   │
   ▼
Rocket
   │
   │ SHA-256
   ▼
token_hash
   │
   ▼
PostgreSQL
```

So the database never contains the actual cookie token.

---

# Step 10 — Login

Now create:

```rust
#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}
```

The login process should be:

```text
email
  ↓
find user
  ↓
verify Argon2 password
  ↓
2FA enabled?
  ├── NO  → create session
  │
  └── YES → temporary 2FA state
              ↓
          verify TOTP
              ↓
          create session
```

**Don't add TOTP yet.**

First we should make normal login + sessions work completely.

---

## Our implementation order

I'd build your project in this exact order:

```text
1. Users
   ↓
2. Argon2 password hashing
   ↓
3. Register
   ↓
4. Login
   ↓
5. Sessions
   ↓
6. HttpOnly cookies
   ↓
7. AuthenticatedUser Rocket guard
   ↓
8. Logout
   ↓
9. Logout all sessions
   ↓
10. TOTP setup
   ↓
11. QR code / otpauth URI
   ↓
12. 2FA verification
   ↓
13. Recovery codes
   ↓
14. Password change
   ↓
15. Session invalidation
   ↓
16. CSRF protection
   ↓
17. Rate limiting
```

**Next step should be Step 10: implement `/auth/login` and create the session cookie.** That will also give you the foundation for a Rocket `FromRequest` authentication guard, which is the really useful part of this architecture.

Yep — now let's implement **login + session creation + HttpOnly cookie**. This is the core of the authentication system.

One small dependency correction first: the session code from the previous step uses `sha2` and `hex`, so make sure these are in `Cargo.toml`:

```toml
sha2 = "0.10"
hex = "0.4"
```

## Step 10 — Implement login

### 1. Update `src/auth/session.rs`

We'll generate a random 256-bit session token and hash it before storing it in PostgreSQL.

```rust
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];

    OsRng.fill_bytes(&mut bytes);

    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();

    hasher.update(token.as_bytes());

    hex::encode(hasher.finalize())
}
```

The important distinction is:

```text
RAW TOKEN
    ↓
Cookie
    ↓
SHA-256
    ↓
Database
```

The raw token is **never stored in the database**.

---

# Step 11 — Create the login request

Open:

```text
src/auth/routes.rs
```

Add:

```rust
use chrono::{Duration, Utc};
use diesel::prelude::*;
use rocket::{
    http::{Cookie, SameSite},
    serde::json::Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{
        password::verify_password,
        session::{generate_token, hash_token},
    },
    models::{
        session::NewSession,
        user::User,
    },
    schema::{sessions, users},
    DbConn,
};
```

Then add:

```rust
#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub message: String,
}
```

---

# Step 12 — Implement `/auth/login`

Add this below your registration route:

```rust
#[post("/login", data = "<request>")]
pub async fn login(
    mut db: DbConn,
    request: Json<LoginRequest>,
) -> Result<(Cookie<'static>, Json<LoginResponse>), String> {

    let email = request.email.to_lowercase();
    let password = request.password.clone();

    let user: User = db
        .run(move |connection| {
            users::table
                .filter(users::email.eq(&email))
                .first::<User>(connection)
        })
        .await
        .map_err(|_| "Invalid email or password".to_string())?;

    let password_valid =
        verify_password(&password, &user.password_hash)?;

    if !password_valid {
        return Err("Invalid email or password".to_string());
    }

    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);

    let session_id = Uuid::new_v4();

    let expires_at =
        (Utc::now() + Duration::days(7)).naive_utc();

    let new_session = NewSession {
        id: session_id,
        user_id: user.id,
        token_hash,
        expires_at,
    };

    db.run(move |connection| {
        diesel::insert_into(sessions::table)
            .values(&new_session)
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie = Cookie::build(("session", raw_token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build();

    Ok((
        cookie,
        Json(LoginResponse {
            message: "Login successful".to_string(),
        }),
    ))
}
```

---

# Step 13 — Mount the login route

Your `main.rs` should now contain:

```rust
mod auth;
mod models;
mod schema;

use rocket::launch;

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register,
                auth::routes::login,
            ],
        )
}
```

And:

```text
src/auth/mod.rs
```

should be:

```rust
pub mod password;
pub mod routes;
pub mod session;
```

---

# Step 14 — Test registration

Start Rocket:

```bash
cargo run
```

Register:

```http
POST /auth/register
Content-Type: application/json
```

Body:

```json
{
    "email": "test@example.com",
    "password": "MyStrongPassword123!"
}
```

You should receive:

```json
{
    "id": "UUID",
    "email": "test@example.com"
}
```

---

# Step 15 — Test login

Now:

```http
POST /auth/login
Content-Type: application/json
```

Body:

```json
{
    "email": "test@example.com",
    "password": "MyStrongPassword123!"
}
```

Response:

```json
{
    "message": "Login successful"
}
```

Your browser should receive:

```text
Set-Cookie:
session=xxxxxxxxxxxxxxxx;
HttpOnly;
Secure;
SameSite=Lax;
Path=/
```

And PostgreSQL should contain something like:

```text
sessions
------------------------------------------------
id                  UUID
user_id             UUID
token_hash          64-character SHA256 hash
expires_at          timestamp
created_at          timestamp
last_used_at        timestamp
revoked_at          NULL
```

---

# Important: `Secure` during local development

There's one issue you'll probably hit immediately.

This:

```rust
.secure(true)
```

means the browser only sends the cookie over HTTPS.

If you're testing:

```text
http://localhost:8000
```

your browser may not send the cookie.

For local development you can temporarily use:

```rust
let cookie = Cookie::build(("session", raw_token))
    .http_only(true)
    .secure(false)
    .same_site(SameSite::Lax)
    .path("/")
    .build();
```

Then turn it back on in production:

```rust
.secure(true)
```

A cleaner approach later is to make this environment-dependent.

---

# Step 16 — What we've built

At this point your authentication flow is:

```text
                  REGISTER
                     │
                     ▼
              ┌─────────────┐
              │    users    │
              └─────────────┘
                     │
                     │
LOGIN                │
 │                   │
 ▼                   ▼
email/password ──► verify Argon2
                     │
                     ▼
              generate token
                     │
              ┌──────┴──────┐
              ▼             ▼
        raw token       SHA-256 hash
              │             │
              ▼             ▼
           Cookie        sessions
              │             │
              └──────┬──────┘
                     ▼
                authenticated
```

### Next step: the important Rocket part

Now we need to make this:

```rust
#[get("/profile")]
async fn profile(user: AuthenticatedUser) {
    ...
}
```

work automatically.

We'll create an `AuthenticatedUser` implementing Rocket's `FromRequest` trait. It will:

1. Read the `session` cookie.
2. Hash the token.
3. Find the session.
4. Check expiration.
5. Check `revoked_at`.
6. Load the user.
7. Give the route the authenticated user.

That means every protected route can simply use:

```rust
user: AuthenticatedUser
```

instead of manually checking sessions every time.

Absolutely. Now we’ll build the **Rocket authentication guard**. This is the piece that turns your session system into something you can actually use on protected endpoints.

# Step 17 — Create `AuthenticatedUser`

The goal is to make this possible:

```rust
#[get("/profile")]
pub async fn profile(user: AuthenticatedUser) -> Json<UserResponse> {
    // user is authenticated
}
```

Rocket will automatically validate the session before your route runs.

---

## 1. Create `src/auth/guards.rs`

```rust
use chrono::Utc;
use diesel::prelude::*;
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};
use uuid::Uuid;

use crate::{
    auth::session::hash_token,
    models::user::User,
    schema::{sessions, users},
    DbConn,
};

pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
}
```

---

## 2. Implement Rocket's `FromRequest`

Add this underneath the struct:

```rust
#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {

        let cookies = request.cookies();

        let cookie = match cookies.get_private("session") {
            Some(cookie) => cookie,
            None => {
                return Outcome::Error((Status::Unauthorized, ()));
            }
        };

        let raw_token = cookie.value().to_string();

        let token_hash = hash_token(&raw_token);

        let db = match request.rocket().state::<DbConn>() {
            Some(db) => db,
            None => {
                return Outcome::Error((Status::InternalServerError, ()));
            }
        };

        let result = db
            .run(move |connection| {
                sessions::table
                    .inner_join(
                        users::table
                            .on(users::id.eq(sessions::user_id))
                    )
                    .filter(sessions::token_hash.eq(token_hash))
                    .filter(sessions::revoked_at.is_null())
                    .select((
                        users::id,
                        users::email,
                        sessions::expires_at,
                    ))
                    .first::<(Uuid, String, chrono::NaiveDateTime)>(
                        connection,
                    )
            })
            .await;

        match result {
            Ok((user_id, email, expires_at)) => {

                if expires_at < Utc::now().naive_utc() {
                    return Outcome::Error((
                        Status::Unauthorized,
                        (),
                    ));
                }

                Outcome::Success(AuthenticatedUser {
                    id: user_id,
                    email,
                })
            }

            Err(_) => {
                Outcome::Error((
                    Status::Unauthorized,
                    (),
                ))
            }
        }
    }
}
```

### Important

This version assumes your Rocket database connection type is available as:

```rust
DbConn
```

from your `main.rs`.

If your current `DbConn` is declared differently, keep your existing definition and we'll adapt this guard to it.

---

# Step 18 — Export the guard

Open:

```text
src/auth/mod.rs
```

Change it to:

```rust
pub mod guards;
pub mod password;
pub mod routes;
pub mod session;
```

---

# Step 19 — Add a protected route

Open:

```text
src/auth/routes.rs
```

Add:

```rust
use crate::auth::guards::AuthenticatedUser;
```

Then:

```rust
#[derive(Serialize)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub email: String,
}
```

And:

```rust
#[get("/profile")]
pub async fn profile(
    user: AuthenticatedUser,
) -> Json<ProfileResponse> {

    Json(ProfileResponse {
        id: user.id,
        email: user.email,
    })
}
```

---

# Step 20 — Mount the protected route

Update `main.rs`:

```rust
#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register,
                auth::routes::login,
                auth::routes::profile,
            ],
        )
}
```

Now you have:

```text
POST /auth/register
POST /auth/login
GET  /auth/profile
```

---

# Step 21 — Test without authentication

Call:

```http
GET http://localhost:8000/auth/profile
```

Without a session cookie, you should get:

```text
401 Unauthorized
```

That's exactly what we want.

---

# Step 22 — Test after login

First login:

```http
POST /auth/login
Content-Type: application/json

{
    "email": "test@example.com",
    "password": "MyStrongPassword123!"
}
```

The server gives you the session cookie.

Then:

```http
GET /auth/profile
Cookie: session=YOUR_SESSION_TOKEN
```

You should receive:

```json
{
    "id": "your-user-uuid",
    "email": "test@example.com"
}
```

The flow is now:

```text
                  HTTP REQUEST
                       │
                       ▼
              ┌─────────────────┐
              │ Rocket Route    │
              └────────┬────────┘
                       │
                       ▼
             AuthenticatedUser
                       │
                       ▼
                session cookie
                       │
                       ▼
                 hash token
                       │
                       ▼
                 sessions DB
                       │
              ┌────────┴────────┐
              │                 │
           invalid            valid
              │                 │
              ▼                 ▼
          401 error         users table
                                │
                                ▼
                         AuthenticatedUser
                                │
                                ▼
                         protected route
```

---

# One correction before we continue

There's an important issue in the login code from the previous step.

We currently used:

```rust
Cookie::build(("session", raw_token))
```

but the guard reads:

```rust
request.cookies().get_private("session")
```

Those are **different cookie mechanisms**.

For now, change the guard to:

```rust
let cookie = match cookies.get("session") {
    Some(cookie) => cookie,
    None => {
        return Outcome::Error((Status::Unauthorized, ()));
    }
};
```

So this:

```rust
let cookie = match cookies.get_private("session") {
```

becomes:

```rust
let cookie = match cookies.get("session") {
```

We'll improve the cookie security shortly.

---

## What we've completed

```text
✅ User registration
✅ Argon2 password hashing
✅ Login
✅ Session generation
✅ Session database
✅ Session cookie
✅ Session expiration
✅ Session revocation support
✅ AuthenticatedUser guard
✅ Protected routes
```

### Next step

Next we'll implement **logout and session revocation**:

```text
POST /auth/logout
```

and then:

```text
POST /auth/logout-all
```

The second one will let a user invalidate **every active login/session** across all devices.

Great. Now we'll implement **logout + session revocation**. This is important because simply deleting the browser cookie is not enough: the server-side session should also become invalid.

## Step 23 — Add logout

We'll implement:

```text
POST /auth/logout
```

Flow:

```text
Cookie
   │
   ▼
session token
   │
   ▼
hash token
   │
   ▼
find session
   │
   ▼
set revoked_at
   │
   ▼
delete cookie
```

### 1. Add logout to `src/auth/routes.rs`

Add:

```rust
use chrono::Utc;
use diesel::prelude::*;
use rocket::http::{Cookie, SameSite, Status};

use crate::{
    auth::session::hash_token,
    schema::sessions,
};
```

Then add:

```rust
#[post("/logout")]
pub async fn logout(
    mut db: DbConn,
    cookies: &rocket::http::CookieJar<'_>,
) -> Status {

    let cookie = match cookies.get("session") {
        Some(cookie) => cookie,
        None => {
            return Status::NoContent;
        }
    };

    let raw_token = cookie.value().to_string();
    let token_hash = hash_token(&raw_token);

    let result = db
        .run(move |connection| {
            diesel::update(
                sessions::table
                    .filter(sessions::token_hash.eq(token_hash))
                    .filter(sessions::revoked_at.is_null())
            )
            .set(
                sessions::revoked_at.eq(
                    Utc::now().naive_utc()
                )
            )
            .execute(connection)
        })
        .await;

    cookies.remove(
        Cookie::build(("session", ""))
            .path("/")
            .build()
    );

    match result {
        Ok(_) => Status::NoContent,
        Err(_) => Status::InternalServerError,
    }
}
```

---

# Step 24 — Mount logout

Update `main.rs`:

```rust
#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register,
                auth::routes::login,
                auth::routes::profile,
                auth::routes::logout,
            ],
        )
}
```

Now you have:

```text
POST /auth/register
POST /auth/login
GET  /auth/profile
POST /auth/logout
```

---

# Step 25 — Test logout

Login first:

```http
POST /auth/login
Content-Type: application/json

{
    "email": "test@example.com",
    "password": "MyStrongPassword123!"
}
```

Then:

```http
GET /auth/profile
```

It should work.

Now:

```http
POST /auth/logout
```

You should receive:

```text
204 No Content
```

Try:

```http
GET /auth/profile
```

again.

You should now get:

```text
401 Unauthorized
```

Even if you manually reuse the old session token, the database contains:

```text
revoked_at
---------------------------
2026-09-18 09:xx:xx
```

so the `AuthenticatedUser` guard rejects it.

---

# Step 26 — Logout all sessions

This is extremely useful.

Imagine the user is logged in from:

```text
Chrome
Laptop
Phone
Tablet
```

The database might contain:

```text
sessions

id       user_id    revoked_at
----------------------------------------
AAA      USER-1     NULL
BBB      USER-1     NULL
CCC      USER-1     NULL
DDD      USER-1     NULL
```

We want:

```text
POST /auth/logout-all
```

to invalidate every session belonging to that user.

The route will use our existing `AuthenticatedUser` guard.

Add:

```rust
use crate::auth::guards::AuthenticatedUser;
```

Then:

```rust
#[post("/logout-all")]
pub async fn logout_all(
    mut db: DbConn,
    user: AuthenticatedUser,
    cookies: &rocket::http::CookieJar<'_>,
) -> Status {

    let user_id = user.id;

    let result = db
        .run(move |connection| {
            diesel::update(
                sessions::table
                    .filter(sessions::user_id.eq(user_id))
                    .filter(sessions::revoked_at.is_null())
            )
            .set(
                sessions::revoked_at.eq(
                    Utc::now().naive_utc()
                )
            )
            .execute(connection)
        })
        .await;

    cookies.remove(
        Cookie::build(("session", ""))
            .path("/")
            .build()
    );

    match result {
        Ok(_) => Status::NoContent,
        Err(_) => Status::InternalServerError,
    }
}
```

---

# Step 27 — Mount it

Add:

```rust
auth::routes::logout_all,
```

So:

```rust
#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register,
                auth::routes::login,
                auth::routes::profile,
                auth::routes::logout,
                auth::routes::logout_all,
            ],
        )
}
```

---

# Step 28 — Your authentication system now looks like this

```text
                 ┌───────────────┐
                 │     User      │
                 └───────┬───────┘
                         │
                 email + password
                         │
                         ▼
                 ┌───────────────┐
                 │    Argon2     │
                 └───────┬───────┘
                         │
                    password OK
                         │
                         ▼
                 ┌───────────────┐
                 │ Create Token  │
                 └───────┬───────┘
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
       HttpOnly Cookie          SHA-256
                                     │
                                     ▼
                               sessions table
                                     │
                                     ▼
                              AuthenticatedUser
                                     │
                                     ▼
                              Protected routes
```

Logout:

```text
POST /auth/logout
        │
        ▼
hash cookie
        │
        ▼
find session
        │
        ▼
revoked_at = NOW()
        │
        ▼
delete cookie
```

Logout all:

```text
POST /auth/logout-all
        │
        ▼
AuthenticatedUser
        │
        ▼
user_id
        │
        ▼
revoke ALL sessions
```

## One security improvement

Before we add 2FA, I recommend we fix the cookie handling so production uses:

```text
HttpOnly
Secure
SameSite
Path
```

and make the session expiration/rotation behavior cleaner.

Then we'll move to the fun part:

```text
STEP 29
   ↓
TOTP 2FA
   ↓
Generate secret
   ↓
otpauth URI
   ↓
Authenticator app
   ↓
Verify 6-digit code
   ↓
Enable 2FA
```

After that we'll modify login so **password verification alone no longer creates a fully authenticated session when 2FA is enabled**.

Let's do **Step 29: TOTP 2FA setup**. We'll make the user enroll an authenticator app first, then we'll modify login afterward.

## 1. Add the TOTP dependency

In `Cargo.toml` add:

```toml
totp-rs = { version = "5", features = ["otpauth"] }
```

Then:

```bash
cargo check
```

---

# Step 29 — Add the 2FA table

Create:

```text
migrations/
└── 00000000000003_create_two_factor/
    ├── up.sql
    └── down.sql
```

### `up.sql`

```sql
CREATE TABLE two_factor_secrets (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    secret VARCHAR(255) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    confirmed_at TIMESTAMP NULL
);
```

### `down.sql`

```sql
DROP TABLE two_factor_secrets;
```

Run:

```bash
diesel migration run
```

Then regenerate:

```bash
diesel print-schema > src/schema.rs
```

You should have:

```rust
diesel::table! {
    two_factor_secrets (id) {
        id -> Uuid,
        user_id -> Uuid,
        secret -> Varchar,
        created_at -> Timestamp,
        confirmed_at -> Nullable<Timestamp>,
    }
}
```

---

# Step 30 — Create the model

Create:

```text
src/models/two_factor.rs
```

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
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
```

Update:

```text
src/models/mod.rs
```

to:

```rust
pub mod session;
pub mod two_factor;
pub mod user;
```

---

# Step 31 — Create the TOTP module

Create:

```text
src/auth/totp.rs
```

```rust
use totp_rs::{Algorithm, Secret, TOTP};

pub fn generate_totp() -> Result<TOTP, String> {
    let secret = Secret::generate_secret();

    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_bytes()
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
```

Then update:

```text
src/auth/mod.rs
```

```rust
pub mod guards;
pub mod password;
pub mod routes;
pub mod session;
pub mod totp;
```

---

# Step 32 — Add 2FA setup endpoint

We want:

```text
POST /auth/2fa/setup
```

The flow is:

```text
Authenticated user
       │
       ▼
Generate TOTP secret
       │
       ▼
Save secret
       │
       ▼
Return setup information
       │
       ▼
Authenticator app
```

In `routes.rs`, add:

```rust
use crate::{
    auth::{
        guards::AuthenticatedUser,
        totp::generate_totp,
    },
    models::two_factor::NewTwoFactorSecret,
    schema::two_factor_secrets,
};
```

Then:

```rust
#[derive(Serialize)]
pub struct TwoFactorSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
}
```

Now the route:

```rust
#[post("/2fa/setup")]
pub async fn setup_2fa(
    mut db: DbConn,
    user: AuthenticatedUser,
) -> Result<Json<TwoFactorSetupResponse>, String> {

    let totp = generate_totp()?;

    let secret = totp
        .get_secret_base32()
        .to_string();

    let otpauth_url = totp
        .get_url();

    let new_secret = NewTwoFactorSecret {
        id: Uuid::new_v4(),
        user_id: user.id,
        secret: secret.clone(),
    };

    db.run(move |connection| {
        diesel::insert_into(two_factor_secrets::table)
            .values(&new_secret)
            .on_conflict(two_factor_secrets::user_id)
            .do_update()
            .set(two_factor_secrets::secret.eq(&secret))
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Json(TwoFactorSetupResponse {
        secret,
        otpauth_url,
    }))
}
```

---

# Step 33 — Mount the route

Add:

```rust
auth::routes::setup_2fa,
```

Your mount becomes:

```rust
#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register,
                auth::routes::login,
                auth::routes::profile,
                auth::routes::logout,
                auth::routes::logout_all,
                auth::routes::setup_2fa,
            ],
        )
}
```

---

# Step 34 — Test it

First log in.

Then:

```http
POST /auth/2fa/setup
```

Because this route requires:

```rust
user: AuthenticatedUser
```

the session cookie must be present.

You should get something like:

```json
{
    "secret": "JBSWY3DPEHPK3PXP",
    "otpauth_url": "otpauth://totp/..."
}
```

The `otpauth_url` can be used by an authenticator application to configure the account.

The secret is sensitive. **Don't log it in production or expose it after enrollment is complete.**

---

# Step 35 — Confirm the 2FA setup

We don't want to enable 2FA merely because someone called `/2fa/setup`.

The user needs to prove that their authenticator works.

Create:

```text
POST /auth/2fa/confirm
```

Add:

```rust
#[derive(Deserialize)]
pub struct TwoFactorCodeRequest {
    pub code: String,
}
```

Then:

```rust
#[post("/2fa/confirm", data = "<request>")]
pub async fn confirm_2fa(
    mut db: DbConn,
    user: AuthenticatedUser,
    request: Json<TwoFactorCodeRequest>,
) -> Result<Json<LoginResponse>, String> {

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

    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .as_bytes()
            .to_vec(),
    )
    .map_err(|e| e.to_string())?;
```

For this part, **don't use the raw Base32 string directly as though it were arbitrary bytes**. `totp-rs` expects the secret in its appropriate decoded/generated form. The exact constructor varies slightly by `totp-rs` version, so let's check the compiler/API after adding the dependency rather than forcing a potentially version-mismatched implementation.

Run:

```bash
cargo check
```

If you get an error around:

```rust
TOTP::new(...)
```

paste that compiler error to me. We'll fix it against the exact `totp-rs` version Cargo resolved.

---

## The architecture we're building

After this step:

```text
                     USER
                      │
                      ▼
              POST /2fa/setup
                      │
                      ▼
              Generate secret
                      │
                      ▼
               PostgreSQL
                      │
                      ▼
               otpauth://...
                      │
                      ▼
             Authenticator App
                      │
                 6-digit code
                      │
                      ▼
             POST /2fa/confirm
                      │
                      ▼
                Verify TOTP
                      │
                      ▼
             Enable 2FA = TRUE
```

And **only after confirmation** should:

```text
users.two_factor_enabled
```

become:

```text
TRUE
```

### One important security improvement

We're currently storing:

```text
two_factor_secrets.secret
```

as plaintext. For a production system, that secret should be **encrypted at rest**, because unlike passwords, TOTP secrets need to be recoverable by the server.

For now, keeping it this way makes the learning implementation much easier. We'll harden it after the complete flow works.

**Next:** finish `/2fa/confirm`, set `two_factor_enabled = true`, and then modify `/auth/login` so a 2FA-enabled account gets a temporary authentication state instead of a fully authenticated session.

Now let's finish **2FA confirmation** properly, then we'll change login so a user with 2FA enabled must provide the TOTP code.

One important correction from the previous step: don't construct the TOTP object from the Base32 string manually. `totp-rs` provides `Secret::Encoded` for this.

## Step 36 — Finish `/auth/2fa/confirm`

### 1. Update `src/auth/totp.rs`

Replace the previous helper with:

```rust
use totp_rs::{Algorithm, Secret, TOTP};

pub fn generate_totp() -> Result<(TOTP, String), String> {
    let secret = Secret::generate_secret();

    let secret_base32 = secret.to_encoded().to_string();

    let secret_bytes = secret
        .to_bytes()
        .map_err(|error| error.to_string())?;

    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
    )
    .map_err(|error| error.to_string())?;

    Ok((totp, secret_base32))
}

pub fn create_totp_from_secret(
    secret: &str,
) -> Result<TOTP, String> {

    let secret = Secret::Encoded(
        secret.to_string()
    );

    let secret_bytes = secret
        .to_bytes()
        .map_err(|error| error.to_string())?;

    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
    )
    .map_err(|error| error.to_string())
}
```

Then your setup route should use:

```rust
let (totp, secret) = generate_totp()?;

let otpauth_url = totp
    .get_url();
```

---

# Step 37 — Update the setup route

Your `/2fa/setup` should now look like:

```rust
#[post("/2fa/setup")]
pub async fn setup_2fa(
    mut db: DbConn,
    user: AuthenticatedUser,
) -> Result<Json<TwoFactorSetupResponse>, String> {

    let (totp, secret) = generate_totp()?;

    let otpauth_url = totp.get_url();

    let new_secret = NewTwoFactorSecret {
        id: Uuid::new_v4(),
        user_id: user.id,
        secret: secret.clone(),
    };

    db.run(move |connection| {
        diesel::insert_into(two_factor_secrets::table)
            .values(&new_secret)
            .on_conflict(two_factor_secrets::user_id)
            .do_update()
            .set(
                two_factor_secrets::secret
                    .eq(&secret)
            )
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Json(TwoFactorSetupResponse {
        secret,
        otpauth_url,
    }))
}
```

---

# Step 38 — Implement confirmation

Add this import:

```rust
use crate::auth::totp::create_totp_from_secret;
```

Then:

```rust
#[post("/2fa/confirm", data = "<request>")]
pub async fn confirm_2fa(
    mut db: DbConn,
    user: AuthenticatedUser,
    request: Json<TwoFactorCodeRequest>,
) -> Result<Json<LoginResponse>, String> {

    let user_id = user.id;

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(
                    two_factor_secrets::user_id
                        .eq(user_id)
                )
                .select(
                    two_factor_secrets::secret
                )
                .first::<String>(connection)
        })
        .await
        .map_err(|_| {
            "2FA setup not found".to_string()
        })?;

    let totp = create_totp_from_secret(&secret)?;

    let valid = totp
        .check(&request.code)
        .map_err(|error| error.to_string())?;

    if !valid {
        return Err(
            "Invalid authentication code".to_string()
        );
    }

    db.run(move |connection| {
        diesel::update(
            users::table
                .filter(users::id.eq(user_id))
        )
        .set(
            users::two_factor_enabled.eq(true)
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Json(LoginResponse {
        message: "Two-factor authentication enabled"
            .to_string(),
    }))
}
```

---

# Step 39 — Mount the confirmation route

Your `main.rs` should now include:

```rust
auth::routes::setup_2fa,
auth::routes::confirm_2fa,
```

So:

```rust
#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/auth",
            routes![
                auth::routes::register,
                auth::routes::login,
                auth::routes::profile,
                auth::routes::logout,
                auth::routes::logout_all,
                auth::routes::setup_2fa,
                auth::routes::confirm_2fa,
            ],
        )
}
```

---

# Step 40 — Test 2FA enrollment

First log in normally.

Then:

```http
POST /auth/2fa/setup
```

You'll receive:

```json
{
    "secret": "BASE32_SECRET",
    "otpauth_url": "otpauth://totp/..."
}
```

Put the `otpauth_url` into your authenticator application.

Your authenticator will generate something like:

```text
482913
```

Then:

```http
POST /auth/2fa/confirm
Content-Type: application/json
```

```json
{
    "code": "482913"
}
```

If correct:

```json
{
    "message": "Two-factor authentication enabled"
}
```

And the database changes from:

```text
two_factor_enabled
------------------
false
```

to:

```text
two_factor_enabled
------------------
true
```

---

# Step 41 — Now fix the login flow

This is the important part.

Currently our login does:

```text
password correct
       ↓
create session
       ↓
logged in
```

That's wrong when 2FA is enabled.

We need:

```text
password correct
       │
       ▼
2FA enabled?
   │          │
  NO         YES
   │          │
   ▼          ▼
session    temporary
           2FA state
              │
              ▼
       POST /auth/2fa/verify
              │
              ▼
          TOTP valid
              │
              ▼
           session
```

## Step 42 — Add a temporary 2FA challenge table

Create:

```text
migrations/
└── 00000000000004_create_two_factor_challenges/
    ├── up.sql
    └── down.sql
```

### `up.sql`

```sql
CREATE TABLE two_factor_challenges (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX two_factor_challenges_user_id_idx
ON two_factor_challenges(user_id);
```

### `down.sql`

```sql
DROP TABLE two_factor_challenges;
```

Run:

```bash
diesel migration run
```

Then:

```bash
diesel print-schema > src/schema.rs
```

---

# Step 43 — Why do we need a challenge?

Suppose someone knows your password but doesn't have your authenticator.

We **must not** give them a normal session.

Instead:

```text
Password ✓
     │
     ▼
Temporary challenge
     │
     │ expires quickly
     ▼
TOTP verification
     │
     ├── invalid → reject
     │
     └── valid → real session
```

This is the same basic security model used by many modern authentication systems.

---

## Next step

Next we'll implement:

```text
POST /auth/login
```

with two possible responses:

```json
{
    "message": "Login successful"
}
```

or:

```json
{
    "message": "Two-factor authentication required",
    "requires_2fa": true,
    "challenge_token": "..."
}
```

Then we'll add:

```text
POST /auth/2fa/verify
```

which converts that temporary challenge into a real authenticated session.

Let's implement the **temporary 2FA challenge** and modify login. This is the key transition from password authentication to full authentication.

## Step 44 — Create the challenge model

Create:

```text
src/models/two_factor_challenge.rs
```

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::two_factor_challenges;

#[derive(Queryable)]
pub struct TwoFactorChallenge {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = two_factor_challenges)]
pub struct NewTwoFactorChallenge {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
}
```

Update:

```text
src/models/mod.rs
```

```rust
pub mod session;
pub mod two_factor;
pub mod two_factor_challenge;
pub mod user;
```

---

# Step 45 — Add challenge helpers

Open:

```text
src/auth/session.rs
```

Add:

```rust
pub fn generate_challenge_token() -> String {
    generate_token()
}
```

We can use the same cryptographically secure random-token generation.

---

# Step 46 — Change the login response

In `routes.rs`, replace the old login response with:

```rust
#[derive(Serialize)]
pub struct LoginResponse {
    pub message: String,
    pub requires_2fa: bool,
    pub challenge_token: Option<String>,
}
```

Now a normal login can return:

```json
{
    "message": "Login successful",
    "requires_2fa": false,
    "challenge_token": null
}
```

A 2FA login will return:

```json
{
    "message": "Two-factor authentication required",
    "requires_2fa": true,
    "challenge_token": "..."
}
```

---

# Step 47 — Modify `/auth/login`

Replace your current login implementation with:

```rust
#[post("/login", data = "<request>")]
pub async fn login(
    mut db: DbConn,
    request: Json<LoginRequest>,
) -> Result<(Option<Cookie<'static>>, Json<LoginResponse>), String> {

    let email = request.email.to_lowercase();
    let password = request.password.clone();

    let user: User = db
        .run(move |connection| {
            users::table
                .filter(users::email.eq(&email))
                .first::<User>(connection)
        })
        .await
        .map_err(|_| "Invalid email or password".to_string())?;

    let password_valid =
        verify_password(
            &password,
            &user.password_hash,
        )?;

    if !password_valid {
        return Err(
            "Invalid email or password".to_string()
        );
    }

    // ------------------------------------
    // 2FA required
    // ------------------------------------

    if user.two_factor_enabled {

        let raw_challenge =
            generate_challenge_token();

        let challenge_hash =
            hash_token(&raw_challenge);

        let challenge_id =
            Uuid::new_v4();

        let expires_at =
            (Utc::now()
                + Duration::minutes(5))
                .naive_utc();

        let challenge =
            NewTwoFactorChallenge {
                id: challenge_id,
                user_id: user.id,
                token_hash: challenge_hash,
                expires_at,
            };

        db.run(move |connection| {
            diesel::insert_into(
                two_factor_challenges::table
            )
            .values(&challenge)
            .execute(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

        return Ok((
            None,
            Json(LoginResponse {
                message:
                    "Two-factor authentication required"
                        .to_string(),

                requires_2fa: true,

                challenge_token:
                    Some(raw_challenge),
            }),
        ));
    }

    // ------------------------------------
    // Normal login
    // ------------------------------------

    let raw_token =
        generate_token();

    let token_hash =
        hash_token(&raw_token);

    let session_id =
        Uuid::new_v4();

    let expires_at =
        (Utc::now()
            + Duration::days(7))
            .naive_utc();

    let new_session =
        NewSession {
            id: session_id,
            user_id: user.id,
            token_hash,
            expires_at,
        };

    db.run(move |connection| {
        diesel::insert_into(
            sessions::table
        )
        .values(&new_session)
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie =
        Cookie::build(
            ("session", raw_token)
        )
        .http_only(true)
        .secure(false)
        .same_site(SameSite::Lax)
        .path("/")
        .build();

    Ok((
        Some(cookie),
        Json(LoginResponse {
            message:
                "Login successful".to_string(),

            requires_2fa: false,

            challenge_token: None,
        }),
    ))
}
```

---

# Step 48 — Add the imports

At the top of `routes.rs`, make sure you have:

```rust
use chrono::{Duration, Utc};
use diesel::prelude::*;
use rocket::http::{Cookie, SameSite};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{
        password::verify_password,
        session::{
            generate_challenge_token,
            generate_token,
            hash_token,
        },
    },
    models::{
        session::NewSession,
        two_factor_challenge::NewTwoFactorChallenge,
        user::User,
    },
    schema::{
        sessions,
        two_factor_challenges,
        users,
    },
    DbConn,
};
```

You may already have some of these imports, so don't duplicate them.

---

# Step 49 — Why the response is different

Without 2FA:

```text
POST /auth/login
       │
       ▼
password ✓
       │
       ▼
create session
       │
       ▼
Set-Cookie
       │
       ▼
authenticated
```

With 2FA:

```text
POST /auth/login
       │
       ▼
password ✓
       │
       ▼
two_factor_enabled = true
       │
       ▼
create challenge
       │
       ▼
return challenge token
       │
       ▼
NO session cookie
```

That last part is extremely important:

**The client does not receive an authenticated session yet.**

---

# Step 50 — Implement `/auth/2fa/verify`

Now create the endpoint that turns the temporary challenge into a real session.

Add:

```rust
#[derive(Deserialize)]
pub struct TwoFactorVerifyRequest {
    pub challenge_token: String,
    pub code: String,
}
```

Then:

```rust
#[post("/2fa/verify", data = "<request>")]
pub async fn verify_2fa(
    mut db: DbConn,
    request: Json<TwoFactorVerifyRequest>,
) -> Result<(Cookie<'static>, Json<LoginResponse>), String> {

    let challenge_hash =
        hash_token(&request.challenge_token);

    let challenge = db
        .run(move |connection| {
            two_factor_challenges::table
                .filter(
                    two_factor_challenges::token_hash
                        .eq(challenge_hash)
                )
                .first::<TwoFactorChallenge>(
                    connection,
                )
        })
        .await
        .map_err(|_| {
            "Invalid or expired challenge"
                .to_string()
        })?;

    if challenge.expires_at
        < Utc::now().naive_utc()
    {
        return Err(
            "Challenge has expired".to_string()
        );
    }

    let user_id = challenge.user_id;

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(
                    two_factor_secrets::user_id
                        .eq(user_id)
                )
                .select(
                    two_factor_secrets::secret
                )
                .first::<String>(connection)
        })
        .await
        .map_err(|_| {
            "2FA configuration not found"
                .to_string()
        })?;

    let totp =
        create_totp_from_secret(&secret)?;

    let valid = totp
        .check(&request.code)
        .map_err(|error| error.to_string())?;

    if !valid {
        return Err(
            "Invalid authentication code"
                .to_string()
        );
    }

    // Create the real session.
    let raw_token =
        generate_token();

    let token_hash =
        hash_token(&raw_token);

    let session_id =
        Uuid::new_v4();

    let expires_at =
        (Utc::now()
            + Duration::days(7))
            .naive_utc();

    let new_session =
        NewSession {
            id: session_id,
            user_id,
            token_hash,
            expires_at,
        };

    db.run(move |connection| {
        diesel::insert_into(
            sessions::table
        )
        .values(&new_session)
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    // Consume the challenge.
    db.run(move |connection| {
        diesel::delete(
            two_factor_challenges::table
                .filter(
                    two_factor_challenges::id
                        .eq(challenge.id)
                )
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie =
        Cookie::build(
            ("session", raw_token)
        )
        .http_only(true)
        .secure(false)
        .same_site(SameSite::Lax)
        .path("/")
        .build();

    Ok((
        cookie,
        Json(LoginResponse {
            message:
                "Login successful".to_string(),

            requires_2fa: false,

            challenge_token: None,
        }),
    ))
}
```

You'll need these imports if they aren't already present:

```rust
use crate::{
    auth::totp::create_totp_from_secret,
    models::two_factor_challenge::TwoFactorChallenge,
    schema::two_factor_secrets,
};
```

---

# Step 51 — Mount the route

In `main.rs`:

```rust
auth::routes::verify_2fa,
```

So your authentication routes now include:

```rust
routes![
    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
]
```

---

# Step 52 — Complete login flow

You now have:

### Normal user

```text
POST /auth/login
       │
       ▼
password ✓
       │
       ▼
2FA disabled
       │
       ▼
session created
       │
       ▼
session cookie
```

### 2FA user

```text
POST /auth/login
       │
       ▼
password ✓
       │
       ▼
2FA enabled
       │
       ▼
5-minute challenge
       │
       ▼
challenge_token
       │
       ▼
POST /auth/2fa/verify
       │
       ▼
TOTP ✓
       │
       ▼
challenge deleted
       │
       ▼
session created
       │
       ▼
session cookie
```

This is the authentication boundary we wanted.

---

## One thing to fix before testing

The code above uses:

```rust
.first::<TwoFactorChallenge>(connection)
```

so add:

```rust
use crate::models::two_factor_challenge::TwoFactorChallenge;
```

And make sure your `schema.rs` actually contains:

```rust
two_factor_challenges
```

If Diesel reports a type mismatch or a `totp-rs` API error, **don't work around it manually**. Paste the exact `cargo check` error, because those errors depend on the exact crate versions Cargo resolved.

### Next step

After this compiles, we'll add **recovery codes** and then harden the authentication system with:

* one-time recovery codes
* disable 2FA
* session rotation
* session expiration cleanup
* secure production cookies
* login rate limiting
* password-change session invalidation.

Yep. Next we're adding **2FA recovery codes**. These are the backup codes a user can use if they lose access to their authenticator app.

We'll make each code **single-use** and store only its hash.

# Step 53 — Create recovery-code table

Create:

```text
migrations/
└── 00000000000005_create_recovery_codes/
    ├── up.sql
    └── down.sql
```

### `up.sql`

```sql
CREATE TABLE recovery_codes (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash VARCHAR(255) NOT NULL UNIQUE,
    used_at TIMESTAMP NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX recovery_codes_user_id_idx
ON recovery_codes(user_id);
```

### `down.sql`

```sql
DROP TABLE recovery_codes;
```

Run:

```bash
diesel migration run
```

Then:

```bash
diesel print-schema > src/schema.rs
```

---

# Step 54 — Create the model

Create:

```text
src/models/recovery_code.rs
```

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::recovery_codes;

#[derive(Queryable)]
pub struct RecoveryCode {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code_hash: String,
    pub used_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = recovery_codes)]
pub struct NewRecoveryCode {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code_hash: String,
}
```

Update:

```text
src/models/mod.rs
```

```rust
pub mod recovery_code;
pub mod session;
pub mod two_factor;
pub mod two_factor_challenge;
pub mod user;
```

---

# Step 55 — Generate recovery codes

Create:

```text
src/auth/recovery.rs
```

```rust
use rand::{rngs::OsRng, RngCore};

use crate::auth::session::hash_token;

pub fn generate_recovery_code() -> String {
    let mut bytes = [0u8; 8];

    OsRng.fill_bytes(&mut bytes);

    let value = u64::from_be_bytes(bytes);

    format!("{:016X}", value)
}

pub fn hash_recovery_code(code: &str) -> String {
    hash_token(code)
}
```

This generates codes like:

```text
7A31D92F81C4B6E0
```

We'll generate multiple codes for each user.

---

# Step 56 — Generate codes when 2FA is confirmed

When this succeeds:

```text
POST /auth/2fa/confirm
```

we want:

```text
TOTP valid
   │
   ▼
enable 2FA
   │
   ▼
generate recovery codes
   │
   ▼
store hashes
   │
   ▼
return raw codes ONCE
```

Add imports to `routes.rs`:

```rust
use crate::{
    auth::recovery::{
        generate_recovery_code,
        hash_recovery_code,
    },
    models::recovery_code::NewRecoveryCode,
    schema::recovery_codes,
};
```

---

# Step 57 — Change the confirmation response

Create:

```rust
#[derive(Serialize)]
pub struct TwoFactorConfirmResponse {
    pub message: String,
    pub recovery_codes: Vec<String>,
}
```

Then after successfully verifying the TOTP code:

```rust
let mut recovery_codes_plain = Vec::new();
let mut recovery_code_rows = Vec::new();

for _ in 0..10 {
    let code = generate_recovery_code();
    let code_hash = hash_recovery_code(&code);

    recovery_codes_plain.push(code);

    recovery_code_rows.push(
        NewRecoveryCode {
            id: Uuid::new_v4(),
            user_id,
            code_hash,
        }
    );
}
```

Before inserting the new codes, delete any old ones:

```rust
db.run(move |connection| {
    diesel::delete(
        recovery_codes::table
            .filter(
                recovery_codes::user_id
                    .eq(user_id)
            )
    )
    .execute(connection)
})
.await
.map_err(|error| error.to_string())?;
```

Then insert the new ones:

```rust
db.run(move |connection| {
    diesel::insert_into(
        recovery_codes::table
    )
    .values(&recovery_code_rows)
    .execute(connection)
})
.await
.map_err(|error| error.to_string())?;
```

Then enable 2FA:

```rust
db.run(move |connection| {
    diesel::update(
        users::table
            .filter(users::id.eq(user_id))
    )
    .set(
        users::two_factor_enabled.eq(true)
    )
    .execute(connection)
})
.await
.map_err(|error| error.to_string())?;
```

Finally return:

```rust
Ok(Json(TwoFactorConfirmResponse {
    message:
        "Two-factor authentication enabled"
            .to_string(),

    recovery_codes: recovery_codes_plain,
}))
```

---

# Step 58 — Important behavior

The API should return recovery codes **only when they're generated**.

For example:

```json
{
    "message": "Two-factor authentication enabled",
    "recovery_codes": [
        "7A31D92F81C4B6E0",
        "D19283AF7E19C442",
        "6F20AB91D8C53E71",
        "A8124B93C7E20F61",
        "E29F713C81A4D602",
        "93AC18F2D7B46109",
        "B721C8F0935A16DE",
        "4E19D72A8C613F50",
        "F381A29D64C70B15",
        "82D6E14F9A30C751"
    ]
}
```

The database only contains:

```text
code_hash
```

not:

```text
7A31D92F81C4B6E0
```

So if your database is compromised, the attacker doesn't immediately get usable recovery codes.

---

# Step 59 — Use recovery code during login

Now we need to allow:

```text
POST /auth/2fa/verify
```

to accept either:

```text
TOTP code
```

or:

```text
recovery code
```

Change:

```rust
pub struct TwoFactorVerifyRequest {
    pub challenge_token: String,
    pub code: String,
}
```

to:

```rust
pub struct TwoFactorVerifyRequest {
    pub challenge_token: String,
    pub code: String,
}
```

We don't actually need another field. The server can determine whether the supplied value is a TOTP code or recovery code.

The logic becomes:

```text
code
 │
 ├── TOTP valid?
 │       │
 │       └── yes → create session
 │
 └── TOTP invalid
         │
         ▼
    recovery code?
         │
      ┌──┴──┐
     yes    no
      │      │
      ▼      ▼
   consume  reject
    code
      │
      ▼
 create session
```

---

# Step 60 — Verify recovery code

Inside `/auth/2fa/verify`, after TOTP verification fails:

```rust
let recovery_hash =
    hash_recovery_code(&request.code);

let recovery_code = db
    .run(move |connection| {
        recovery_codes::table
            .filter(
                recovery_codes::user_id
                    .eq(user_id)
            )
            .filter(
                recovery_codes::code_hash
                    .eq(recovery_hash)
            )
            .filter(
                recovery_codes::used_at
                    .is_null()
            )
            .first::<RecoveryCode>(
                connection,
            )
            .optional()
    })
    .await
    .map_err(|error| error.to_string())?;
```

You'll need:

```rust
use diesel::OptionalExtension;
```

and:

```rust
use crate::models::recovery_code::RecoveryCode;
```

Then:

```rust
let recovery_code = match recovery_code {
    Some(code) => code,

    None => {
        return Err(
            "Invalid authentication code"
                .to_string()
        );
    }
};
```

Now consume it:

```rust
db.run(move |connection| {
    diesel::update(
        recovery_codes::table
            .filter(
                recovery_codes::id
                    .eq(recovery_code.id)
            )
    )
    .set(
        recovery_codes::used_at
            .eq(Utc::now().naive_utc())
    )
    .execute(connection)
})
.await
.map_err(|error| error.to_string())?;
```

Then continue with your existing session creation code.

---

# Step 61 — The complete 2FA login is now

```text
             POST /auth/login
                    │
                    ▼
             Verify password
                    │
                    ▼
             2FA enabled?
              /          \
            no            yes
            │              │
            ▼              ▼
       create session   challenge
                           │
                           ▼
                  POST /auth/2fa/verify
                           │
                    ┌──────┴──────┐
                    │             │
                  TOTP       Recovery code
                    │             │
                    │          mark used
                    │             │
                    └──────┬──────┘
                           ▼
                     create session
                           │
                           ▼
                    HttpOnly cookie
```

And recovery codes are:

```text
Generated: 10
Used:      0
Remaining: 10
```

If one is used:

```text
Generated: 10
Used:      1
Remaining: 9
```

---

## One important production consideration

The current recovery-code implementation uses SHA-256 hashing, which is appropriate for high-entropy random recovery codes. These codes are generated by the server rather than chosen by users, so they have much more entropy than passwords.

For passwords, continue using **Argon2id**.

So your credential storage now looks like:

```text
Password
   ↓
Argon2id
   ↓
password_hash


Session token
   ↓
SHA-256
   ↓
token_hash


Recovery code
   ↓
SHA-256
   ↓
code_hash


TOTP secret
   ↓
encrypted at rest
   ↓
secret
```

The next step should be **disable/regenerate 2FA**, including requiring the current password and TOTP/recovery code before allowing 2FA to be disabled. That closes an important security hole in the account-management side.

## Next step: Disable 2FA securely

We'll add `POST /auth/2fa/disable`.

The important part is: **don't allow a logged-in session alone to disable 2FA**. Require the user's current password **and** a valid TOTP code.

### 1. Add the request type

In `src/auth/routes.rs`:

```rust
#[derive(Deserialize)]
pub struct DisableTwoFactorRequest {
    pub password: String,
    pub code: String,
}
```

### 2. Add the route

```rust
#[post("/2fa/disable", data = "<request>")]
pub async fn disable_2fa(
    mut db: DbConn,
    user: AuthenticatedUser,
    request: Json<DisableTwoFactorRequest>,
) -> Result<Status, String> {
    let user_id = user.id;

    // 1. Load the user's password hash.
    let password_hash = db
        .run(move |connection| {
            users::table
                .filter(users::id.eq(user_id))
                .select(users::password_hash)
                .first::<String>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    // 2. Verify the current password.
    let password_valid =
        verify_password(&request.password, &password_hash)?;

    if !password_valid {
        return Err("Invalid password".to_string());
    }

    // 3. Load the confirmed TOTP secret.
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

    // 4. Build the TOTP generator.
    let totp = create_totp_from_secret(&secret)?;

    // 5. Verify the current authenticator code.
    let code_valid = totp
        .check(&request.code)
        .map_err(|error| error.to_string())?;

    if !code_valid {
        return Err("Invalid authentication code".to_string());
    }

    // 6. Disable 2FA and remove the TOTP secret.
    db.run(move |connection| {
        connection.transaction::<_, diesel::result::Error, _>(|connection| {
            diesel::update(
                users::table
                    .filter(users::id.eq(user_id))
            )
            .set(users::two_factor_enabled.eq(false))
            .execute(connection)?;

            diesel::delete(
                two_factor_secrets::table
                    .filter(two_factor_secrets::user_id.eq(user_id))
            )
            .execute(connection)?;

            diesel::delete(
                recovery_codes::table
                    .filter(recovery_codes::user_id.eq(user_id))
            )
            .execute(connection)?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Status::NoContent)
}
```

### 3. Add the imports

At the top of `routes.rs`, make sure you have:

```rust
use diesel::prelude::*;
use rocket::{
    http::Status,
    response::status,
    serde::json::Json,
};

use crate::{
    auth::{
        guards::AuthenticatedUser,
        password::verify_password,
        totp::create_totp_from_secret,
    },
    models::user::NewUser,
    schema::{
        recovery_codes,
        two_factor_secrets,
        users,
    },
    DbConn,
};
```

Adjust the existing imports rather than duplicating them.

### 4. Mount the route

Where you mount your auth routes in `main.rs`:

```rust
.routes(routes![
    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
    auth::routes::disable_2fa,
])
```

### 5. Test it

First authenticate normally.

Then send:

```http
POST /auth/2fa/disable
Content-Type: application/json
Cookie: session=YOUR_SESSION_TOKEN

{
    "password": "your-current-password",
    "code": "123456"
}
```

Successful response:

```http
204 No Content
```

Afterward:

* `users.two_factor_enabled` → `false`
* the TOTP secret is deleted
* all recovery codes are deleted
* existing sessions remain active

### Why delete the recovery codes?

Because they're only valid as a second-factor mechanism while 2FA is enabled. Keeping them after disabling 2FA creates unnecessary credential material.

**Next step:** we'll implement **recovery-code login**, so a user can authenticate when they don't have access to their authenticator app.

## Next step: Login with a recovery code

Now we'll make the recovery codes actually usable.

The flow will be:

```text
Password
   ↓
2FA challenge
   ↓
Recovery code
   ↓
Find unused code
   ↓
Mark code as used
   ↓
Create session
```

### 1. Add the request type

In `src/auth/routes.rs`:

```rust
#[derive(Deserialize)]
pub struct RecoveryCodeVerifyRequest {
    pub challenge_token: String,
    pub code: String,
}
```

### 2. Add the route

```rust
#[post("/2fa/recovery", data = "<request>")]
pub async fn verify_recovery_code(
    mut db: DbConn,
    request: Json<RecoveryCodeVerifyRequest>,
) -> Result<(Cookie<'static>, Json<LoginResponse>), String> {
    let challenge_hash = hash_token(&request.challenge_token);

    // Find the 2FA challenge.
    let challenge = db
        .run(move |connection| {
            two_factor_challenges::table
                .filter(
                    two_factor_challenges::token_hash
                        .eq(challenge_hash)
                )
                .first::<TwoFactorChallenge>(connection)
        })
        .await
        .map_err(|_| "Invalid or expired challenge".to_string())?;

    // Check expiration.
    if challenge.expires_at < Utc::now().naive_utc() {
        return Err("Invalid or expired challenge".to_string());
    }

    let user_id = challenge.user_id;

    // Hash the supplied recovery code.
    let code_hash = hash_recovery_code(&request.code);

    // Find an unused recovery code.
    let recovery_code = db
        .run(move |connection| {
            recovery_codes::table
                .filter(recovery_codes::user_id.eq(user_id))
                .filter(recovery_codes::code_hash.eq(code_hash))
                .filter(recovery_codes::used_at.is_null())
                .first::<RecoveryCode>(connection)
        })
        .await
        .map_err(|_| "Invalid recovery code".to_string())?;

    // Mark the recovery code as consumed.
    let recovery_id = recovery_code.id;

    db.run(move |connection| {
        diesel::update(
            recovery_codes::table
                .filter(recovery_codes::id.eq(recovery_id))
                .filter(recovery_codes::used_at.is_null())
        )
        .set(
            recovery_codes::used_at.eq(
                Utc::now().naive_utc()
            )
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    // Create a normal authenticated session.
    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);

    let session_id = Uuid::new_v4();
    let expires_at =
        Utc::now().naive_utc() + Duration::days(7);

    let new_session = NewSession {
        id: session_id,
        user_id,
        token_hash,
        expires_at,
    };

    db.run(move |connection| {
        diesel::insert_into(sessions::table)
            .values(&new_session)
            .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    // The challenge is single-use.
    db.run(move |connection| {
        diesel::delete(
            two_factor_challenges::table
                .filter(
                    two_factor_challenges::id
                        .eq(challenge.id)
                )
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie = Cookie::build(("session", raw_token))
        .path("/")
        .http_only(true)
        .build();

    Ok((
        cookie,
        Json(LoginResponse {
            message: "Login successful".to_string(),
            requires_2fa: false,
            challenge_token: None,
        }),
    ))
}
```

### 3. Add the imports

You should have these available in `routes.rs`:

```rust
use chrono::{Duration, Utc};
use rocket::http::{Cookie, Status};
use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    auth::{
        recovery::hash_recovery_code,
        session::{generate_token, hash_token},
    },
    models::{
        recovery_code::RecoveryCode,
        session::NewSession,
        two_factor_challenge::TwoFactorChallenge,
    },
    schema::{
        recovery_codes,
        sessions,
        two_factor_challenges,
    },
};
```

Use your actual model module names if yours differ.

### 4. Mount the route

Add it to `main.rs`:

```rust
.routes(routes![
    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
    auth::routes::disable_2fa,
    auth::routes::verify_recovery_code,
])
```

### 5. Test the complete fallback flow

First login with a user who has 2FA enabled:

```http
POST /auth/login
Content-Type: application/json

{
    "email": "user@example.com",
    "password": "password"
}
```

You should receive:

```json
{
    "message": "Two-factor authentication required",
    "requires_2fa": true,
    "challenge_token": "..."
}
```

Then instead of the authenticator code:

```http
POST /auth/2fa/recovery
Content-Type: application/json

{
    "challenge_token": "...",
    "code": "A1B2C3D4E5F60708"
}
```

If valid, you'll receive the session cookie.

The recovery code is immediately marked:

```text
used_at = current timestamp
```

Trying that same recovery code again should return:

```text
Invalid recovery code
```

### One important improvement

The **challenge lookup, recovery-code consumption, and session creation should eventually happen inside one database transaction**. Otherwise, a request failure between those operations could leave inconsistent state.

We'll handle that next, along with **regenerating recovery codes** so users can replace their entire recovery-code set.

## Next step: Regenerate recovery codes

We'll add:

```text
POST /auth/2fa/recovery-codes/regenerate
```

The user must already be authenticated and provide their **current password + TOTP code**. This replaces all old recovery codes.

### 1. Add the request type

In `src/auth/routes.rs`:

```rust
#[derive(Deserialize)]
pub struct RegenerateRecoveryCodesRequest {
    pub password: String,
    pub code: String,
}
```

### 2. Add the response type

```rust
#[derive(Serialize)]
pub struct RecoveryCodesResponse {
    pub recovery_codes: Vec<String>,
}
```

### 3. Add the route

```rust
#[post(
    "/2fa/recovery-codes/regenerate",
    data = "<request>"
)]
pub async fn regenerate_recovery_codes(
    mut db: DbConn,
    user: AuthenticatedUser,
    request: Json<RegenerateRecoveryCodesRequest>,
) -> Result<Json<RecoveryCodesResponse>, String> {
    let user_id = user.id;

    // Load the user's password hash.
    let password_hash = db
        .run(move |connection| {
            users::table
                .filter(users::id.eq(user_id))
                .select(users::password_hash)
                .first::<String>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    // Verify password.
    if !verify_password(
        &request.password,
        &password_hash,
    )? {
        return Err("Invalid password".to_string());
    }

    // Load the confirmed TOTP secret.
    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(two_factor_secrets::user_id.eq(user_id))
                .filter(
                    two_factor_secrets::confirmed_at
                        .is_not_null()
                )
                .select(two_factor_secrets::secret)
                .first::<String>(connection)
        })
        .await
        .map_err(|_| "Two-factor authentication is not enabled".to_string())?;

    // Build TOTP.
    let totp = create_totp_from_secret(&secret)?;

    // Verify current authenticator code.
    if !totp
        .check(&request.code)
        .map_err(|error| error.to_string())?
    {
        return Err("Invalid authentication code".to_string());
    }

    // Generate a completely new set.
    let mut raw_codes = Vec::with_capacity(10);

    for _ in 0..10 {
        raw_codes.push(generate_recovery_code());
    }

    // Hash the codes before storing them.
    let new_codes: Vec<NewRecoveryCode> = raw_codes
        .iter()
        .map(|code| NewRecoveryCode {
            id: Uuid::new_v4(),
            user_id,
            code_hash: hash_recovery_code(code),
        })
        .collect();

    // Replace the old codes atomically.
    db.run(move |connection| {
        connection.transaction::<_, diesel::result::Error, _>(
            |connection| {
                diesel::delete(
                    recovery_codes::table
                        .filter(
                            recovery_codes::user_id
                                .eq(user_id)
                        )
                )
                .execute(connection)?;

                diesel::insert_into(
                    recovery_codes::table
                )
                .values(&new_codes)
                .execute(connection)?;

                Ok(())
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Json(RecoveryCodesResponse {
        recovery_codes: raw_codes,
    }))
}
```

### 4. Imports

Make sure `routes.rs` has:

```rust
use crate::{
    auth::{
        guards::AuthenticatedUser,
        password::verify_password,
        recovery::{
            generate_recovery_code,
            hash_recovery_code,
        },
        totp::create_totp_from_secret,
    },
    models::{
        recovery_code::{
            NewRecoveryCode,
        },
    },
    schema::{
        recovery_codes,
        two_factor_secrets,
        users,
    },
    DbConn,
};
```

And:

```rust
use diesel::prelude::*;
use chrono::Utc;
use uuid::Uuid;
```

### 5. Mount the route

In `main.rs`:

```rust
.routes(routes![
    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
    auth::routes::verify_recovery_code,
    auth::routes::disable_2fa,
    auth::routes::regenerate_recovery_codes,
])
```

### 6. Test

Send:

```http
POST /auth/2fa/recovery-codes/regenerate
Content-Type: application/json
Cookie: session=YOUR_SESSION

{
    "password": "your-password",
    "code": "123456"
}
```

Response:

```json
{
    "recovery_codes": [
        "8A13F4C91B27D602",
        "C7201A9D45E83F11",
        "6B9E2D1047FA8C32",
        "..."
    ]
}
```

The old recovery codes are now invalid.

### Important security rule

The API should return these codes **only once**. We store only:

```text
raw recovery code
        ↓
SHA-256
        ↓
database
```

So if someone gets database access, they don't get the usable recovery codes directly.

At this point your authentication system has:

```text
Registration
    ↓
Argon2id password
    ↓
Login
    ↓
Password correct?
    ├── No → reject
    │
    └── Yes
          ↓
       2FA enabled?
       ├── No → session
       │
       └── Yes
             ↓
          TOTP code
          ├── Valid → session
          │
          └── Invalid
                 ↓
            Recovery code
                 ↓
               session
```

**Next:** we'll clean up the authentication flow by making **2FA verification + recovery-code consumption + session creation transactional**, preventing replay/race-condition problems.

## Next step: Make 2FA verification atomic

Right now, there's a small security problem: the recovery code is marked used and the session is created in separate database operations.

We want this:

```text
Verify code
    ↓
BEGIN TRANSACTION
    ↓
Mark challenge consumed
    ↓
Mark recovery code consumed
    ↓
Create session
    ↓
COMMIT
```

If anything fails, **everything rolls back**.

### 1. Update `verify_recovery_code`

Replace the current route with this version:

```rust
#[post("/2fa/recovery", data = "<request>")]
pub async fn verify_recovery_code(
    mut db: DbConn,
    request: Json<RecoveryCodeVerifyRequest>,
) -> Result<(Cookie<'static>, Json<LoginResponse>), String> {
    let challenge_hash = hash_token(&request.challenge_token);
    let recovery_hash = hash_recovery_code(&request.code);

    let raw_token = generate_token();
    let session_token_hash = hash_token(&raw_token);

    let now = Utc::now().naive_utc();
    let session_expires_at = now + Duration::days(7);

    let cookie = Cookie::build(("session", raw_token))
        .path("/")
        .http_only(true)
        .build();

    db.run(move |connection| {
        connection.transaction::<(), String, _>(|connection| {
            // Find the challenge.
            let challenge = two_factor_challenges::table
                .filter(
                    two_factor_challenges::token_hash
                        .eq(&challenge_hash)
                )
                .first::<TwoFactorChallenge>(connection)
                .map_err(|_| {
                    "Invalid or expired challenge".to_string()
                })?;

            // Check challenge expiration.
            if challenge.expires_at < now {
                return Err(
                    "Invalid or expired challenge".to_string()
                );
            }

            let user_id = challenge.user_id;

            // Find an unused recovery code.
            let recovery_code = recovery_codes::table
                .filter(
                    recovery_codes::user_id.eq(user_id)
                )
                .filter(
                    recovery_codes::code_hash
                        .eq(&recovery_hash)
                )
                .filter(
                    recovery_codes::used_at.is_null()
                )
                .first::<RecoveryCode>(connection)
                .map_err(|_| {
                    "Invalid recovery code".to_string()
                })?;

            // Mark recovery code as used.
            diesel::update(
                recovery_codes::table
                    .filter(
                        recovery_codes::id
                            .eq(recovery_code.id)
                    )
                    .filter(
                        recovery_codes::used_at
                            .is_null()
                    )
            )
            .set(
                recovery_codes::used_at.eq(now)
            )
            .execute(connection)
            .map_err(|error| error.to_string())?;

            // Create authenticated session.
            let new_session = NewSession {
                id: Uuid::new_v4(),
                user_id,
                token_hash: session_token_hash.clone(),
                expires_at: session_expires_at,
            };

            diesel::insert_into(sessions::table)
                .values(&new_session)
                .execute(connection)
                .map_err(|error| error.to_string())?;

            // Delete the challenge so it cannot be reused.
            diesel::delete(
                two_factor_challenges::table
                    .filter(
                        two_factor_challenges::id
                            .eq(challenge.id)
                    )
            )
            .execute(connection)
            .map_err(|error| error.to_string())?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok((
        cookie,
        Json(LoginResponse {
            message: "Login successful".to_string(),
            requires_2fa: false,
            challenge_token: None,
        }),
    ))
}
```

### 2. Why this is better

Previously:

```text
UPDATE recovery_codes
        ↓
INSERT session
        ↓
DELETE challenge
```

Those were three separate database operations.

Now Diesel gives us:

```rust
connection.transaction(|connection| {
    // everything
})
```

If session creation fails:

```text
Recovery code remains unused
Challenge remains available
No partial authentication
```

If deleting the challenge fails:

```text
Recovery code remains unused
Session is not created
```

That's exactly what we want.

### 3. Add the required imports

At the top of `routes.rs`:

```rust
use chrono::{Duration, Utc};
use diesel::prelude::*;
use rocket::http::{Cookie, Status};
use uuid::Uuid;
```

And your existing imports for:

```rust
use crate::{
    auth::{
        recovery::hash_recovery_code,
        session::{generate_token, hash_token},
    },
    models::{
        recovery_code::RecoveryCode,
        session::NewSession,
        two_factor_challenge::TwoFactorChallenge,
    },
    schema::{
        recovery_codes,
        sessions,
        two_factor_challenges,
    },
    DbConn,
};
```

### 4. One more important race-condition fix

There's still a subtle issue with:

```rust
.filter(recovery_codes::used_at.is_null())
```

Two simultaneous requests could potentially find the same unused code before either update happens.

The conditional update helps:

```rust
.filter(recovery_codes::used_at.is_null())
```

But we should also verify that **exactly one row was updated**.

Change:

```rust
diesel::update(
    recovery_codes::table
        .filter(
            recovery_codes::id
                .eq(recovery_code.id)
        )
        .filter(
            recovery_codes::used_at
                .is_null()
        )
)
.set(
    recovery_codes::used_at.eq(now)
)
.execute(connection)
.map_err(|error| error.to_string())?;
```

to:

```rust
let updated = diesel::update(
    recovery_codes::table
        .filter(
            recovery_codes::id
                .eq(recovery_code.id)
        )
        .filter(
            recovery_codes::used_at
                .is_null()
        )
)
.set(
    recovery_codes::used_at.eq(now)
)
.execute(connection)
.map_err(|error| error.to_string())?;

if updated != 1 {
    return Err(
        "Recovery code has already been used".to_string()
    );
}
```

Now the database itself protects the one-time-use property.

---

### Your authentication architecture now looks like this

```text
                    REGISTER
                       │
                       ▼
                 Argon2id hash
                       │
                       ▼
                     LOGIN
                       │
                Password valid?
                  ┌────┴────┐
                 No        Yes
                 │          │
               Reject    2FA enabled?
                           ┌──┴──┐
                          No    Yes
                           │      │
                        Session  Challenge
                                  │
                           ┌──────┴──────┐
                           │             │
                          TOTP       Recovery Code
                           │             │
                           └──────┬──────┘
                                  │
                                  ▼
                             TRANSACTION
                                  │
                          ┌───────┼───────┐
                          │       │       │
                       consume  create  consume
                       factor   session challenge
                          │       │       │
                          └───────┼───────┘
                                  │
                                COMMIT
                                  │
                                  ▼
                              AUTHENTICATED
```

**Next step:** harden the session cookie with **HttpOnly + Secure + SameSite**, and fix the session guard so the browser authentication layer is production-safe.

## Next step: Harden the session cookie

Right now we're creating the cookie like this:

```rust
Cookie::build(("session", raw_token))
    .path("/")
    .http_only(true)
    .build()
```

We'll make the cookie safer for a browser-based application.

### 1. Create one cookie helper

Create:

`src/auth/cookie.rs`

```rust id="58321"
use rocket::http::{Cookie, SameSite};

pub fn session_cookie(token: String) -> Cookie<'static> {
    Cookie::build(("session", token))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .build()
}

pub fn remove_session_cookie() -> Cookie<'static> {
    Cookie::build(("session", ""))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .build()
}
```

### 2. Export the module

In:

`src/auth/mod.rs`

add:

```rust id="79436"
pub mod cookie;
```

Your module list should now look roughly like:

```rust id="7r8kz1"
pub mod cookie;
pub mod guards;
pub mod password;
pub mod recovery;
pub mod routes;
pub mod session;
pub mod totp;
```

### 3. Replace cookie creation

Anywhere you currently have:

```rust id="p5w9h2"
Cookie::build(("session", raw_token))
    .path("/")
    .http_only(true)
    .build()
```

replace it with:

```rust id="1xq2vf"
session_cookie(raw_token)
```

Import:

```rust id="m3s7k8"
use crate::auth::cookie::session_cookie;
```

Do this in:

* normal login
* TOTP verification
* recovery-code verification

### 4. Update logout

Instead of:

```rust id="3b8wq1"
cookies.remove(
    Cookie::build(("session", ""))
        .path("/")
        .build()
);
```

use:

```rust id="5f2n9c"
cookies.remove(remove_session_cookie());
```

Import:

```rust id="q7d4ma"
use crate::auth::cookie::remove_session_cookie;
```

### 5. What these flags do

```text id="x8r2vp"
HttpOnly
   ↓
JavaScript cannot read the session cookie

Secure
   ↓
Cookie is sent only over HTTPS

SameSite=Lax
   ↓
Reduces cross-site cookie attacks

Path=/
   ↓
Cookie applies to the entire application
```

For production, **`Secure` should stay enabled**.

If you're developing locally over plain HTTP, you may temporarily need:

```rust id="6c1j9x"
.secure(false)
```

But don't accidentally carry that setting into production.

---

## 6. Add the cookie module to login

For example, your successful non-2FA login should end with:

```rust id="w6p4zd"
let cookie = session_cookie(raw_token);

Ok((
    cookie,
    Json(LoginResponse {
        message: "Login successful".to_string(),
        requires_2fa: false,
        challenge_token: None,
    }),
))
```

And TOTP verification should use the same helper:

```rust id="2m7qka"
let cookie = session_cookie(raw_token);
```

This is useful because you now have **one place** controlling your authentication cookie security.

---

## 7. Important: don't use `get_private()` yet

Your current session system stores:

```text
random session token
        ↓
SHA-256
        ↓
database
```

The browser receives only the random token.

So your guard should continue using:

```rust id="h4n9cx"
cookies.get("session")
```

not:

```rust id="q2v8dy"
cookies.get_private("session")
```

Mixing `add_private()` with `get()` or `add()` with `get_private()` would break the authentication flow.

---

## 8. Update the session guard

Your guard should continue extracting:

```rust id="j5k2ra"
let cookie = match request.cookies().get("session") {
    Some(cookie) => cookie,
    None => {
        return Outcome::Error((
            Status::Unauthorized,
            (),
        ));
    }
};
```

Then:

```rust id="s9c4wb"
let raw_token = cookie.value().to_string();
let token_hash = hash_token(&raw_token);
```

The database still contains only:

```text
token_hash
```

and never the browser's raw session token.

---

### Current security model

```text
Browser
   │
   │ HTTPS
   ▼
HttpOnly + Secure + SameSite cookie
   │
   │ raw random token
   ▼
Rocket
   │
   │ SHA-256
   ▼
Database
   │
   ▼
session.token_hash
```

That's a solid server-side session design.

**Next step:** we'll improve `AuthenticatedUser` so every authenticated request also updates `last_used_at` and properly rejects expired sessions.

## Next step: Improve `AuthenticatedUser`

We'll make the session guard do three things:

1. Find the session.
2. Reject expired or revoked sessions.
3. Update `last_used_at` when the session is successfully used.

### 1. Replace `src/auth/guards.rs`

Use this version:

```rust
use chrono::Utc;
use diesel::prelude::*;
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};
use uuid::Uuid;

use crate::{
    auth::session::hash_token,
    models::user::User,
    schema::{sessions, users},
    DbConn,
};

pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        let cookies = request.cookies();

        let cookie = match cookies.get("session") {
            Some(cookie) => cookie,
            None => {
                return Outcome::Error((
                    Status::Unauthorized,
                    (),
                ));
            }
        };

        let raw_token = cookie.value().to_string();
        let token_hash = hash_token(&raw_token);

        let db = match request.rocket().state::<DbConn>() {
            Some(db) => db,
            None => {
                return Outcome::Error((
                    Status::InternalServerError,
                    (),
                ));
            }
        };

        let now = Utc::now().naive_utc();

        let result = db
            .run(move |connection| {
                sessions::table
                    .inner_join(
                        users::table.on(
                            users::id
                                .eq(sessions::user_id),
                        ),
                    )
                    .filter(
                        sessions::token_hash
                            .eq(&token_hash),
                    )
                    .filter(
                        sessions::revoked_at
                            .is_null(),
                    )
                    .filter(
                        sessions::expires_at
                            .gt(now),
                    )
                    .select((
                        users::id,
                        users::email,
                    ))
                    .first::<(Uuid, String)>(
                        connection,
                    )
            })
            .await;

        match result {
            Ok((user_id, email)) => {
                // Refresh last-used timestamp.
                let refresh_hash = hash_token(
                    &raw_token,
                );

                let _ = db
                    .run(move |connection| {
                        diesel::update(
                            sessions::table
                                .filter(
                                    sessions::token_hash
                                        .eq(refresh_hash),
                                )
                                .filter(
                                    sessions::revoked_at
                                        .is_null(),
                                ),
                        )
                        .set(
                            sessions::last_used_at
                                .eq(now),
                        )
                        .execute(connection)
                    })
                    .await;

                Outcome::Success(
                    AuthenticatedUser {
                        id: user_id,
                        email,
                    },
                )
            }

            Err(_) => Outcome::Error((
                Status::Unauthorized,
                (),
            )),
        }
    }
}
```

### 2. Remove the unused `User` import

The guard no longer loads the entire `User` model, so remove:

```rust
models::user::User,
```

Your imports should therefore be:

```rust
use crate::{
    auth::session::hash_token,
    schema::{sessions, users},
    DbConn,
};
```

### 3. What changed?

Previously we checked:

```text
token
 ↓
session exists
 ↓
not revoked
 ↓
expiration check
```

Now the database query itself checks expiration:

```rust
.filter(sessions::expires_at.gt(now))
```

So an expired session never reaches your protected route.

Then successful requests update:

```text
last_used_at = current time
```

That gives you useful session activity information.

---

## 4. Better approach: don't update on every request

There's one optimization I'd make before calling this finished.

If your application gets lots of requests, this:

```text
GET /profile
GET /profile
GET /profile
GET /profile
GET /profile
```

would produce five database `UPDATE`s.

Instead, later we can refresh `last_used_at` only when it's older than, say, **5 minutes**.

Conceptually:

```text
Request
   ↓
Session valid?
   ↓
last_used_at older than 5 minutes?
   ├── No → continue
   └── Yes → update timestamp
```

We'll implement that after the core auth flow is stable.

---

## 5. One thing to check now

Run:

```bash
cargo check
```

If you get an error around:

```rust
request.rocket().state::<DbConn>()
```

**stop there and send me the exact compiler error.**

Your Rocket database pool setup determines the correct way to access `DbConn` from a request guard, and I don't want to have you blindly patch around that.

Once `cargo check` is clean, the next security layer is **CSRF protection for cookie-based authentication**.

## Next step: Add CSRF protection

Because we're using **cookie-based authentication**, CSRF protection is important for state-changing endpoints such as:

```text
POST /auth/logout
POST /auth/logout-all
POST /auth/2fa/setup
POST /auth/2fa/confirm
POST /auth/2fa/disable
POST /auth/2fa/recovery-codes/regenerate
```

We'll use the **double-submit cookie** pattern:

```text
Browser
   │
   ├── session cookie (HttpOnly)
   │
   ├── csrf cookie
   │
   └── X-CSRF-Token header
             │
             ▼
          Rocket
             │
       compare cookie
       against header
             │
        ┌────┴────┐
       match    mismatch
        │           │
      allow       403
```

### 1. Add `subtle`

In `Cargo.toml`:

```toml
subtle = "2"
```

Then:

```bash
cargo check
```

### 2. Create `src/auth/csrf.rs`

```rust
use rand::{rngs::OsRng, RngCore};
use subtle::ConstantTimeEq;

pub fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 32];

    OsRng.fill_bytes(&mut bytes);

    hex::encode(bytes)
}

pub fn valid_csrf_token(
    cookie_token: &str,
    header_token: &str,
) -> bool {
    cookie_token
        .as_bytes()
        .ct_eq(header_token.as_bytes())
        .into()
}
```

The token contains 256 bits of randomness.

### 3. Export the module

In `src/auth/mod.rs`:

```rust
pub mod csrf;
```

So you'll have:

```rust
pub mod cookie;
pub mod csrf;
pub mod guards;
pub mod password;
pub mod recovery;
pub mod routes;
pub mod session;
pub mod totp;
```

### 4. Create a CSRF guard

Add this to `src/auth/guards.rs`:

```rust
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};

use crate::auth::csrf::valid_csrf_token;

pub struct CsrfProtected;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for CsrfProtected {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        let cookies = request.cookies();

        let cookie_token = match cookies.get("csrf_token") {
            Some(cookie) => cookie.value(),
            None => {
                return Outcome::Error((
                    Status::Forbidden,
                    (),
                ));
            }
        };

        let header_token = match request.headers().get_one("X-CSRF-Token") {
            Some(token) => token,
            None => {
                return Outcome::Error((
                    Status::Forbidden,
                    (),
                ));
            }
        };

        if !valid_csrf_token(cookie_token, header_token) {
            return Outcome::Error((
                Status::Forbidden,
                (),
            ));
        }

        Outcome::Success(CsrfProtected)
    }
}
```

### 5. Generate the CSRF cookie

Create an endpoint that gives the frontend a CSRF token.

In `routes.rs`:

```rust
use rocket::http::{Cookie, SameSite};
use crate::auth::csrf::generate_csrf_token;

#[get("/csrf")]
pub fn csrf_token(
    cookies: &rocket::http::CookieJar<'_>,
) {
    let token = generate_csrf_token();

    cookies.add(
        Cookie::build(("csrf_token", token))
            .path("/")
            .http_only(false)
            .secure(true)
            .same_site(SameSite::Lax)
            .build(),
    );
}
```

Notice the difference:

```text
session
  HttpOnly = true

csrf_token
  HttpOnly = false
```

That's intentional.

JavaScript needs to read the CSRF cookie so it can send the value in:

```http
X-CSRF-Token: ...
```

JavaScript **must not** be able to read the session cookie.

### 6. Mount the endpoint

In `main.rs`:

```rust
.routes(routes![
    auth::routes::csrf_token,

    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
    auth::routes::verify_recovery_code,
    auth::routes::disable_2fa,
    auth::routes::regenerate_recovery_codes,
])
```

### 7. Protect a POST route

For example, change logout from:

```rust
#[post("/logout")]
pub async fn logout(
    mut db: DbConn,
    cookies: &CookieJar<'_>,
) -> Status {
```

to:

```rust
#[post("/logout")]
pub async fn logout(
    _csrf: CsrfProtected,
    mut db: DbConn,
    cookies: &CookieJar<'_>,
) -> Status {
```

Import:

```rust
use crate::auth::guards::CsrfProtected;
```

Rocket will execute the request guard before your logout function runs.

Do the same for:

```text
/logout-all
/2fa/setup
/2fa/confirm
/2fa/disable
/2fa/recovery-codes/regenerate
```

### 8. Frontend flow

First:

```http
GET /auth/csrf
```

The server sets:

```text
csrf_token=abc123...
```

Then JavaScript reads that cookie and sends:

```http
POST /auth/2fa/disable
X-CSRF-Token: abc123...
Content-Type: application/json
```

The server compares:

```text
Cookie csrf_token
       =
X-CSRF-Token header
```

If they don't match:

```http
403 Forbidden
```

### One important distinction

Don't require CSRF for every endpoint blindly.

For your current API, the clean split is:

```text
GET
 └── no CSRF

POST /register
 └── can remain public

POST /login
 └── can remain public

POST /2FA/verify
 └── challenge-based authentication

POST /2FA/recovery
 └── challenge-based authentication

Authenticated state-changing routes
 └── CSRF required
```

For a production app, you can also protect login/register against CSRF depending on the browser architecture.

**Next step:** we'll add **rate limiting and brute-force protection** to login, TOTP verification, and recovery-code attempts.

## Next step: Rate-limit authentication attempts

We need to protect three particularly sensitive operations:

```text
POST /auth/login
POST /auth/2fa/verify
POST /auth/2fa/recovery
```

A simple first implementation is an **in-memory rate limiter**. It's good for learning and for a single-server development environment. If you later run multiple instances, move this state to Redis or another shared store.

### 1. Add a rate limiter module

Create:

`src/auth/rate_limit.rs`

```rust
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, AttemptState>>,
    max_attempts: u32,
    window: Duration,
}

struct AttemptState {
    count: u32,
    window_started: Instant,
}

impl RateLimiter {
    pub fn new(
        max_attempts: u32,
        window: Duration,
    ) -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            max_attempts,
            window,
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        let now = Instant::now();

        let state = attempts
            .entry(key.to_string())
            .or_insert(AttemptState {
                count: 0,
                window_started: now,
            });

        if now.duration_since(state.window_started)
            >= self.window
        {
            state.count = 0;
            state.window_started = now;
        }

        if state.count >= self.max_attempts {
            return false;
        }

        state.count += 1;

        true
    }

    pub fn reset(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        attempts.remove(key);
    }
}
```

### 2. Export it

In `src/auth/mod.rs`:

```rust
pub mod rate_limit;
```

### 3. Add the limiter to Rocket state

In `main.rs`, import:

```rust
use std::time::Duration;

use crate::auth::rate_limit::RateLimiter;
```

Then when building Rocket:

```rust
.manage(
    RateLimiter::new(
        5,
        Duration::from_secs(60),
    )
)
```

For example:

```rust
rocket::build()
    .manage(RateLimiter::new(
        5,
        Duration::from_secs(60),
    ))
    .attach(DbConn::fairing())
    .mount("/", routes![/* ... */])
```

This gives us:

```text
5 attempts
per 60 seconds
per limiter key
```

### 4. Use the limiter in login

At the beginning of your login route:

```rust
let limiter = request
    .rocket()
    .state::<RateLimiter>()
    .ok_or_else(|| {
        "Rate limiter unavailable".to_string()
    })?;
```

You'll need the request to be available, so change:

```rust
pub async fn login(
    mut db: DbConn,
    request: Json<LoginRequest>,
)
```

to:

```rust
pub async fn login(
    mut db: DbConn,
    request: Json<LoginRequest>,
    rocket_request: &Request<'_>,
)
```

However, **don't do that** with Rocket route parameters. Rocket doesn't inject `Request` into routes that way.

Instead, the cleaner approach is to make a request guard.

### 5. Create `ClientIp`

In `rate_limit.rs`:

```rust
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};

pub struct ClientIp(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ClientIp {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        match request.client_ip() {
            Some(ip) => Outcome::Success(
                ClientIp(ip.to_string())
            ),
            None => Outcome::Error((
                Status::BadRequest,
                (),
            )),
        }
    }
}
```

### 6. Use it in login

Now:

```rust
#[post("/login", data = "<request>")]
pub async fn login(
    mut db: DbConn,
    client_ip: ClientIp,
    request: Json<LoginRequest>,
    rate_limiter: &State<'_, RateLimiter>,
) -> Result<..., String> {
```

But again, Rocket route state should generally be accessed through a guard rather than relying on a mutable reference parameter.

So let's make this simpler and more idiomatic.

Create a combined guard:

```rust
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
    State,
};

pub struct AuthRateLimit {
    pub key: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        let ip = match request.client_ip() {
            Some(ip) => ip.to_string(),
            None => {
                return Outcome::Error((
                    Status::BadRequest,
                    (),
                ));
            }
        };

        let limiter =
            match request.rocket().state::<RateLimiter>() {
                Some(limiter) => limiter,
                None => {
                    return Outcome::Error((
                        Status::InternalServerError,
                        (),
                    ));
                }
            };

        if !limiter.check(&ip) {
            return Outcome::Error((
                Status::TooManyRequests,
                (),
            ));
        }

        Outcome::Success(AuthRateLimit {
            key: ip,
        })
    }
}
```

Then login becomes:

```rust
#[post("/login", data = "<request>")]
pub async fn login(
    _rate_limit: AuthRateLimit,
    mut db: DbConn,
    request: Json<LoginRequest>,
) -> Result<..., String> {
```

Now Rocket automatically performs the rate-limit check before entering the login handler.

### 7. Apply the same guard to 2FA

For:

```rust
#[post("/2fa/verify", data = "<request>")]
```

use:

```rust
#[post("/2fa/verify", data = "<request>")]
pub async fn verify_2fa(
    _rate_limit: AuthRateLimit,
    mut db: DbConn,
    request: Json<TwoFactorVerifyRequest>,
) -> Result<..., String> {
```

And recovery:

```rust
#[post("/2fa/recovery", data = "<request>")]
pub async fn verify_recovery_code(
    _rate_limit: AuthRateLimit,
    mut db: DbConn,
    request: Json<RecoveryCodeVerifyRequest>,
) -> Result<..., String> {
```

### 8. Important limitation

This limiter currently uses:

```text
IP address
```

That's useful, but not sufficient by itself.

For login, eventually we want something closer to:

```text
IP + normalized email
```

For example:

```text
192.0.2.10:user@example.com
```

That prevents an attacker from bypassing an account-specific limit simply by changing IP addresses.

For 2FA:

```text
IP + challenge token
```

is also useful.

### 9. Test

Make six failed login requests within 60 seconds.

The first five should reach the login logic.

The sixth should return:

```http
429 Too Many Requests
```

That gives us our first brute-force defense.

---

### Current authentication stack

```text
                    HTTPS
                      │
                      ▼
              Secure session cookie
                      │
                      ▼
                CSRF protection
                      │
                      ▼
                Rate limiting
                      │
                      ▼
                 Auth guard
                      │
                      ▼
              Session validation
                      │
             ┌────────┴────────┐
             │                 │
          Password            2FA
             │                 │
          Argon2id       TOTP / Recovery
             │                 │
             └────────┬────────┘
                      ▼
              Server-side session
```

One correction before we continue: **the in-memory limiter above is intentionally a development implementation, not a production distributed limiter.** When you deploy multiple Rocket instances, we'll move the counter to Redis so all instances share the same limits.

**Next step: session management endpoints** — list the user's active sessions, revoke one session, and revoke all sessions.

Yes. Let's add an endpoint that returns the authenticated user's **active sessions**.

We'll return useful metadata, but **never return the session token or token hash**.

### 1. Add a response model

In `src/auth/routes.rs`:

```rust
#[derive(Serialize)]
pub struct ActiveSessionResponse {
    pub id: Uuid,
    pub created_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub current: bool,
}
```

### 2. Add the route

```rust
#[get("/sessions")]
pub async fn list_sessions(
    mut db: DbConn,
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
                .filter(
                    sessions::user_id.eq(user_id)
                )
                .filter(
                    sessions::revoked_at.is_null()
                )
                .filter(
                    sessions::expires_at.gt(now)
                )
                .select((
                    sessions::id,
                    sessions::token_hash,
                    sessions::created_at,
                    sessions::last_used_at,
                    sessions::expires_at,
                ))
                .order(
                    sessions::last_used_at.desc()
                )
                .load::<(
                    Uuid,
                    String,
                    NaiveDateTime,
                    NaiveDateTime,
                    NaiveDateTime,
                )>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    let result = sessions
        .into_iter()
        .map(
            |(
                id,
                token_hash,
                created_at,
                last_used_at,
                expires_at,
            )| {
                let current = current_session_hash
                    .as_deref()
                    == Some(token_hash.as_str());

                ActiveSessionResponse {
                    id,
                    created_at,
                    last_used_at,
                    expires_at,
                    current,
                }
            },
        )
        .collect();

    Ok(Json(result))
}
```

### 3. Add the imports

At the top of `routes.rs`:

```rust
use chrono::{NaiveDateTime, Utc};
use rocket::{
    http::{CookieJar, Status},
    serde::json::Json,
};
use uuid::Uuid;

use crate::{
    auth::{
        guards::AuthenticatedUser,
        session::hash_token,
    },
    schema::sessions,
    DbConn,
};
```

Adjust the existing imports rather than duplicating them.

### 4. Mount the route

In `main.rs`:

```rust
.routes(routes![
    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::csrf_token,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
    auth::routes::verify_recovery_code,
    auth::routes::disable_2fa,
    auth::routes::regenerate_recovery_codes,
    auth::routes::list_sessions,
])
```

### 5. Test it

With a valid session:

```http
GET /auth/sessions
Cookie: session=YOUR_SESSION_TOKEN
```

You'll get something like:

```json
[
    {
        "id": "7c2d5d9a-9d55-4b0e-9f1a-3f1b3e6f7a20",
        "created_at": "2026-09-18T14:20:00",
        "last_used_at": "2026-09-18T15:42:11",
        "expires_at": "2026-09-25T14:20:00",
        "current": true
    },
    {
        "id": "9a3f3c11-5d12-42b1-a4f1-72e7e9d2f810",
        "created_at": "2026-09-17T20:10:00",
        "last_used_at": "2026-09-18T10:12:43",
        "expires_at": "2026-09-24T20:10:00",
        "current": false
    }
]
```

### Why we return `current`

This lets your frontend display:

```text
Active Sessions

● Current session
  Created: Sep 18
  Last active: Just now

  Session
  Created: Sep 17
  Last active: 1 hour ago
  [Revoke]
```

Notice that we **never expose**:

```text
token_hash
raw session token
```

The `token_hash` is used internally only to determine which session belongs to the current browser.

### One improvement

Right now `sessions` can accumulate expired rows indefinitely.

Later we'll add a cleanup mechanism:

```text
expired sessions
       ↓
periodic cleanup
       ↓
DELETE
```

Next, the natural companion endpoint is:

```text
DELETE /auth/sessions/<session_id>
```

so the user can revoke **one specific device/session** without logging out everywhere.

## Next step: Revoke one specific session

We'll add:

```text
DELETE /auth/sessions/<session_id>
```

This lets a user log out a specific device while leaving their other sessions active.

### 1. Add the route

In `src/auth/routes.rs`:

```rust
#[delete("/sessions/<session_id>")]
pub async fn revoke_session(
    mut db: DbConn,
    user: AuthenticatedUser,
    session_id: Uuid,
    cookies: &CookieJar<'_>,
) -> Result<Status, Status> {
    let user_id = user.id;

    // Identify the current session.
    let current_session_hash = cookies
        .get("session")
        .map(|cookie| hash_token(cookie.value()));

    // Load the session being revoked.
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

    // Revoke it.
    db.run(move |connection| {
        diesel::update(
            sessions::table
                .filter(sessions::id.eq(session_id))
                .filter(sessions::user_id.eq(user_id))
                .filter(sessions::revoked_at.is_null()),
        )
        .set(
            sessions::revoked_at
                .eq(Utc::now().naive_utc()),
        )
        .execute(connection)
    })
    .await
    .map_err(|_| Status::InternalServerError)?;

    // If the user revoked the current session,
    // remove the browser cookie too.
    if current_session_hash.as_deref()
        == Some(target_token_hash.as_str())
    {
        cookies.remove(
            remove_session_cookie()
        );
    }

    Ok(Status::NoContent)
}
```

### 2. Imports

Make sure you have:

```rust
use chrono::Utc;
use diesel::prelude::*;
use rocket::http::{CookieJar, Status};
use uuid::Uuid;

use crate::{
    auth::{
        cookie::remove_session_cookie,
        guards::AuthenticatedUser,
        session::hash_token,
    },
    schema::sessions,
    DbConn,
};
```

### 3. Mount the route

In `main.rs`:

```rust
.routes(routes![
    auth::routes::register,
    auth::routes::login,
    auth::routes::profile,
    auth::routes::logout,
    auth::routes::logout_all,
    auth::routes::csrf_token,
    auth::routes::setup_2fa,
    auth::routes::confirm_2fa,
    auth::routes::verify_2fa,
    auth::routes::verify_recovery_code,
    auth::routes::disable_2fa,
    auth::routes::regenerate_recovery_codes,
    auth::routes::list_sessions,
    auth::routes::revoke_session,
])
```

### 4. Test another session

First:

```http
GET /auth/sessions
Cookie: session=YOUR_SESSION
```

Suppose you receive:

```json
[
    {
        "id": "7c2d5d9a-9d55-4b0e-9f1a-3f1b3e6f7a20",
        "created_at": "2026-09-18T14:20:00",
        "last_used_at": "2026-09-18T15:42:11",
        "expires_at": "2026-09-25T14:20:00",
        "current": true
    },
    {
        "id": "9a3f3c11-5d12-42b1-a4f1-72e7e9d2f810",
        "created_at": "2026-09-17T20:10:00",
        "last_used_at": "2026-09-18T10:12:43",
        "expires_at": "2026-09-24T20:10:00",
        "current": false
    }
]
```

Then:

```http
DELETE /auth/sessions/9a3f3c11-5d12-42b1-a4f1-72e7e9d2f810
Cookie: session=YOUR_CURRENT_SESSION
```

Response:

```http
204 No Content
```

That session can no longer authenticate.

### 5. Important security detail

Notice this filter:

```rust
.filter(sessions::user_id.eq(user_id))
```

That's critical.

Without it, someone who somehow obtained another user's session UUID could potentially attempt to revoke it.

We're enforcing:

```text
authenticated user
       +
their own session ID
       ↓
session can be revoked
```

### 6. What happens if they revoke the current session?

We detect it:

```rust
if current_session_hash.as_deref()
    == Some(target_token_hash.as_str())
```

and remove the browser cookie.

So the endpoint safely supports both:

```text
Revoke another device
        ↓
current browser stays logged in
```

and:

```text
Revoke current session
        ↓
browser is logged out
```

Your session-management API now has:

```text
GET    /auth/sessions
DELETE /auth/sessions/<session_id>
POST   /auth/logout
POST   /auth/logout-all
```

**Next step:** add a proper **session cleanup job** so expired and revoked sessions don't accumulate forever in PostgreSQL.

## Next step: Clean up expired and revoked sessions

Your `sessions` table will grow forever unless we periodically delete old records.

We'll add a reusable cleanup function first.

### 1. Add cleanup function

Create:

`src/auth/session_cleanup.rs`

```rust
use chrono::Utc;
use diesel::prelude::*;

use crate::schema::sessions;

pub fn cleanup_sessions(
    connection: &mut PgConnection,
) -> Result<usize, diesel::result::Error> {
    let now = Utc::now().naive_utc();

    diesel::delete(
        sessions::table
            .filter(
                sessions::expires_at.le(now)
                    .or(
                        sessions::revoked_at
                            .is_not_null()
                    )
            )
    )
    .execute(connection)
}
```

### 2. Export the module

In `src/auth/mod.rs`:

```rust
pub mod session_cleanup;
```

### 3. Add the database connection import

At the top of `session_cleanup.rs`:

```rust
use diesel::PgConnection;
```

So the complete file is:

```rust
use chrono::Utc;
use diesel::{prelude::*, PgConnection};

use crate::schema::sessions;

pub fn cleanup_sessions(
    connection: &mut PgConnection,
) -> Result<usize, diesel::result::Error> {
    let now = Utc::now().naive_utc();

    diesel::delete(
        sessions::table
            .filter(
                sessions::expires_at.le(now)
                    .or(
                        sessions::revoked_at
                            .is_not_null()
                    )
            )
    )
    .execute(connection)
}
```

### 4. Don't delete recently revoked sessions immediately

There's a useful improvement here.

For security auditing, you may want to retain revoked sessions for a short period.

For example:

```text
active
   ↓
revoked
   ↓
retain 30 days
   ↓
delete
```

That means we shouldn't simply delete every `revoked_at IS NOT NULL`.

Change the cleanup logic to:

```rust
pub fn cleanup_sessions(
    connection: &mut PgConnection,
) -> Result<usize, diesel::result::Error> {
    let now = Utc::now().naive_utc();
    let retention_cutoff =
        now - chrono::Duration::days(30);

    diesel::delete(
        sessions::table
            .filter(
                sessions::expires_at.le(now)
                    .or(
                        sessions::revoked_at
                            .lt(retention_cutoff)
                    )
            )
    )
    .execute(connection)
}
```

Now:

```text
Expired session
      ↓
deleted immediately

Revoked session
      ↓
kept for 30 days

Revoked > 30 days
      ↓
deleted
```

### 5. Run cleanup manually first

Before adding an automatic scheduler, expose a temporary development route or simply call the function from a test.

For example:

```rust
let deleted = db
    .run(|connection| {
        cleanup_sessions(connection)
    })
    .await
    .map_err(|error| error.to_string())?;
```

You can log:

```rust
println!(
    "Cleaned up {} sessions",
    deleted
);
```

### 6. Add an index

Your cleanup query will benefit from indexes.

Add a migration:

```sql
CREATE INDEX sessions_expires_at_idx
ON sessions(expires_at);

CREATE INDEX sessions_revoked_at_idx
ON sessions(revoked_at);
```

Down migration:

```sql
DROP INDEX sessions_expires_at_idx;
DROP INDEX sessions_revoked_at_idx;
```

Then run:

```bash
diesel migration run
```

### 7. Your session lifecycle is now

```text
              LOGIN
                │
                ▼
          Create session
                │
                ▼
             ACTIVE
                │
       ┌────────┴────────┐
       │                 │
    expires            logout
       │                 │
       ▼                 ▼
    EXPIRED            REVOKED
       │                 │
       └────────┬────────┘
                │
             cleanup
                │
                ▼
             DELETED
```

This gives you a clean lifecycle without storing authentication records indefinitely.

**Next step:** add a **background cleanup task in Rocket** so this happens automatically rather than requiring a manual call.

Yep — next step: **automatically clean up expired/revoked sessions in the background**.

Rocket 0.5 runs on Tokio, so a periodic async task is a good fit. ([Rocket][1])

### 1. Keep your existing cleanup function

Your `src/auth/session_cleanup.rs` stays:

```rust
use chrono::Utc;
use diesel::{prelude::*, PgConnection};

use crate::schema::sessions;

pub fn cleanup_sessions(
    connection: &mut PgConnection,
) -> Result<usize, diesel::result::Error> {
    let now = Utc::now().naive_utc();

    let retention_cutoff =
        now - chrono::Duration::days(30);

    diesel::delete(
        sessions::table.filter(
            sessions::expires_at
                .le(now)
                .or(
                    sessions::revoked_at
                        .lt(retention_cutoff)
                )
        )
    )
    .execute(connection)
}
```

### 2. Add the background task

Create:

`src/auth/background.rs`

```rust
use std::time::Duration;

use rocket::tokio::time::interval;

use crate::{
    auth::session_cleanup::cleanup_sessions,
    DbConn,
};

pub async fn session_cleanup_loop(
    rocket: rocket::Rocket<rocket::Orbit>,
) {
    let mut timer = interval(Duration::from_secs(60 * 60));

    loop {
        timer.tick().await;

        match DbConn::get_one(&rocket).await {
            Some(mut db) => {
                match db.run(|connection| {
                    cleanup_sessions(connection)
                }).await {
                    Ok(deleted) => {
                        if deleted > 0 {
                            println!(
                                "Session cleanup removed {} sessions",
                                deleted
                            );
                        }
                    }

                    Err(error) => {
                        eprintln!(
                            "Session cleanup failed: {}",
                            error
                        );
                    }
                }
            }

            None => {
                eprintln!(
                    "Session cleanup could not acquire database connection"
                );
            }
        }
    }
}
```

This uses the existing Rocket database pool rather than creating a second PostgreSQL connection. `rocket_sync_db_pools` is specifically designed for synchronous Diesel work behind Rocket's async runtime. ([Rocket][2])

### 3. Export the module

In:

`src/auth/mod.rs`

add:

```rust
pub mod background;
```

So your module list should contain roughly:

```rust
pub mod background;
pub mod cookie;
pub mod csrf;
pub mod guards;
pub mod password;
pub mod rate_limit;
pub mod recovery;
pub mod routes;
pub mod session;
pub mod session_cleanup;
pub mod totp;
```

### 4. Start it from Rocket

This part depends slightly on your existing `main.rs`.

If you currently have something like:

```rust
#[launch]
fn rocket() -> _ {
    rocket::build()
        // ...
}
```

change it to:

```rust
#[launch]
async fn rocket() -> _ {
    let rocket = rocket::build()
        // your existing .manage(...)
        // your existing .attach(...)
        // your existing .mount(...)
        ;

    let cleanup_rocket = rocket.clone();

    rocket::tokio::spawn(async move {
        crate::auth::background::session_cleanup_loop(
            cleanup_rocket
        ).await;
    });

    rocket
}
```

**One caveat:** if your current `#[launch] fn rocket()` setup or your `DbConn` macro-generated API rejects this exact `get_one`/clone arrangement, don't change anything else yet. Paste the compiler error and your current `main.rs`; we'll adapt it to your exact `rocket_sync_db_pools` setup rather than guessing.

### 5. What happens now

Every **1 hour**:

```text
Rocket starts
     ↓
Background Tokio task starts
     ↓
Wait 1 hour
     ↓
Acquire DbConn
     ↓
Delete expired sessions
     ↓
Delete revoked sessions older than 30 days
     ↓
Wait another hour
     ↓
repeat
```

So your `sessions` table won't grow forever.

Also, the cleanup doesn't affect active sessions:

```text
expires_at > now
AND revoked_at IS NULL
        ↓
       KEEP
```

while:

```text
expires_at <= now
        ↓
       DELETE
```

and:

```text
revoked_at < now - 30 days
        ↓
       DELETE
```

### Next

After this, the important security piece is **session rotation**: preventing a stolen session token from remaining valid indefinitely and handling authentication changes safely. That's the next thing I'd implement before moving on to email verification/password reset.

[1]: https://rocket.rs/guide/v0.5/overview/?utm_source=chatgpt.com "Overview - Rocket Web Framework"
[2]: https://rocket.rs/guide/v0.5/state/?utm_source=chatgpt.com "State - Rocket Web Framework"


Next: **session rotation**.

The goal is to rotate the session token after sensitive authentication events. This gives you a fresh opaque token instead of continuing to use the same token indefinitely.

### 1. Add a session rotation helper

Update `src/auth/session.rs`:

```rust
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn generate_challenge_token() -> String {
    generate_token()
}

pub fn generate_session_token() -> (String, String) {
    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);

    (raw_token, token_hash)
}
```

Now instead of repeating:

```rust
let raw_token = generate_token();
let token_hash = hash_token(&raw_token);
```

you can use:

```rust
let (raw_token, token_hash) =
    generate_session_token();
```

---

### 2. Add a function to rotate the current session

Create:

`src/auth/session_rotation.rs`

```rust
use chrono::{Duration, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    auth::{
        cookie::session_cookie,
        session::{generate_session_token, hash_token},
    },
    models::session::NewSession,
    schema::sessions,
    DbConn,
};

pub async fn rotate_session(
    db: &mut DbConn,
    current_token: &str,
    user_id: Uuid,
) -> Result<rocket::http::Cookie<'static>, String> {
    let current_hash = hash_token(current_token);

    let (new_raw_token, new_token_hash) =
        generate_session_token();

    let now = Utc::now().naive_utc();

    let expires_at =
        now + Duration::days(7);

    let new_session = NewSession {
        id: Uuid::new_v4(),
        user_id,
        token_hash: new_token_hash,
        expires_at,
    };

    db.run(move |connection| {
        connection.transaction::<(), String, _>(|connection| {
            let current_session_exists =
                sessions::table
                    .filter(
                        sessions::token_hash
                            .eq(&current_hash)
                    )
                    .filter(
                        sessions::user_id
                            .eq(user_id)
                    )
                    .filter(
                        sessions::revoked_at
                            .is_null()
                    )
                    .filter(
                        sessions::expires_at
                            .gt(now)
                    )
                    .select(sessions::id)
                    .first::<Uuid>(connection)
                    .optional()
                    .map_err(|error| {
                        error.to_string()
                    })?;

            if current_session_exists.is_none() {
                return Err(
                    "Current session is invalid"
                        .to_string()
                );
            }

            diesel::update(
                sessions::table
                    .filter(
                        sessions::token_hash
                            .eq(&current_hash)
                    )
                    .filter(
                        sessions::user_id
                            .eq(user_id)
                    )
                    .filter(
                        sessions::revoked_at
                            .is_null()
                    ),
            )
            .set(
                sessions::revoked_at.eq(now)
            )
            .execute(connection)
            .map_err(|error| {
                error.to_string()
            })?;

            diesel::insert_into(
                sessions::table
            )
            .values(&new_session)
            .execute(connection)
            .map_err(|error| {
                error.to_string()
            })?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(session_cookie(new_raw_token))
}
```

Notice the important part:

```rust
connection.transaction(...)
```

The old session is revoked and the new session is created **atomically**.

If inserting the new session fails, the old session isn't left revoked.

---

### 3. Export the module

In `src/auth/mod.rs`:

```rust
pub mod session_rotation;
```

---

### 4. Add `OptionalExtension`

Because we're using:

```rust
.optional()
```

make sure this import exists:

```rust
use diesel::prelude::*;
```

`OptionalExtension` is included through Diesel's prelude.

---

### 5. Where we'll use rotation

The most important places are:

```text
Password login
     ↓
Password verified
     ↓
2FA required?
     ↓
YES
     ↓
TOTP verified
     ↓
CREATE/ROTATE SESSION
```

and for an already authenticated user:

```text
Authenticated session
     ↓
Sensitive authentication change
     ↓
Rotate session
     ↓
New session cookie
```

This gives us a clean security boundary.

### One important detail

Don't rotate the session on **every normal API request**.

That creates unnecessary database writes and can introduce race conditions when multiple browser requests arrive simultaneously.

Rotate around authentication/security transitions instead.

---

**Next step:** we'll wire this into the **TOTP verification endpoint**, so successful 2FA authentication creates a fresh session and invalidates any previous session associated with that authentication flow.


Next: **wire session rotation into successful TOTP login**.

Right now your `/auth/2fa/verify` creates a session directly. We'll change it so the successful 2FA step creates a fresh authenticated session and consumes the challenge atomically.

### 1. Update `src/auth/routes.rs`

Replace your current TOTP verification route with this:

```rust
#[post("/2fa/verify", data = "<request>")]
pub async fn verify_two_factor(
    mut db: DbConn,
    request: Json<TwoFactorVerifyRequest>,
) -> Result<(Cookie<'static>, Json<LoginResponse>), String> {
    let challenge_hash =
        hash_token(&request.challenge_token);

    let now = Utc::now().naive_utc();

    let challenge = db
        .run(move |connection| {
            two_factor_challenges::table
                .filter(
                    two_factor_challenges::token_hash
                        .eq(&challenge_hash)
                )
                .filter(
                    two_factor_challenges::expires_at
                        .gt(now)
                )
                .first::<TwoFactorChallenge>(
                    connection
                )
        })
        .await
        .map_err(|_| {
            "Invalid or expired challenge".to_string()
        })?;

    let user_id = challenge.user_id;

    let secret = db
        .run(move |connection| {
            two_factor_secrets::table
                .filter(
                    two_factor_secrets::user_id
                        .eq(user_id)
                )
                .filter(
                    two_factor_secrets::confirmed_at
                        .is_not_null()
                )
                .select(
                    two_factor_secrets::secret
                )
                .first::<String>(connection)
        })
        .await
        .map_err(|_| {
            "Two-factor authentication is not configured"
                .to_string()
        })?;

    let totp =
        create_totp_from_secret(&secret)?;

    let valid = totp
        .check(&request.code)
        .map_err(|error| error.to_string())?;

    if !valid {
        return Err(
            "Invalid authentication code".to_string()
        );
    }

    let (raw_token, token_hash) =
        generate_session_token();

    let session_expires_at =
        now + Duration::days(7);

    let new_session = NewSession {
        id: Uuid::new_v4(),
        user_id,
        token_hash,
        expires_at: session_expires_at,
    };

    let challenge_id = challenge.id;

    db.run(move |connection| {
        connection.transaction::<(), String, _>(
            |connection| {
                /*
                 * Re-check the challenge inside the
                 * transaction to prevent replay.
                 */
                let challenge_exists =
                    two_factor_challenges::table
                        .filter(
                            two_factor_challenges::id
                                .eq(challenge_id)
                        )
                        .filter(
                            two_factor_challenges::expires_at
                                .gt(now)
                        )
                        .select(
                            two_factor_challenges::id
                        )
                        .first::<Uuid>(connection)
                        .optional()
                        .map_err(|error| {
                            error.to_string()
                        })?;

                if challenge_exists.is_none() {
                    return Err(
                        "Invalid or expired challenge"
                            .to_string()
                    );
                }

                diesel::insert_into(
                    sessions::table
                )
                .values(&new_session)
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                diesel::delete(
                    two_factor_challenges::table
                        .filter(
                            two_factor_challenges::id
                                .eq(challenge_id)
                        )
                )
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                Ok(())
            }
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    let cookie =
        session_cookie(raw_token);

    Ok((
        cookie,
        Json(LoginResponse {
            message: "Login successful".to_string(),
            requires_2fa: false,
            challenge_token: None,
        }),
    ))
}
```

### 2. Make sure these imports exist

At the top of `routes.rs`:

```rust
use chrono::{Duration, Utc};
use diesel::prelude::*;
use rocket::{
    http::{Cookie, Status},
    serde::json::Json,
};
use uuid::Uuid;

use crate::{
    auth::{
        cookie::session_cookie,
        session::{
            generate_session_token,
            hash_token,
        },
        totp::create_totp_from_secret,
    },
    models::{
        session::NewSession,
        two_factor_challenge::TwoFactorChallenge,
    },
    schema::{
        sessions,
        two_factor_challenges,
        two_factor_secrets,
    },
    DbConn,
};
```

Adjust the model module path if your `models/mod.rs` exposes the challenge model differently.

### 3. Why the second challenge check matters

We first check:

```text
challenge token
      ↓
find challenge
      ↓
check expiration
      ↓
load TOTP secret
      ↓
check TOTP
      ↓
transaction
      ↓
check challenge AGAIN
      ↓
create session
      ↓
delete challenge
```

That second check is important.

Imagine the browser sends the same challenge twice at nearly the same time:

```text
Request A ───────┐
                 ├── same challenge
Request B ───────┘
```

Only the transaction that successfully consumes the challenge should be allowed to finish authentication.

After successful verification:

```text
two_factor_challenges
        ↓
     DELETED
```

Therefore the challenge becomes **single-use**.

### 4. Your authentication flow is now

```text
POST /auth/login
       │
       ├── password invalid
       │       └── reject
       │
       ├── 2FA disabled
       │       └── create session
       │
       └── 2FA enabled
               │
               ▼
       create challenge
               │
               ▼
       return challenge_token
               │
               ▼
       POST /auth/2fa/verify
               │
               ├── challenge valid?
               ├── challenge unexpired?
               ├── TOTP secret exists?
               ├── TOTP code valid?
               │
               ▼
          transaction
               │
               ├── create session
               └── consume challenge
               │
               ▼
          session cookie
```

One subtle point: this is **session creation after 2FA**, rather than literally rotating an already authenticated session, because the user doesn't have an authenticated session before completing 2FA.

Your `rotate_session()` helper will become useful for **password changes, email changes, enabling/disabling security settings, and other privilege-sensitive transitions**.

### Next

We'll implement **password change + session invalidation/rotation**. This is an important piece because changing a password should deal with existing sessions rather than leaving old authenticated sessions untouched.

Next: **change password securely and invalidate existing sessions**.

For a password change, we'll require the current password, verify the authenticated session, update the Argon2id hash, and revoke the user's other sessions.

### 1. Add the request type

In `src/auth/routes.rs`:

```rust
#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}
```

### 2. Add the password-change route

```rust
#[post("/password/change", data = "<request>")]
pub async fn change_password(
    mut db: DbConn,
    user: AuthenticatedUser,
    _csrf: CsrfProtected,
    cookies: &CookieJar<'_>,
    request: Json<ChangePasswordRequest>,
) -> Result<Status, String> {
    if request.new_password.len() < 12 {
        return Err(
            "New password must be at least 12 characters"
                .to_string()
        );
    }

    if request.current_password == request.new_password {
        return Err(
            "New password must be different from current password"
                .to_string()
        );
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
        .map_err(|_| {
            "Unable to load account".to_string()
        })?;

    let valid = verify_password(
        &request.current_password,
        &password_hash,
    )?;

    if !valid {
        return Err(
            "Current password is incorrect".to_string()
        );
    }

    let new_password_hash =
        hash_password(&request.new_password)?;

    let current_session_hash = cookies
        .get("session")
        .map(|cookie| {
            hash_token(cookie.value())
        });

    let now = Utc::now().naive_utc();

    db.run(move |connection| {
        connection.transaction::<(), String, _>(
            |connection| {
                diesel::update(
                    users::table
                        .filter(users::id.eq(user_id))
                )
                .set((
                    users::password_hash
                        .eq(&new_password_hash),
                    users::updated_at.eq(now),
                ))
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                /*
                 * Revoke every existing session.
                 */
                diesel::update(
                    sessions::table
                        .filter(
                            sessions::user_id
                                .eq(user_id)
                        )
                        .filter(
                            sessions::revoked_at
                                .is_null()
                        )
                )
                .set(
                    sessions::revoked_at.eq(now)
                )
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                Ok(())
            }
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    cookies.remove(
        remove_session_cookie()
    );

    Ok(Status::NoContent)
}
```

### 3. Why revoke all sessions?

Suppose the user has:

```text
Laptop ───── session A
Phone  ───── session B
Tablet ───── session C
```

Then the password is changed.

If we leave those sessions alive:

```text
Password changed
      │
      ├── session A → still valid
      ├── session B → still valid
      └── session C → still valid
```

The password change wouldn't terminate existing authenticated access.

Instead:

```text
Password changed
      │
      ├── session A → revoked
      ├── session B → revoked
      └── session C → revoked
```

The current browser's cookie is also removed:

```rust
cookies.remove(
    remove_session_cookie()
);
```

So the user must authenticate again with the new password.

### 4. Add the route to your mount

Where you currently mount your authentication routes:

```rust
.mount(
    "/auth",
    routes![
        register,
        login,
        logout,
        logout_all,
        list_sessions,
        revoke_session,
        setup_2fa,
        confirm_2fa,
        verify_two_factor,
        verify_recovery_code,
        disable_two_factor,
        regenerate_recovery_codes,
        csrf_token,
        change_password,
    ]
)
```

If you're using a different route list, just add:

```rust
change_password,
```

### 5. Add the required imports

Make sure `routes.rs` has:

```rust
use rocket::http::{
    CookieJar,
    Status,
};
```

and your auth imports include:

```rust
use crate::auth::{
    csrf::CsrfProtected,
    cookie::remove_session_cookie,
    password::{
        hash_password,
        verify_password,
    },
    session::hash_token,
};
```

### 6. One improvement to your existing session listing

After password change, the database may temporarily contain revoked sessions until the cleanup task removes them.

That's fine because your active-session query already has:

```rust
.filter(
    sessions::revoked_at.is_null()
)
```

So revoked sessions aren't returned as active.

---

### Security flow now

Your account security flow is becoming:

```text
                    AUTHENTICATED USER
                           │
          ┌────────────────┼────────────────┐
          │                │                │
       Logout        Change password     2FA
          │                │                │
       revoke          update hash       verify
       session          revoke all          │
          │                │                 ▼
          ▼                ▼            create session
      cookie gone      cookie gone
```

One thing we'll improve next: **password policy and password-change rate limiting**. We don't want an arbitrary 12-character check to be the only validation, and sensitive authentication endpoints should have their own rate limits rather than sharing one broad limiter.

Next: **separate rate limits for login, 2FA, and recovery codes**.

Your current limiter uses one global bucket. That means five failed login attempts could interfere with 2FA attempts. We want independent limits.

### 1. Replace `RateLimiter`

Update `src/auth/rate_limit.rs`:

```rust
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, AttemptState>>,
    max_attempts: u32,
    window: Duration,
}

struct AttemptState {
    count: u32,
    window_started: Instant,
}

impl RateLimiter {
    pub fn new(
        max_attempts: u32,
        window: Duration,
    ) -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            max_attempts,
            window,
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        let now = Instant::now();

        let state = attempts
            .entry(key.to_string())
            .or_insert(AttemptState {
                count: 0,
                window_started: now,
            });

        if now.duration_since(
            state.window_started
        ) >= self.window {
            state.count = 0;
            state.window_started = now;
        }

        if state.count >= self.max_attempts {
            return false;
        }

        state.count += 1;

        true
    }

    pub fn reset(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        attempts.remove(key);
    }
}
```

The implementation itself doesn't need to change much. The important change is **how we construct the key**.

---

## 2. Create an authentication-specific guard

In `src/auth/rate_limit.rs`, add:

```rust
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};

pub struct AuthRateLimit {
    pub key: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        let ip = match request.client_ip() {
            Some(ip) => ip.to_string(),
            None => {
                return Outcome::Error((
                    Status::BadRequest,
                    (),
                ));
            }
        };

        let limiter = match request
            .rocket()
            .state::<RateLimiter>()
        {
            Some(limiter) => limiter,

            None => {
                return Outcome::Error((
                    Status::InternalServerError,
                    (),
                ));
            }
        };

        let key = format!(
            "auth:ip:{}",
            ip
        );

        if !limiter.check(&key) {
            return Outcome::Error((
                Status::TooManyRequests,
                (),
            ));
        }

        Outcome::Success(
            AuthRateLimit { key }
        )
    }
}
```

---

## 3. Give each authentication operation its own namespace

We'll use keys like:

```text
auth:login:ip:1.2.3.4
auth:2fa:ip:1.2.3.4
auth:recovery:ip:1.2.3.4
```

Add this helper:

```rust
pub fn rate_limit_key(
    operation: &str,
    ip: &str,
) -> String {
    format!(
        "auth:{}:ip:{}",
        operation,
        ip
    )
}
```

Then the limiter can distinguish:

```text
login attempts
       ↓
auth:login:ip:...

2FA attempts
       ↓
auth:2fa:ip:...

Recovery attempts
       ↓
auth:recovery:ip:...
```

---

## 4. Better login protection

Your login route should eventually look like:

```rust
#[post("/login", data = "<request>")]
pub async fn login(
    mut db: DbConn,
    request: Json<LoginRequest>,
    rate_limit: AuthRateLimit,
) -> Result<..., String> {
```

But there's a problem: `AuthRateLimit` currently always uses the same `"auth:ip"` namespace.

So let's make the guard generic through a request header.

For now, a cleaner approach is to create three guards.

### Login limiter

```rust
pub struct LoginRateLimit;

#[rocket::async_trait]
impl<'r> FromRequest<'r>
    for LoginRateLimit
{
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        check_operation(
            request,
            "login",
        )
    }
}
```

### 2FA limiter

```rust
pub struct TwoFactorRateLimit;

#[rocket::async_trait]
impl<'r> FromRequest<'r>
    for TwoFactorRateLimit
{
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        check_operation(
            request,
            "2fa",
        )
    }
}
```

### Recovery limiter

```rust
pub struct RecoveryRateLimit;

#[rocket::async_trait]
impl<'r> FromRequest<'r>
    for RecoveryRateLimit
{
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        check_operation(
            request,
            "recovery",
        )
    }
}
```

And the shared function:

```rust
fn check_operation(
    request: &Request<'_>,
    operation: &str,
) -> Outcome<
    impl Sized,
    (),
> {
    let ip = match request.client_ip() {
        Some(ip) => ip.to_string(),

        None => {
            return Outcome::Error((
                Status::BadRequest,
                (),
            ));
        }
    };

    let limiter = match request
        .rocket()
        .state::<RateLimiter>()
    {
        Some(limiter) => limiter,

        None => {
            return Outcome::Error((
                Status::InternalServerError,
                (),
            ));
        }
    };

    let key = rate_limit_key(
        operation,
        &ip,
    );

    if !limiter.check(&key) {
        return Outcome::Error((
            Status::TooManyRequests,
            (),
        ));
    }

    Outcome::Success(())
}
```

However, Rust's `impl Sized` return here won't work cleanly with the different guard types. **Don't use this exact helper yet.** Instead, we'll keep the shared function returning a boolean:

```rust
fn check_operation(
    request: &Request<'_>,
    operation: &str,
) -> bool {
    let ip = match request.client_ip() {
        Some(ip) => ip.to_string(),
        None => return false,
    };

    let limiter = match request
        .rocket()
        .state::<RateLimiter>()
    {
        Some(limiter) => limiter,
        None => return false,
    };

    let key = rate_limit_key(
        operation,
        &ip,
    );

    limiter.check(&key)
}
```

Then the guards become:

```rust
pub struct LoginRateLimit;

#[rocket::async_trait]
impl<'r> FromRequest<'r>
    for LoginRateLimit
{
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_operation(
            request,
            "login",
        ) {
            Outcome::Success(
                LoginRateLimit
            )
        } else {
            Outcome::Error((
                Status::TooManyRequests,
                (),
            ))
        }
    }
}
```

Do the same pattern for `TwoFactorRateLimit` and `RecoveryRateLimit`.

---

## 5. Apply the guards

Login:

```rust
#[post("/login", data = "<request>")]
pub async fn login(
    mut db: DbConn,
    request: Json<LoginRequest>,
    _rate_limit: LoginRateLimit,
) -> Result<..., String> {
```

TOTP:

```rust
#[post("/2fa/verify", data = "<request>")]
pub async fn verify_two_factor(
    mut db: DbConn,
    request: Json<TwoFactorVerifyRequest>,
    _rate_limit: TwoFactorRateLimit,
) -> Result<..., String> {
```

Recovery:

```rust
#[post("/2fa/recovery", data = "<request>")]
pub async fn verify_recovery_code(
    mut db: DbConn,
    request: Json<RecoveryCodeVerifyRequest>,
    _rate_limit: RecoveryRateLimit,
) -> Result<..., String> {
```

---

## 6. Important production improvement

The current:

```text
HashMap + Mutex
```

works for learning and a single application process.

It does **not** provide a shared rate limit when you have:

```text
           Load Balancer
          /      |      \
       App 1   App 2   App 3
```

Each process would have its own counter.

Later we'll move this to a shared store such as Redis if you deploy multiple instances.

---

### Current authentication architecture

You now have:

```text
                    AUTH SYSTEM
                         │
        ┌────────────────┼────────────────┐
        │                │                │
     Password          Session           2FA
        │                │                │
      Argon2id       opaque token       TOTP
                         │                │
                    SHA-256 hash       recovery
                         │                │
                    HttpOnly cookie    single-use
                         │
                    session listing
                         │
                    session revoke
                         │
                    auto cleanup
```

And authentication attacks are now rate-limited independently.

**Next:** we'll add **email verification**, including a single-use expiring verification token and the database model for it.

Next: **email verification**.

We'll add it as a separate authentication flow:

```text
Register
   ↓
Create user
   ↓
Create verification token
   ↓
Send email
   ↓
User clicks verification link
   ↓
Validate token + expiration
   ↓
Mark email verified
   ↓
Consume token
```

## 1. Add `email_verified_at` to `users`

Create a new Diesel migration:

```bash
diesel migration generate add_email_verified_at_to_users
```

`up.sql`:

```sql
ALTER TABLE users
ADD COLUMN email_verified_at TIMESTAMP NULL;
```

`down.sql`:

```sql
ALTER TABLE users
DROP COLUMN email_verified_at;
```

Run:

```bash
diesel migration run
```

Then regenerate:

```bash
diesel print-schema > src/schema.rs
```

Your `users` table should now contain:

```rust
email_verified_at -> Nullable<Timestamp>,
```

---

## 2. Update `User`

In `src/models/user.rs`:

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use serde::Serialize;
use uuid::Uuid;

use crate::schema::users;

#[derive(Queryable, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,

    #[serde(skip_serializing)]
    pub password_hash: String,

    pub two_factor_enabled: bool,

    pub email_verified_at: Option<NaiveDateTime>,

    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
}
```

---

# 3. Create verification-token table

Generate another migration:

```bash
diesel migration generate create_email_verification_tokens
```

`up.sql`:

```sql
CREATE TABLE email_verification_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    used_at TIMESTAMP NULL
);

CREATE INDEX email_verification_tokens_user_id_idx
ON email_verification_tokens(user_id);

CREATE INDEX email_verification_tokens_expires_at_idx
ON email_verification_tokens(expires_at);
```

`down.sql`:

```sql
DROP TABLE email_verification_tokens;
```

Run:

```bash
diesel migration run
```

Then:

```bash
diesel print-schema > src/schema.rs
```

---

# 4. Create the model

Create:

`src/models/email_verification.rs`

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::email_verification_tokens;

#[derive(Queryable)]
pub struct EmailVerificationToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub used_at: Option<NaiveDateTime>,
}

#[derive(Insertable)]
#[diesel(table_name = email_verification_tokens)]
pub struct NewEmailVerificationToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
}
```

---

# 5. Export the model

In `src/models/mod.rs`:

```rust
pub mod email_verification;
pub mod session;
pub mod two_factor_challenge;
pub mod two_factor_secret;
pub mod recovery_code;
pub mod user;
```

Use your existing module names if they differ.

---

# 6. Create token helpers

Create:

`src/auth/email_verification.rs`

```rust
use crate::auth::session::{
    generate_token,
    hash_token,
};

pub fn generate_verification_token()
    -> (String, String)
{
    let raw_token = generate_token();

    let token_hash =
        hash_token(&raw_token);

    (raw_token, token_hash)
}
```

The database only receives:

```text
token_hash
```

The raw token is what eventually goes into the email link.

So the database never needs to contain the usable verification token.

---

# 7. Add verification-token creation to registration

Your registration route currently creates the user.

After inserting the user, create a verification token.

Add these imports:

```rust
use chrono::{Duration, Utc};
use crate::{
    auth::email_verification::generate_verification_token,
    models::email_verification::NewEmailVerificationToken,
    schema::email_verification_tokens,
};
```

Then after the user has been inserted:

```rust
let (raw_token, token_hash) =
    generate_verification_token();

let now =
    Utc::now().naive_utc();

let verification_token =
    NewEmailVerificationToken {
        id: Uuid::new_v4(),
        user_id,
        token_hash,
        expires_at:
            now + Duration::hours(24),
    };
```

Insert it:

```rust
db.run(move |connection| {
    diesel::insert_into(
        email_verification_tokens::table
    )
    .values(&verification_token)
    .execute(connection)
})
.await
.map_err(|error| error.to_string())?;
```

At this point you have:

```text
users
 └── user_id

email_verification_tokens
 ├── user_id
 ├── token_hash
 ├── expires_at
 └── used_at
```

---

# 8. Don't return the token from the API

For development, you might be tempted to return:

```json
{
  "verification_token": "..."
}
```

Don't build your real API around that.

The intended flow is:

```text
Backend
   │
   ├── generates raw token
   │
   ├── hashes token
   │
   ├── stores hash
   │
   └── sends raw token to email service
```

Eventually you'll have something like:

```text
https://your-domain.com/verify-email?token=<token>
```

The frontend can then call the backend with that token.

---

# 9. Add the verification request

In `src/auth/routes.rs`:

```rust
#[derive(Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}
```

---

# 10. Add the verification endpoint

```rust
#[post("/verify-email", data = "<request>")]
pub async fn verify_email(
    mut db: DbConn,
    request: Json<VerifyEmailRequest>,
) -> Result<Status, String> {
    let token_hash =
        hash_token(&request.token);

    let now =
        Utc::now().naive_utc();

    let token = db
        .run(move |connection| {
            email_verification_tokens::table
                .filter(
                    email_verification_tokens::token_hash
                        .eq(&token_hash)
                )
                .filter(
                    email_verification_tokens::used_at
                        .is_null()
                )
                .filter(
                    email_verification_tokens::expires_at
                        .gt(now)
                )
                .first::<EmailVerificationToken>(
                    connection
                )
        })
        .await
        .map_err(|_| {
            "Invalid or expired verification token"
                .to_string()
        })?;

    let user_id =
        token.user_id;

    let token_id =
        token.id;

    db.run(move |connection| {
        connection.transaction::<(), String, _>(
            |connection| {
                let updated =
                    diesel::update(
                        users::table
                            .filter(
                                users::id
                                    .eq(user_id)
                            )
                            .filter(
                                users::email_verified_at
                                    .is_null()
                            )
                    )
                    .set(
                        users::email_verified_at
                            .eq(now)
                    )
                    .execute(connection)
                    .map_err(|error| {
                        error.to_string()
                    })?;

                if updated != 1 {
                    return Err(
                        "Email is already verified"
                            .to_string()
                    );
                }

                diesel::update(
                    email_verification_tokens::table
                        .filter(
                            email_verification_tokens::id
                                .eq(token_id)
                        )
                        .filter(
                            email_verification_tokens::used_at
                                .is_null()
                        )
                )
                .set(
                    email_verification_tokens::used_at
                        .eq(now)
                )
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                Ok(())
            }
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Status::NoContent)
}
```

The transaction makes the verification operation atomic.

---

# 11. Add the imports

At the top of `routes.rs`:

```rust
use crate::{
    models::email_verification::{
        EmailVerificationToken,
        NewEmailVerificationToken,
    },
    schema::{
        email_verification_tokens,
        users,
    },
};
```

And:

```rust
use crate::auth::session::hash_token;
```

You already have `Utc` and `Status` from the previous routes.

---

# 12. Mount the endpoint

Add:

```rust
verify_email,
```

to your authentication route list.

Your flow is now:

```text
POST /auth/register
        │
        ▼
Create user
        │
        ▼
Create 24-hour verification token
        │
        ▼
Send email
        │
        ▼
POST /auth/verify-email
        │
        ├── token missing → reject
        ├── token expired → reject
        ├── token already used → reject
        │
        ▼
Transaction
        │
        ├── email_verified_at = now
        └── used_at = now
```

### Important

Don't automatically log the user in merely because they verified their email. Keep email verification and authentication as separate concerns.

Also, **registration should eventually require rate limiting**, just like login and 2FA.

Next we'll implement **resend verification email**, including invalidating the previous verification tokens and preventing someone from hammering the endpoint.

Next: **resend email verification securely**.

We want this endpoint to:

* Require the user's current authenticated session.
* Refuse if the email is already verified.
* Invalidate previous verification tokens.
* Create a fresh token valid for 24 hours.
* Rate-limit requests.
* Never return the raw token in the API response.

## 1. Add a resend request

No request body is necessary.

In `src/auth/routes.rs`, add:

```rust id="48173"
#[derive(Serialize)]
pub struct ResendVerificationResponse {
    pub message: String,
}
```

## 2. Add the route

```rust id="72614"
#[post("/verify-email/resend")]
pub async fn resend_verification_email(
    mut db: DbConn,
    user: AuthenticatedUser,
    _csrf: CsrfProtected,
    _rate_limit: LoginRateLimit,
) -> Result<Json<ResendVerificationResponse>, String> {
    let user_id = user.id;

    let email_verified = db
        .run(move |connection| {
            users::table
                .filter(users::id.eq(user_id))
                .select(users::email_verified_at)
                .first::<Option<NaiveDateTime>>(
                    connection
                )
        })
        .await
        .map_err(|error| error.to_string())?;

    if email_verified.is_some() {
        return Ok(Json(
            ResendVerificationResponse {
                message:
                    "Email is already verified"
                        .to_string(),
            }
        ));
    }

    let (raw_token, token_hash) =
        generate_verification_token();

    let now =
        Utc::now().naive_utc();

    let expires_at =
        now + Duration::hours(24);

    let new_token =
        NewEmailVerificationToken {
            id: Uuid::new_v4(),
            user_id,
            token_hash,
            expires_at,
        };

    db.run(move |connection| {
        connection.transaction::<(), String, _>(
            |connection| {
                /*
                 * Invalidate all previous tokens.
                 */
                diesel::update(
                    email_verification_tokens::table
                        .filter(
                            email_verification_tokens::user_id
                                .eq(user_id)
                        )
                        .filter(
                            email_verification_tokens::used_at
                                .is_null()
                        )
                )
                .set(
                    email_verification_tokens::used_at
                        .eq(now)
                )
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                /*
                 * Insert the new token.
                 */
                diesel::insert_into(
                    email_verification_tokens::table
                )
                .values(&new_token)
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                Ok(())
            }
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    /*
     * For now, just demonstrate where the
     * email delivery will happen.
     *
     * Do NOT return raw_token to the client.
     */
    println!(
        "Verification email token generated: {}",
        raw_token
    );

    Ok(Json(
        ResendVerificationResponse {
            message:
                "If your email is not verified, a new verification email has been sent"
                    .to_string(),
        }
    ))
}
```

### 3. Add the imports

Make sure `routes.rs` has:

```rust id="x8k7v3"
use chrono::{
    Duration,
    NaiveDateTime,
    Utc,
};
```

And:

```rust id="zj6e8m"
use crate::auth::{
    email_verification::generate_verification_token,
    csrf::CsrfProtected,
    guards::AuthenticatedUser,
    rate_limit::LoginRateLimit,
};
```

You can rename the rate-limit guard later to something more appropriate such as `VerificationRateLimit`. For now, don't create duplicate limiter code unnecessarily.

---

## 4. Don't log the token in production

This:

```rust id="6ql2ag"
println!(
    "Verification email token generated: {}",
    raw_token
);
```

is only useful while developing.

Remove it before production.

The real flow will become:

```text
generate raw token
       ↓
hash token
       ↓
store hash
       ↓
email service
       ↓
send raw token in HTTPS link
```

The API response remains:

```json id="g6yyku"
{
  "message": "If your email is not verified, a new verification email has been sent"
}
```

That generic response is intentional. It avoids exposing unnecessary account state.

---

## 5. Mount the endpoint

Add:

```rust id="7z0j5k"
resend_verification_email,
```

to your `routes![]`.

---

## 6. Why invalidate the old token?

Suppose the user requests twice:

```text
Request #1
    ↓
Token A

Request #2
    ↓
Token B
```

After request #2:

```text
Token A → used_at = now → invalid
Token B → active        → valid
```

Only the newest verification link works.

This prevents multiple outstanding verification credentials from accumulating.

---

## 7. One change before we move on

The current rate limiter allows:

```text
5 requests / 60 seconds
```

per IP.

For email verification, that's a little crude. A better production design is to combine:

```text
IP address
+
authenticated user ID
```

so one user cannot repeatedly trigger email delivery from a single account while still avoiding overly broad IP blocking.

We'll address that when we build the actual **email delivery layer**.

Your authentication system now has:

```text
Registration
     ↓
Email verification
     ↓
Password authentication
     ↓
2FA challenge
     ↓
TOTP / recovery code
     ↓
Authenticated session
     ↓
Session listing
     ↓
Session revocation
     ↓
Automatic session cleanup
```

**Next: password reset ("forgot password")**, using a separate single-use, expiring reset token.

Next: **forgot-password / password reset**.

This flow should be completely separate from the authenticated password-change flow we built earlier.

```text
POST /auth/password/forgot
        ↓
Find account
        ↓
Create reset token
        ↓
Send email
        ↓
User clicks link
        ↓
POST /auth/password/reset
        ↓
Validate token
        ↓
Set new password
        ↓
Invalidate token
        ↓
Revoke existing sessions
```

## 1. Create the migration

Run:

```bash
diesel migration generate create_password_reset_tokens
```

`up.sql`:

```sql
CREATE TABLE password_reset_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    used_at TIMESTAMP NULL
);

CREATE INDEX password_reset_tokens_user_id_idx
ON password_reset_tokens(user_id);

CREATE INDEX password_reset_tokens_expires_at_idx
ON password_reset_tokens(expires_at);
```

`down.sql`:

```sql
DROP TABLE password_reset_tokens;
```

Run:

```bash
diesel migration run
```

Then:

```bash
diesel print-schema > src/schema.rs
```

---

## 2. Create the model

Create:

`src/models/password_reset.rs`

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::password_reset_tokens;

#[derive(Queryable)]
pub struct PasswordResetToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub used_at: Option<NaiveDateTime>,
}

#[derive(Insertable)]
#[diesel(table_name = password_reset_tokens)]
pub struct NewPasswordResetToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
}
```

---

## 3. Export the model

In `src/models/mod.rs`:

```rust
pub mod password_reset;
```

---

# 4. Forgot-password request

Add to `src/auth/routes.rs`:

```rust
#[derive(Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}
```

And response:

```rust
#[derive(Serialize)]
pub struct ForgotPasswordResponse {
    pub message: String,
}
```

---

# 5. Create the forgot-password endpoint

```rust
#[post("/password/forgot", data = "<request>")]
pub async fn forgot_password(
    mut db: DbConn,
    _rate_limit: LoginRateLimit,
    request: Json<ForgotPasswordRequest>,
) -> Result<Json<ForgotPasswordResponse>, String> {
    let email =
        request.email.trim().to_lowercase();

    let user = db
        .run(move |connection| {
            users::table
                .filter(users::email.eq(email))
                .select((
                    users::id,
                    users::email_verified_at,
                ))
                .first::<(
                    Uuid,
                    Option<NaiveDateTime>,
                )>(connection)
                .optional()
        })
        .await
        .map_err(|error| error.to_string())?;

    /*
     * Always return the same response,
     * whether the account exists or not.
     */
    let Some((user_id, _email_verified_at)) = user
    else {
        return Ok(Json(
            ForgotPasswordResponse {
                message:
                    "If an account exists for that email, a password reset link has been sent."
                        .to_string(),
            }
        ));
    };

    let (raw_token, token_hash) =
        generate_token_pair();

    let now =
        Utc::now().naive_utc();

    let new_token =
        NewPasswordResetToken {
            id: Uuid::new_v4(),
            user_id,
            token_hash,
            expires_at:
                now + Duration::minutes(30),
        };

    db.run(move |connection| {
        connection.transaction::<(), String, _>(
            |connection| {
                /*
                 * Invalidate previous reset tokens.
                 */
                diesel::update(
                    password_reset_tokens::table
                        .filter(
                            password_reset_tokens::user_id
                                .eq(user_id)
                        )
                        .filter(
                            password_reset_tokens::used_at
                                .is_null()
                        )
                )
                .set(
                    password_reset_tokens::used_at
                        .eq(now)
                )
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                diesel::insert_into(
                    password_reset_tokens::table
                )
                .values(&new_token)
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                Ok(())
            }
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    /*
     * Temporary development output.
     *
     * Replace this with your email service.
     */
    println!(
        "Password reset token: {}",
        raw_token
    );

    Ok(Json(
        ForgotPasswordResponse {
            message:
                "If an account exists for that email, a password reset link has been sent."
                    .to_string(),
        }
    ))
}
```

---

# 6. Add a token helper

In `src/auth/session.rs`, add:

```rust
pub fn generate_token_pair() -> (String, String) {
    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);

    (raw_token, token_hash)
}
```

You could technically reuse `generate_session_token()`, but `generate_token_pair()` makes the purpose clearer because this token isn't a session.

---

# 7. Why the response is deliberately vague

Don't return:

```json
{
    "message": "No user exists with that email"
}
```

because an attacker could use the endpoint to discover registered accounts.

Instead, always return:

```json
{
    "message": "If an account exists for that email, a password reset link has been sent."
}
```

So:

```text
existing@example.com
        ↓
reset token created
        ↓
same response

doesnotexist@example.com
        ↓
no token
        ↓
same response
```

This prevents straightforward account enumeration through the endpoint.

---

# 8. Password reset request

Add:

```rust
#[derive(Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}
```

---

# 9. Implement password reset

```rust
#[post("/password/reset", data = "<request>")]
pub async fn reset_password(
    mut db: DbConn,
    _rate_limit: LoginRateLimit,
    request: Json<ResetPasswordRequest>,
) -> Result<Status, String> {
    if request.new_password.len() < 12 {
        return Err(
            "New password must be at least 12 characters"
                .to_string()
        );
    }

    let token_hash =
        hash_token(&request.token);

    let now =
        Utc::now().naive_utc();

    let reset_token = db
        .run(move |connection| {
            password_reset_tokens::table
                .filter(
                    password_reset_tokens::token_hash
                        .eq(&token_hash)
                )
                .filter(
                    password_reset_tokens::used_at
                        .is_null()
                )
                .filter(
                    password_reset_tokens::expires_at
                        .gt(now)
                )
                .first::<PasswordResetToken>(
                    connection
                )
        })
        .await
        .map_err(|_| {
            "Invalid or expired reset token"
                .to_string()
        })?;

    let user_id =
        reset_token.user_id;

    let token_id =
        reset_token.id;

    let new_password_hash =
        hash_password(&request.new_password)?;

    db.run(move |connection| {
        connection.transaction::<(), String, _>(
            |connection| {
                /*
                 * Update password.
                 */
                diesel::update(
                    users::table
                        .filter(
                            users::id.eq(user_id)
                        )
                )
                .set((
                    users::password_hash
                        .eq(&new_password_hash),
                    users::updated_at.eq(now),
                ))
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                /*
                 * Revoke every existing session.
                 */
                diesel::update(
                    sessions::table
                        .filter(
                            sessions::user_id
                                .eq(user_id)
                        )
                        .filter(
                            sessions::revoked_at
                                .is_null()
                        )
                )
                .set(
                    sessions::revoked_at.eq(now)
                )
                .execute(connection)
                .map_err(|error| {
                    error.to_string()
                })?;

                /*
                 * Consume reset token.
                 */
                let updated =
                    diesel::update(
                        password_reset_tokens::table
                            .filter(
                                password_reset_tokens::id
                                    .eq(token_id)
                            )
                            .filter(
                                password_reset_tokens::used_at
                                    .is_null()
                            )
                    )
                    .set(
                        password_reset_tokens::used_at
                            .eq(now)
                    )
                    .execute(connection)
                    .map_err(|error| {
                        error.to_string()
                    })?;

                if updated != 1 {
                    return Err(
                        "Reset token has already been used"
                            .to_string()
                    );
                }

                Ok(())
            }
        )
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Status::NoContent)
}
```

---

# 10. Add the required imports

Make sure you have:

```rust
use chrono::{
    Duration,
    NaiveDateTime,
    Utc,
};

use diesel::prelude::*;
```

And:

```rust
use crate::{
    auth::{
        password::hash_password,
        session::hash_token,
    },
    models::password_reset::{
        NewPasswordResetToken,
        PasswordResetToken,
    },
    schema::{
        password_reset_tokens,
        sessions,
        users,
    },
};
```

---

# 11. Mount both endpoints

Add:

```rust
forgot_password,
reset_password,
```

to your `routes![]`.

Your authentication API now has:

```text
/auth/register

/auth/login

/auth/logout
/auth/logout-all

/auth/sessions
/auth/sessions/<id>

/auth/2fa/setup
/auth/2fa/verify
/auth/2fa/recovery
/auth/2fa/disable
/auth/2fa/recovery-codes/regenerate

/auth/verify-email
/auth/verify-email/resend

/auth/password/change
/auth/password/forgot
/auth/password/reset

/auth/csrf
```

---

## 12. Security behavior after reset

The important part is that a successful password reset does **not** simply change the password.

It does this atomically:

```text
Password reset
      │
      ▼
Update password hash
      │
      ├── Revoke session A
      ├── Revoke session B
      ├── Revoke session C
      │
      ▼
Consume reset token
```

So a previously stolen session cannot remain an active session after the password reset.

And the reset token itself is:

```text
Random 256-bit token
        ↓
SHA-256 hash stored in DB
        ↓
30-minute expiration
        ↓
single-use
```

One thing we **shouldn't** do yet is send real email from inside the route. The next step is to create a small **email service abstraction** so registration verification, resend verification, and password reset all use the same delivery mechanism instead of duplicating email code.

Next: **create the email service abstraction**.

We'll keep the authentication routes independent from whichever provider you eventually use. That means registration and password reset can simply call `send_verification_email()` or `send_password_reset_email()`.

## 1. Create the email module

Create:

`src/auth/email.rs`

```rust
use std::env;

pub struct EmailService {
    from: String,
    base_url: String,
}

impl EmailService {
    pub fn from_env() -> Result<Self, String> {
        let from = env::var("EMAIL_FROM")
            .map_err(|_| "EMAIL_FROM is missing".to_string())?;

        let base_url = env::var("APP_BASE_URL")
            .map_err(|_| "APP_BASE_URL is missing".to_string())?;

        Ok(Self {
            from,
            base_url,
        })
    }

    pub async fn send_verification_email(
        &self,
        email: &str,
        token: &str,
    ) -> Result<(), String> {
        let url = format!(
            "{}/verify-email?token={}",
            self.base_url,
            token
        );

        println!("FROM: {}", self.from);
        println!("TO: {}", email);
        println!("VERIFY URL: {}", url);

        Ok(())
    }

    pub async fn send_password_reset_email(
        &self,
        email: &str,
        token: &str,
    ) -> Result<(), String> {
        let url = format!(
            "{}/reset-password?token={}",
            self.base_url,
            token
        );

        println!("FROM: {}", self.from);
        println!("TO: {}", email);
        println!("RESET URL: {}", url);

        Ok(())
    }
}
```

For now this **doesn't actually send email**. It gives us a clean interface while we're building the authentication system.

---

## 2. Export it

In `src/auth/mod.rs`:

```rust
pub mod email;
```

---

## 3. Add environment variables

In `.env`:

```env
EMAIL_FROM=no-reply@example.com
APP_BASE_URL=http://localhost:8000
```

Later, when deployed:

```env
APP_BASE_URL=https://your-domain.com
```

Don't commit `.env` to Git.

Your `.gitignore` should contain:

```gitignore
.env
```

---

## 4. Register the service with Rocket

In `main.rs`, before launching Rocket:

```rust
use crate::auth::email::EmailService;
```

Then:

```rust
let email_service =
    EmailService::from_env()
        .expect("Email service configuration is invalid");
```

Attach it:

```rust
rocket::build()
    .manage(email_service)
```

So your Rocket state contains:

```text
Rocket
 ├── DbConn
 ├── RateLimiter
 └── EmailService
```

---

## 5. Use it from registration

After generating the verification token, instead of:

```rust
println!(
    "Verification email token: {}",
    raw_token
);
```

retrieve the service:

```rust
let email_service = rocket
    .state::<EmailService>()
    .ok_or_else(|| {
        "Email service unavailable".to_string()
    })?;
```

However, there's an important architectural issue here: your current registration route only has `DbConn`, not the Rocket instance.

So don't force Rocket state into the route yet.

Instead, add `&State<EmailService>`:

```rust
use rocket::State;
```

Then your route becomes:

```rust
#[post("/register", data = "<request>")]
pub async fn register(
    mut db: DbConn,
    email_service: &State<EmailService>,
    request: Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, String> {
```

After creating the verification token:

```rust
email_service
    .send_verification_email(
        &email,
        &raw_token,
    )
    .await?;
```

---

## 6. Do the same for password reset

Change:

```rust
#[post("/password/forgot", data = "<request>")]
pub async fn forgot_password(
    mut db: DbConn,
    _rate_limit: LoginRateLimit,
    request: Json<ForgotPasswordRequest>,
)
```

to:

```rust
#[post("/password/forgot", data = "<request>")]
pub async fn forgot_password(
    mut db: DbConn,
    email_service: &State<EmailService>,
    _rate_limit: LoginRateLimit,
    request: Json<ForgotPasswordRequest>,
)
```

Then replace:

```rust
println!(
    "Password reset token: {}",
    raw_token
);
```

with:

```rust
email_service
    .send_password_reset_email(
        &email,
        &raw_token,
    )
    .await?;
```

You'll need to retain the user's email when loading the account:

```rust
let user = db
    .run(move |connection| {
        users::table
            .filter(users::id.eq(user_id))
            .select((
                users::id,
                users::email,
            ))
            .first::<(Uuid, String)>(
                connection
            )
            .optional()
    })
    .await
    .map_err(|error| error.to_string())?;
```

Then:

```rust
let Some((user_id, email)) = user else {
    return Ok(Json(
        ForgotPasswordResponse {
            message:
                "If an account exists for that email, a password reset link has been sent."
                    .to_string(),
        }
    ));
};
```

And finally:

```rust
email_service
    .send_password_reset_email(
        &email,
        &raw_token,
    )
    .await?;
```

---

## 7. One important correction

Earlier we used:

```text
LoginRateLimit
```

for password-reset and email-verification endpoints.

Let's **not** keep doing that.

These should eventually have separate guards:

```text
LoginRateLimit
VerificationEmailRateLimit
PasswordResetRateLimit
TwoFactorRateLimit
RecoveryRateLimit
```

That way:

```text
5 login attempts
```

doesn't consume:

```text
password reset attempts
```

or:

```text
2FA attempts
```

We'll clean that up next.

### Current architecture

```text
              EmailService
                  │
       ┌──────────┼──────────┐
       │          │          │
   Register     Resend    Password
       │       Verify       Reset
       │          │          │
       └──────────┼──────────┘
                  │
             Email Provider
```

The nice part is that when we switch from the development `println!` implementation to a real transactional email provider, **the authentication routes don't need to know the provider's API**.

Next: **split the rate limiter into operation-specific guards and add proper limits for registration, verification, password reset, login, TOTP, and recovery codes.**

Yep — next step is **operation-specific rate limiting**. Right now one global limiter can let login, TOTP, and password-reset attempts interfere with each other. Let's separate them.

### 1. Replace `src/auth/rate_limit.rs`

```rust
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, AttemptState>>,
    max_attempts: u32,
    window: Duration,
}

struct AttemptState {
    count: u32,
    window_started: Instant,
}

impl RateLimiter {
    pub fn new(max_attempts: u32, window: Duration) -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            max_attempts,
            window,
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        let now = Instant::now();

        let state = attempts.entry(key.to_string()).or_insert(AttemptState {
            count: 0,
            window_started: now,
        });

        if now.duration_since(state.window_started) >= self.window {
            state.count = 0;
            state.window_started = now;
        }

        if state.count >= self.max_attempts {
            return false;
        }

        state.count += 1;
        true
    }

    pub fn reset(&self, key: &str) {
        let mut attempts = self
            .lock()
            .expect("rate limiter mutex poisoned");

        attempts.remove(key);
    }
}
```

**Important:** there is a typo in that version if copied literally: `reset()` needs to lock `attempts`, not `self`. Use this corrected version:

```rust
pub fn reset(&self, key: &str) {
    let mut attempts = self
        .attempts
        .lock()
        .expect("rate limiter mutex poisoned");

    attempts.remove(key);
}
```

So your complete file should have `self.attempts` in both methods.

---

### 2. Add operation-specific guards

Create/replace `src/auth/rate_limit_guards.rs`:

```rust
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};

use crate::auth::rate_limit::RateLimiter;

pub struct LoginRateLimit;

pub struct TwoFactorRateLimit;

pub struct RecoveryRateLimit;

pub struct VerificationEmailRateLimit;

pub struct PasswordResetRateLimit;

pub struct RegistrationRateLimit;

fn client_ip(request: &Request<'_>) -> String {
    request
        .client_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

async fn check_limit(
    request: &Request<'_>,
    prefix: &str,
    max_attempts: u32,
) -> bool {
    let limiter = match request.rocket().state::<RateLimiter>() {
        Some(limiter) => limiter,
        None => return false,
    };

    let key = format!("{}:{}", prefix, client_ip(request));

    limiter.check(&key)
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for LoginRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_limit(request, "auth:login", 5).await {
            Outcome::Success(LoginRateLimit)
        } else {
            Outcome::Error((Status::TooManyRequests, ()))
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for TwoFactorRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_limit(request, "auth:2fa", 5).await {
            Outcome::Success(TwoFactorRateLimit)
        } else {
            Outcome::Error((Status::TooManyRequests, ()))
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for RecoveryRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_limit(request, "auth:recovery", 5).await {
            Outcome::Success(RecoveryRateLimit)
        } else {
            Outcome::Error((Status::TooManyRequests, ()))
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for VerificationEmailRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_limit(request, "auth:verification-email", 3).await {
            Outcome::Success(VerificationEmailRateLimit)
        } else {
            Outcome::Error((Status::TooManyRequests, ()))
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for PasswordResetRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_limit(request, "auth:password-reset", 5).await {
            Outcome::Success(PasswordResetRateLimit)
        } else {
            Outcome::Error((Status::TooManyRequests, ()))
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for RegistrationRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        if check_limit(request, "auth:registration", 5).await {
            Outcome::Success(RegistrationRateLimit)
        } else {
            Outcome::Error((Status::TooManyRequests, ()))
        }
    }
}
```

Then add to `auth/mod.rs`:

```rust
pub mod rate_limit_guards;
```

---

### 3. Use the guards in your routes

For login:

```rust
use crate::auth::rate_limit_guards::LoginRateLimit;

#[post("/login", data = "<request>")]
pub async fn login(
    _rate_limit: LoginRateLimit,
    mut db: DbConn,
    request: Json<LoginRequest>,
) -> Result<...> {
    // existing login logic
}
```

For TOTP verification:

```rust
use crate::auth::rate_limit_guards::TwoFactorRateLimit;

#[post("/2fa/verify", data = "<request>")]
pub async fn verify_2fa(
    _rate_limit: TwoFactorRateLimit,
    _csrf: CsrfProtected,
    mut db: DbConn,
    request: Json<TwoFactorVerifyRequest>,
) -> Result<...> {
    // existing logic
}
```

For recovery-code login:

```rust
use crate::auth::rate_limit_guards::RecoveryRateLimit;

#[post("/2fa/recovery", data = "<request>")]
pub async fn recovery_login(
    _rate_limit: RecoveryRateLimit,
    mut db: DbConn,
    request: Json<RecoveryLoginRequest>,
) -> Result<...> {
    // existing logic
}
```

For verification-email resend:

```rust
#[post("/verify-email/resend")]
pub async fn resend_verification_email(
    _rate_limit: VerificationEmailRateLimit,
    _csrf: CsrfProtected,
    user: AuthenticatedUser,
    // ...
) -> Result<...> {
    // existing logic
}
```

For password reset:

```rust
#[post("/password/forgot", data = "<request>")]
pub async fn forgot_password(
    _rate_limit: PasswordResetRateLimit,
    mut db: DbConn,
    request: Json<ForgotPasswordRequest>,
    // ...
) -> Result<...> {
    // existing logic
}
```

And registration:

```rust
#[post("/register", data = "<request>")]
pub async fn register(
    _rate_limit: RegistrationRateLimit,
    mut db: DbConn,
    request: Json<RegisterRequest>,
) -> Result<...> {
    // existing registration logic
}
```

### 4. Why this structure is better

Now each operation has its **own bucket**:

```text
auth:login:<ip>
auth:2fa:<ip>
auth:recovery:<ip>
auth:verification-email:<ip>
auth:password-reset:<ip>
auth:registration:<ip>
```

So five failed login attempts don't consume the user's password-reset allowance.

One caveat: this is still an **in-memory, per-process limiter**. If you eventually run multiple Rocket instances behind a load balancer, move these counters to Redis or another shared store.

**Next after this:** we'll make the rate limits smarter by combining **IP + account/email identity**, which is important because IP-only limiting is easy to bypass with distributed attempts and can also unfairly affect shared networks.

Next: let's upgrade the limiter from **IP-only** to **IP + account identity**.

This matters most for login, 2FA, recovery, and password-reset flows. An attacker shouldn't get unlimited attempts against one account just because they rotate IP addresses.

## 1. Update `rate_limit.rs`

Add a method that accepts an arbitrary key:

```rust id="41827"
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, AttemptState>>,
    max_attempts: u32,
    window: Duration,
}

struct AttemptState {
    count: u32,
    window_started: Instant,
}

impl RateLimiter {
    pub fn new(max_attempts: u32, window: Duration) -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            max_attempts,
            window,
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        let now = Instant::now();

        let state = attempts.entry(key.to_string()).or_insert(AttemptState {
            count: 0,
            window_started: now,
        });

        if now.duration_since(state.window_started) >= self.window {
            state.count = 0;
            state.window_started = now;
        }

        if state.count >= self.max_attempts {
            return false;
        }

        state.count += 1;
        true
    }

    pub fn reset(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        attempts.remove(key);
    }
}
```

---

## 2. Add a reusable key helper

In `rate_limit_guards.rs`:

```rust id="73419"
fn client_ip(request: &Request<'_>) -> String {
    request
        .client_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn ip_key(request: &Request<'_>, operation: &str) -> String {
    format!(
        "auth:{}:ip:{}",
        operation,
        client_ip(request)
    )
}
```

We'll use this for operations that don't have an account identity.

---

## 3. Add account-based login limiting

For login, we want two independent limits:

```text
IP limit
    ↓
auth:login:ip:<ip>

Account limit
    ↓
auth:login:account:<email>
```

Add:

```rust id="qk5g1w"
pub fn account_key(operation: &str, account: &str) -> String {
    format!(
        "auth:{}:account:{}",
        operation,
        account.trim().to_lowercase()
    )
}
```

Then modify `LoginRateLimit`.

```rust id="6u9n3r"
pub struct LoginRateLimit;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for LoginRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        let limiter = match request.rocket().state::<RateLimiter>() {
            Some(limiter) => limiter,
            None => {
                return Outcome::Error((
                    Status::InternalServerError,
                    (),
                ));
            }
        };

        let ip_key = ip_key(request, "login");

        if !limiter.check(&ip_key) {
            return Outcome::Error((
                Status::TooManyRequests,
                (),
            ));
        }

        Outcome::Success(LoginRateLimit)
    }
}
```

There is one important limitation here:

**A Rocket request guard does not conveniently have access to your JSON request body.**

So we should not try to extract the email inside `FromRequest`.

Instead, do the account-level check **inside the login route after parsing the email**.

---

# 4. Add account-level checking to the login route

Your login route should contain something like:

```rust id="9m7q2x"
let email = request.email.trim().to_lowercase();

let account_key = account_key("login", &email);

let limiter = rocket
    .state::<RateLimiter>()
    .ok_or_else(|| "Rate limiter unavailable".to_string())?;

if !limiter.check(&account_key) {
    return Err("Too many login attempts".to_string());
}
```

But because your route currently doesn't have `rocket` available, use Rocket's `State` instead.

Import:

```rust id="5j2k8a"
use rocket::State;

use crate::auth::{
    rate_limit::RateLimiter,
    rate_limit_guards::account_key,
};
```

Then:

```rust id="k2h4n7"
#[post("/login", data = "<request>")]
pub async fn login(
    _rate_limit: LoginRateLimit,
    mut db: DbConn,
    request: Json<LoginRequest>,
    limiter: &State<RateLimiter>,
) -> Result<Json<LoginResponse>, String> {
    let email = request.email.trim().to_lowercase();

    let account_key = account_key("login", &email);

    if !limiter.check(&account_key) {
        return Err("Too many login attempts".to_string());
    }

    // existing login logic...
}
```

The exact argument order doesn't matter much, but keep the extractor types clear.

---

# 5. Do the same for password reset

Password reset should have an IP limit **and** an email/account limit.

Inside:

```rust
POST /auth/password/forgot
```

after normalizing the email:

```rust id="c8v4p1"
let email = request.email.trim().to_lowercase();

let account_key = account_key(
    "password-reset",
    &email,
);

if !limiter.check(&account_key) {
    return Err("Too many password reset attempts".to_string());
}
```

The API should still return your generic response regardless of whether the email exists.

For example:

```text
If account exists:
    generate token
    save token
    send email

If account doesn't exist:
    do nothing

Both:
    "If an account exists, a reset email has been sent."
```

That prevents account enumeration.

---

# 6. Important improvement for 2FA

2FA is different because we already have the authenticated user's ID in the challenge.

Use:

```text
auth:2fa:user:<user_id>
```

rather than relying only on IP.

For your `/auth/2fa/verify` route:

```rust id="n4p8t6"
let user_key = format!(
    "auth:2fa:user:{}",
    challenge.user_id
);

if !limiter.check(&user_key) {
    return Err("Too many two-factor attempts".to_string());
}
```

You can retain the IP-based limiter too:

```text
auth:2fa:ip:<ip>
auth:2fa:user:<user_id>
```

That gives you two layers:

```text
             Request
                │
        ┌───────┴───────┐
        │               │
      IP limit       User limit
        │               │
        └───────┬───────┘
                │
           Verify TOTP
```

---

## 7. Recommended limits

For the learning implementation, use:

| Operation              |          IP |   Account/User |
| ---------------------- | ----------: | -------------: |
| Login                  |  5 / minute |     5 / minute |
| TOTP                   | 10 / minute | 5 / 10 minutes |
| Recovery code          |  5 / minute | 5 / 10 minutes |
| Password reset request |  5 / minute |       3 / hour |
| Verification resend    |    3 / hour |       3 / hour |
| Registration           |  5 / minute |        IP only |

Your current `RateLimiter` has one fixed window/limit, so these different windows will eventually require **separate managed limiters** rather than one global configuration.

That's the next refinement we'll make.

### One security detail before moving on

Don't key an account limiter with the raw email if you're going to put this into a shared logging/monitoring system. For a production implementation, hash or otherwise safely derive the identifier:

```text
email
  ↓
SHA-256
  ↓
account rate-limit key
```

That avoids unnecessarily exposing email addresses in rate-limit state.

**Next step:** we'll implement a `NamedRateLimiter`/multiple limiters so login, TOTP, recovery, verification, and password-reset operations can each have their own limits and windows.

Yep. Now let's make the rate limiter **configurable per operation**, instead of having one hard-coded `5 requests per minute` policy.

This gives you a clean structure for the auth system.

## 1. Replace `src/auth/rate_limit.rs`

```rust
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, AttemptState>>,
}

struct AttemptState {
    count: u32,
    window_started: Instant,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(
        &self,
        key: &str,
        max_attempts: u32,
        window: Duration,
    ) -> bool {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        let now = Instant::now();

        let state = attempts.entry(key.to_string()).or_insert(AttemptState {
            count: 0,
            window_started: now,
        });

        if now.duration_since(state.window_started) >= window {
            state.count = 0;
            state.window_started = now;
        }

        if state.count >= max_attempts {
            return false;
        }

        state.count += 1;
        true
    }

    pub fn reset(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        attempts.remove(key);
    }
}
```

Now the limiter itself doesn't know anything about login, TOTP, or password reset.

The caller decides:

```text
5 attempts / 1 minute
3 attempts / 1 hour
10 attempts / 10 minutes
```

---

# 2. Add centralized policies

Create `src/auth/rate_limit_policy.rs`:

```rust
use std::time::Duration;

pub struct RateLimitPolicy {
    pub max_attempts: u32,
    pub window: Duration,
}

pub fn login_ip() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(60),
    }
}

pub fn login_account() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(60),
    }
}

pub fn two_factor_ip() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 10,
        window: Duration::from_secs(60),
    }
}

pub fn two_factor_user() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(10 * 60),
    }
}

pub fn recovery_ip() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(60),
    }
}

pub fn recovery_user() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(10 * 60),
    }
}

pub fn verification_email_ip() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 3,
        window: Duration::from_secs(60 * 60),
    }
}

pub fn verification_email_user() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 3,
        window: Duration::from_secs(60 * 60),
    }
}

pub fn password_reset_ip() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(60),
    }
}

pub fn password_reset_account() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 3,
        window: Duration::from_secs(60 * 60),
    }
}

pub fn registration_ip() -> RateLimitPolicy {
    RateLimitPolicy {
        max_attempts: 5,
        window: Duration::from_secs(60),
    }
}
```

This is much easier to maintain.

If you later decide:

```text
password reset = 2/hour
```

you change one place.

---

# 3. Export the module

In `src/auth/mod.rs`:

```rust
pub mod rate_limit;
pub mod rate_limit_guards;
pub mod rate_limit_policy;
```

---

# 4. Update the login guard

Your `LoginRateLimit` should now use the policy.

```rust
use std::time::Duration;

use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};

use crate::{
    auth::{
        rate_limit::RateLimiter,
        rate_limit_policy,
    },
};
```

Then:

```rust
pub struct LoginRateLimit;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for LoginRateLimit {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> Outcome<Self, Self::Error> {
        let limiter = match request
            .rocket()
            .state::<RateLimiter>()
        {
            Some(limiter) => limiter,

            None => {
                return Outcome::Error((
                    Status::InternalServerError,
                    (),
                ));
            }
        };

        let ip = request
            .client_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let key = format!(
            "auth:login:ip:{}",
            ip
        );

        let policy = rate_limit_policy::login_ip();

        if !limiter.check(
            &key,
            policy.max_attempts,
            policy.window,
        ) {
            return Outcome::Error((
                Status::TooManyRequests,
                (),
            ));
        }

        Outcome::Success(LoginRateLimit)
    }
}
```

---

# 5. Update the login account limiter

Because the email comes from the JSON body, this belongs **inside the route**, not inside a request guard.

```rust
use rocket::State;

use crate::auth::{
    rate_limit::RateLimiter,
    rate_limit_policy,
};
```

Then after normalizing the email:

```rust
let email = request.email.trim().to_lowercase();

let account_key = format!(
    "auth:login:account:{}",
    hex::encode(
        sha2::Sha256::digest(email.as_bytes())
    )
);

let policy = rate_limit_policy::login_account();

if !limiter.check(
    &account_key,
    policy.max_attempts,
    policy.window,
) {
    return Err("Too many login attempts".to_string());
}
```

You'll need:

```rust
use sha2::{Digest, Sha256};
```

And the route gets:

```rust
limiter: &State<RateLimiter>,
```

---

# 6. Make the Rocket managed state simpler

In `main.rs`:

```rust
.manage(RateLimiter::new())
```

instead of the old:

```rust
.manage(RateLimiter::new(
    5,
    Duration::from_secs(60),
))
```

So your builder becomes roughly:

```rust
rocket::build()
    .manage(db_pool)
    .manage(email_service)
    .manage(RateLimiter::new())
    // ...
```

---

# 7. Why this architecture is cleaner

You now have three separate layers:

```text
                 AUTH REQUEST
                      │
                      ▼
              ┌───────────────┐
              │ Rate Limiter  │
              │               │
              │ Generic       │
              │ Counter       │
              └───────┬───────┘
                      │
             ┌────────▼────────┐
             │ Operation Policy│
             │                 │
             │ login           │
             │ 2FA             │
             │ recovery        │
             │ reset           │
             └─────────────────┘
```

The limiter doesn't care what operation it's protecting.

The policy doesn't care how counters are stored.

The route doesn't contain random hard-coded numbers.

That's a nice separation of responsibilities.

---

## 8. One thing we should fix before production

Your current implementation is still:

```text
Rocket process
      │
      └── HashMap
```

If you eventually run:

```text
Load Balancer
   │
   ├── Rocket #1 → HashMap #1
   ├── Rocket #2 → HashMap #2
   └── Rocket #3 → HashMap #3
```

an attacker can potentially distribute requests across instances.

The production architecture becomes:

```text
Rocket #1 ─┐
Rocket #2 ─┼── Redis ── shared rate-limit state
Rocket #3 ─┘
```

But **don't add Redis yet**. For your current project, the in-memory implementation is perfectly useful for learning and local development.

### Next

We'll tackle **account lockout behavior** carefully: how repeated failed passwords interact with rate limits, how to avoid permanent lockouts, and how to return `429 Too Many Requests` with a proper `Retry-After` response instead of a generic error.

Next up: **proper `429 Too Many Requests` responses with `Retry-After`**.

This is better than returning a plain `"Too many login attempts"` string because clients can know exactly when to retry.

## 1. Create a reusable rate-limit error

Add this to `src/auth/rate_limit.rs`:

```rust id="53184"
use rocket::http::{Header, Status};

pub struct RateLimitError {
    pub retry_after: u64,
}

impl RateLimitError {
    pub fn responder(&self) -> rocket::response::status::Custom<String> {
        let mut response = rocket::response::status::Custom(
            Status::TooManyRequests,
            format!(
                "Too many requests. Retry after {} seconds.",
                self.retry_after
            ),
        );

        response.0 = Status::TooManyRequests;

        response
    }
}
```

Actually, let's keep this cleaner and use Rocket's `Responder` directly.

Replace it with:

```rust id="81427"
use rocket::{
    http::{Header, Status},
    response::{Responder, Response},
    Request,
};

pub struct RateLimitResponse {
    pub retry_after: u64,
}

impl<'r, 'o: 'r> Responder<'r, 'o> for RateLimitResponse {
    fn respond_to(
        self,
        request: &'r Request<'_>,
    ) -> rocket::response::Result<'o> {
        Response::build_from(
            rocket::response::status::Custom(
                Status::TooManyRequests,
                format!(
                    "Too many requests. Retry after {} seconds.",
                    self.retry_after
                ),
            )
            .respond_to(request)?,
        )
        .header(Header::new(
            "Retry-After",
            self.retry_after.to_string(),
        ))
        .ok()
    }
}
```

---

## 2. Make the limiter return retry information

Instead of:

```rust
pub fn check(...) -> bool
```

we want:

```text
allowed
   OR
retry after N seconds
```

Change the method to:

```rust id="29651"
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub enum RateLimitResult {
    Allowed,
    Limited { retry_after: u64 },
}

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, AttemptState>>,
}

struct AttemptState {
    count: u32,
    window_started: Instant,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(
        &self,
        key: &str,
        max_attempts: u32,
        window: Duration,
    ) -> RateLimitResult {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        let now = Instant::now();

        let state = attempts.entry(key.to_string()).or_insert(
            AttemptState {
                count: 0,
                window_started: now,
            },
        );

        let elapsed = now.duration_since(state.window_started);

        if elapsed >= window {
            state.count = 0;
            state.window_started = now;
        }

        if state.count >= max_attempts {
            let retry_after = window
                .saturating_sub(
                    now.duration_since(state.window_started)
                )
                .as_secs()
                .max(1);

            return RateLimitResult::Limited {
                retry_after,
            };
        }

        state.count += 1;

        RateLimitResult::Allowed
    }

    pub fn reset(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .expect("rate limiter mutex poisoned");

        attempts.remove(key);
    }
}
```

---

# 3. Update the login guard

Now instead of:

```rust
if !limiter.check(...) {
    return Outcome::Error(...);
}
```

use:

```rust id="94063"
use crate::auth::rate_limit::{
    RateLimitResult,
    RateLimiter,
};
```

Then:

```rust id="57291"
match limiter.check(
    &key,
    policy.max_attempts,
    policy.window,
) {
    RateLimitResult::Allowed => {
        Outcome::Success(LoginRateLimit)
    }

    RateLimitResult::Limited { .. } => {
        Outcome::Error((
            Status::TooManyRequests,
            (),
        ))
    }
}
```

The guard itself can't conveniently customize the response body because request guards return an `Outcome`.

That's okay.

For APIs where you want a detailed `Retry-After`, do the check **inside the route**.

---

# 4. Add `Retry-After` to login

Your login route can do:

```rust id="24589"
match limiter.check(
    &account_key,
    policy.max_attempts,
    policy.window,
) {
    RateLimitResult::Allowed => {}

    RateLimitResult::Limited { retry_after } => {
        return Err(
            format!(
                "Too many login attempts. Retry after {} seconds.",
                retry_after
            )
        );
    }
}
```

But there is an even cleaner approach.

Create:

```rust id="37706"
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub retry_after: Option<u64>,
}
```

Then your API can return:

```json
{
  "error": "too_many_requests",
  "retry_after": 42
}
```

with:

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 42
```

---

# 5. Don't lock accounts permanently

This is an important security distinction.

Avoid:

```text
5 failures
   ↓
ACCOUNT LOCKED FOREVER
```

That allows an attacker to deliberately lock somebody else's account.

Instead use temporary throttling:

```text
5 failures
   ↓
temporary rate limit
   ↓
wait
   ↓
try again
```

And potentially progressively increase the delay for repeated abuse.

For example:

```text
Attempts       Response
────────────────────────────
1–5            normal
6              429 / 60 sec
after window   normal
repeated abuse longer throttling
```

The rate limiter already gives you the first layer.

---

# 6. One more important change: failed vs successful login

Right now the limiter increments **every login request**, including successful ones.

That's not ideal.

A better model is:

```text
Request
   │
   ▼
IP rate limit
   │
   ▼
Check credentials
   │
   ├── invalid ──→ increment account failure
   │
   └── valid ────→ reset account failure
```

So the account-specific limiter should primarily track **failed authentication attempts**, while the IP limiter can protect the endpoint from request flooding.

After successful password verification:

```rust id="8e4212"
limiter.reset(&account_key);
```

After a failed password:

```rust id="4j7n0q"
let result = limiter.check(
    &account_key,
    policy.max_attempts,
    policy.window,
);
```

This gives you a much more sensible behavior.

---

## Final architecture

```text
                    LOGIN
                      │
                      ▼
              ┌──────────────┐
              │ IP limiter   │
              │ 5 / minute   │
              └──────┬───────┘
                     │
                     ▼
              Check credentials
                     │
             ┌───────┴───────┐
             │               │
          FAILED           SUCCESS
             │               │
             ▼               ▼
     Account limiter      Reset account
       5 / minute          failures
             │
             ▼
          429 when
          exceeded
```

This is the point where your authentication system starts behaving like a real production API rather than just a collection of auth endpoints.

**Next:** we'll add a proper **security event/audit log** so events like login failures, successful logins, 2FA changes, password changes, session revocations, and password resets are recorded in PostgreSQL.

Yep — next is the **security audit log**. This gives us a durable record of important authentication events.

We’ll keep it generic enough that later you can add IP, user agent, request ID, etc.

## 1. Create the migration

Run:

```bash
diesel migration generate create_security_events
```

### `up.sql`

```sql
CREATE TABLE security_events (
    id UUID PRIMARY KEY,
    user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    event_type VARCHAR(100) NOT NULL,
    ip_address VARCHAR(45) NULL,
    user_agent VARCHAR(500) NULL,
    metadata JSONB NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX security_events_user_id_idx
ON security_events(user_id);

CREATE INDEX security_events_event_type_idx
ON security_events(event_type);

CREATE INDEX security_events_created_at_idx
ON security_events(created_at);
```

### `down.sql`

```sql
DROP INDEX security_events_created_at_idx;
DROP INDEX security_events_event_type_idx;
DROP INDEX security_events_user_id_idx;

DROP TABLE security_events;
```

Then:

```bash
diesel migration run
```

And regenerate:

```bash
diesel print-schema > src/schema.rs
```

---

# 2. Create the model

Create:

```text
src/models/security_event.rs
```

```rust id="68241"
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
```

---

# 3. Export the model

In:

```text
src/models/mod.rs
```

add:

```rust id="20853"
pub mod security_event;
```

---

# 4. Create the audit helper

Create:

```text
src/auth/audit.rs
```

```rust id="95173"
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
```

Then export it:

```rust id="e2d61"
pub mod audit;
```

in `src/auth/mod.rs`.

---

# 5. Define consistent event names

Don't scatter random strings throughout the application.

Create:

```text
src/auth/audit_events.rs
```

```rust id="50162"
pub const REGISTERED: &str = "user.registered";

pub const LOGIN_SUCCESS: &str = "auth.login.success";
pub const LOGIN_FAILED: &str = "auth.login.failed";

pub const LOGOUT: &str = "auth.logout";
pub const LOGOUT_ALL: &str = "auth.logout_all";

pub const TWO_FACTOR_ENABLED: &str = "auth.2fa.enabled";
pub const TWO_FACTOR_DISABLED: &str = "auth.2fa.disabled";
pub const TWO_FACTOR_FAILED: &str = "auth.2fa.failed";

pub const RECOVERY_CODE_USED: &str = "auth.recovery_code.used";
pub const RECOVERY_CODES_REGENERATED: &str =
    "auth.recovery_codes.regenerated";

pub const EMAIL_VERIFIED: &str = "auth.email.verified";
pub const EMAIL_VERIFICATION_RESENT: &str =
    "auth.email_verification.resent";

pub const PASSWORD_CHANGED: &str = "auth.password.changed";
pub const PASSWORD_RESET_REQUESTED: &str =
    "auth.password_reset.requested";
pub const PASSWORD_RESET_COMPLETED: &str =
    "auth.password_reset.completed";

pub const SESSION_REVOKED: &str = "auth.session.revoked";
pub const SESSIONS_REVOKED_ALL: &str =
    "auth.sessions.revoked_all";
```

Export:

```rust id="56b83"
pub mod audit_events;
```

---

# 6. Record a successful login

After credentials are successfully verified:

```rust id="76319"
use crate::auth::{
    audit,
    audit_events,
};
```

Then:

```rust id="f1y4y5"
let ip = request
    .client_ip()
    .map(|ip| ip.to_string());

let user_agent = request
    .headers()
    .get_one("User-Agent")
    .map(str::to_string);

audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGIN_SUCCESS,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

---

# 7. Record failed login attempts

For an invalid password:

```rust id="n28a8"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGIN_FAILED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

But there's an important case:

### Unknown email

If someone submits:

```text
attacker@example.com
```

and that account doesn't exist, **don't create a security event containing that email**.

You can instead record:

```rust id="j8p9qa"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: None,
        event_type: audit_events::LOGIN_FAILED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

That preserves useful security information without unnecessarily storing the submitted email.

---

# 8. Record 2FA events

After successful TOTP verification:

```rust id="a6n3ks"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGIN_SUCCESS,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

For a failed TOTP:

```rust id="q3s7cx"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(challenge.user_id),
        event_type: audit_events::TWO_FACTOR_FAILED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

Don't store:

```text
❌ password
❌ TOTP code
❌ recovery code
❌ session token
❌ verification token
❌ password-reset token
```

The audit table should tell us **what happened**, not expose authentication secrets.

---

# 9. Record password changes

After a successful password change:

```rust id="m7h2q4"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::PASSWORD_CHANGED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

Likewise after a successful password reset:

```rust id="t6f9a1"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user_id),
        event_type: audit_events::PASSWORD_RESET_COMPLETED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

---

# 10. Record 2FA enable/disable

When confirmation succeeds:

```rust id="c1e4v8"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::TWO_FACTOR_ENABLED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

When disabled:

```rust id="r4k8s2"
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::TWO_FACTOR_DISABLED,
        ip_address: ip.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: None,
    },
)
.await?;
```

---

## 11. What the database now gives you

For example:

```text
security_events

id                  user_id       event_type                  created_at
──────────────────────────────────────────────────────────────────────────────
...                 ...           user.registered             ...
...                 ...           auth.login.success          ...
...                 ...           auth.2fa.enabled            ...
...                 ...           auth.session.revoked        ...
...                 ...           auth.password.changed       ...
...                 ...           auth.login.failed           ...
```

And because we have indexes on:

```text
user_id
event_type
created_at
```

we can efficiently answer questions like:

```sql
SELECT *
FROM security_events
WHERE user_id = $1
ORDER BY created_at DESC;
```

That leads naturally to the next feature:

**`GET /auth/security-events`** — let an authenticated user see their recent security activity, while making sure we don't expose sensitive metadata or allow them to access another user's events.

Yep — now we’ll expose the audit log safely through **`GET /auth/security-events`**.

The key rule: **the authenticated user's `user_id` is always taken from `AuthenticatedUser`, never from a URL/query parameter.**

## 1. Add the response type

In `src/auth/routes.rs`:

```rust id="48216"
#[derive(Serialize)]
pub struct SecurityEventResponse {
    pub id: Uuid,
    pub event_type: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: NaiveDateTime,
}
```

Notice we intentionally omit:

```text
user_id
metadata
```

The user already knows which account they're viewing, and arbitrary metadata shouldn't automatically become public API data.

---

## 2. Add the route

```rust id="73154"
#[get("/security-events")]
pub async fn security_events(
    mut db: DbConn,
    user: AuthenticatedUser,
) -> Result<Json<Vec<SecurityEventResponse>>, String> {
    use crate::schema::security_events::dsl::*;

    let events = db
        .run(move |connection| {
            security_events
                .filter(user_id.eq(user.id))
                .order(created_at.desc())
                .limit(50)
                .select((
                    id,
                    event_type,
                    ip_address,
                    user_agent,
                    created_at,
                ))
                .load::<(
                    Uuid,
                    String,
                    Option<String>,
                    Option<String>,
                    NaiveDateTime,
                )>(connection)
        })
        .await
        .map_err(|error| error.to_string())?;

    let response = events
        .into_iter()
        .map(
            |(
                id,
                event_type,
                ip_address,
                user_agent,
                created_at,
            )| {
                SecurityEventResponse {
                    id,
                    event_type,
                    ip_address,
                    user_agent,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(response))
}
```

This returns the **50 most recent events**.

---

## 3. Add it to `routes![]`

Where you register your auth routes:

```rust id="84593"
routes![
    register,
    login,
    logout,
    logout_all,
    list_sessions,
    revoke_session,
    security_events,
    // ...
]
```

---

## 4. Test it

After logging in:

```http id="x1h4a6"
GET /auth/security-events
Cookie: session=<your-session-cookie>
```

You should get something like:

```json id="a5r2k8"
[
  {
    "id": "7c7...",
    "event_type": "auth.login.success",
    "ip_address": "127.0.0.1",
    "user_agent": "Mozilla/5.0",
    "created_at": "2026-09-18T20:42:10"
  },
  {
    "id": "91e...",
    "event_type": "auth.2fa.enabled",
    "ip_address": "127.0.0.1",
    "user_agent": "Mozilla/5.0",
    "created_at": "2026-09-18T20:40:02"
  }
]
```

---

## 5. Don't allow arbitrary user IDs

Avoid creating an endpoint like:

```text
GET /auth/security-events/<user_id>
```

That creates an unnecessary authorization surface.

Instead:

```text
Cookie
  ↓
AuthenticatedUser
  ↓
user.id
  ↓
WHERE security_events.user_id = user.id
```

So even if someone modifies their request, they can't ask for another user's events.

---

## 6. Add pagination now

Fifty events is okay initially, but eventually you'll want pagination.

A simple version:

```text
GET /auth/security-events?limit=50&offset=0
```

Request 1:

```text
limit=50
offset=0
```

Request 2:

```text
limit=50
offset=50
```

The route can use:

```rust id="70324"
#[get("/security-events?<limit>&<offset>")]
pub async fn security_events(
    mut db: DbConn,
    user: AuthenticatedUser,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Json<Vec<SecurityEventResponse>>, String> {
```

Then sanitize them:

```rust id="11684"
let limit = limit
    .unwrap_or(50)
    .clamp(1, 100);

let offset = offset
    .unwrap_or(0)
    .max(0);
```

And Diesel:

```rust id="92841"
security_events
    .filter(user_id.eq(user.id))
    .order(created_at.desc())
    .limit(limit)
    .offset(offset)
```

This prevents someone from requesting:

```text
?limit=999999999
```

---

## 7. Better pagination: cursor-based later

For a real production API, I would eventually replace offset pagination with:

```text
GET /auth/security-events?before=<timestamp-or-id>
```

because:

```text
OFFSET 100000
```

can become expensive as the table grows.

But **don't complicate it yet**. Limit/offset is perfectly fine while we're building the authentication system.

---

## 8. One privacy improvement

IP addresses and user-agent strings are potentially sensitive operational data.

So your security-events endpoint should eventually have a retention policy, for example:

```text
Security events
      ↓
retain for X days
      ↓
delete old events
```

Just like we already clean up expired sessions.

---

### Current auth architecture

You now have:

```text
Registration
     │
     ▼
Email verification
     │
     ▼
Login ──────────────┐
     │              │
     ▼              │
   2FA               │
     │              │
     ▼              │
 Session             │
     │              │
     ├── list        │
     ├── revoke      │
     └── logout      │
                    │
Password change ────┤
Password reset ─────┤
2FA changes ────────┤
Recovery codes ─────┤
                    ▼
             Security Events
                    │
                    ▼
          GET /security-events
```

**Next:** we'll add **security-event retention/cleanup**, then the auth system is ready for a more interesting piece: **session/device management with friendly device names and last-seen information**.

Let's do **security-event cleanup** next. We don't want the audit table growing forever.

## 1. Add a cleanup function

Create:

```text
src/auth/security_event_cleanup.rs
```

```rust id="58321"
use chrono::{Duration, Utc};
use diesel::prelude::*;

use crate::schema::security_events;
use crate::DbConn;

pub async fn cleanup_security_events(
    db: &mut DbConn,
) -> Result<usize, String> {
    let cutoff =
        Utc::now().naive_utc() - Duration::days(180);

    db.run(move |connection| {
        diesel::delete(
            security_events::table
                .filter(
                    security_events::created_at.lt(cutoff)
                ),
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())
}
```

We're keeping **180 days** of security events.

---

## 2. Export the module

In `src/auth/mod.rs`:

```rust id="91472"
pub mod security_event_cleanup;
```

---

## 3. Add cleanup to the background worker

You already have your session cleanup loop.

Instead of creating another independent background task, let's run both from the same loop.

Update `src/auth/background.rs`:

```rust id="26743"
use std::time::Duration;

use rocket::tokio::time::interval;

use crate::{
    auth::{
        security_event_cleanup::cleanup_security_events,
        session_cleanup::cleanup_sessions,
    },
    DbConn,
};

pub async fn cleanup_loop(
    rocket: rocket::Rocket<rocket::Orbit>,
) {
    let mut timer =
        interval(Duration::from_secs(60 * 60));

    loop {
        timer.tick().await;

        let Some(mut db) = DbConn::get_one(&rocket).await
        else {
            eprintln!(
                "Cleanup could not acquire database connection"
            );

            continue;
        };

        match cleanup_sessions(&mut db).await {
            Ok(deleted) if deleted > 0 => {
                println!(
                    "Session cleanup removed {} records",
                    deleted
                );
            }

            Ok(_) => {}

            Err(error) => {
                eprintln!(
                    "Session cleanup failed: {}",
                    error
                );
            }
        }

        match cleanup_security_events(&mut db).await {
            Ok(deleted) if deleted > 0 => {
                println!(
                    "Security-event cleanup removed {} records",
                    deleted
                );
            }

            Ok(_) => {}

            Err(error) => {
                eprintln!(
                    "Security-event cleanup failed: {}",
                    error
                );
            }
        }
    }
}
```

### One adjustment

Your existing `cleanup_sessions()` currently accepts a `&mut PgConnection`:

```rust
pub fn cleanup_sessions(
    connection: &mut PgConnection,
) -> Result<usize, diesel::result::Error>
```

So don't call it with `&mut DbConn`.

Instead, keep the database work inside `db.run()`:

```rust id="65217"
match db
    .run(|connection| cleanup_sessions(connection))
    .await
{
    Ok(deleted) if deleted > 0 => {
        println!(
            "Session cleanup removed {} records",
            deleted
        );
    }

    Ok(_) => {}

    Err(error) => {
        eprintln!(
            "Session cleanup failed: {}",
            error
        );
    }
}
```

And the security-event cleanup should likewise operate on the connection directly.

So I'd actually simplify `security_event_cleanup.rs` to:

```rust id="40186"
use chrono::{Duration, Utc};
use diesel::prelude::*;

use crate::schema::security_events;

pub fn cleanup_security_events(
    connection: &mut PgConnection,
) -> Result<usize, diesel::result::Error> {
    let cutoff =
        Utc::now().naive_utc() - Duration::days(180);

    diesel::delete(
        security_events::table
            .filter(
                security_events::created_at.lt(cutoff)
            ),
    )
    .execute(connection)
}
```

Then the background loop does:

```rust id="70418"
match db
    .run(|connection| cleanup_security_events(connection))
    .await
{
    Ok(deleted) if deleted > 0 => {
        println!(
            "Security-event cleanup removed {} records",
            deleted
        );
    }

    Ok(_) => {}

    Err(error) => {
        eprintln!(
            "Security-event cleanup failed: {}",
            error
        );
    }
}
```

That matches your existing session-cleanup architecture.

---

# 4. Keep the cleanup interval separate from retention

These are two different concepts:

```text
Cleanup interval
    = how often we run the job

Retention period
    = how long data remains
```

Currently:

```text
Every 1 hour
     ↓
Delete security events older than 180 days
```

That's good because you don't need to run an expensive cleanup every request.

---

# 5. Add an index-friendly query

You already created:

```sql
CREATE INDEX security_events_created_at_idx
ON security_events(created_at);
```

So this:

```sql
WHERE created_at < cutoff
```

can efficiently locate old records.

---

# 6. Test it manually

You can temporarily insert an old event:

```sql id="9f4q2p"
INSERT INTO security_events (
    id,
    user_id,
    event_type,
    created_at
)
VALUES (
    gen_random_uuid(),
    NULL,
    'test.old_event',
    CURRENT_TIMESTAMP - INTERVAL '200 days'
);
```

Then run your cleanup function.

Verify:

```sql id="6h5t1m"
SELECT *
FROM security_events
WHERE event_type = 'test.old_event';
```

It should be gone.

---

## One important production note

Audit logs can have **compliance requirements** depending on what your application does.

So don't blindly assume 180 days is universally correct. Treat it as a development default and choose retention based on your application's requirements.

---

### Where we are now

Your auth system has:

```text
✅ Password hashing
✅ Registration
✅ Email verification
✅ Login
✅ Sessions
✅ Session revocation
✅ Session cleanup
✅ CSRF
✅ Rate limiting
✅ TOTP 2FA
✅ Recovery codes
✅ Password change
✅ Password reset
✅ Email service abstraction
✅ Security audit events
✅ Security-event API
✅ Security-event cleanup
```

**Next:** let's improve sessions themselves by storing **device information and last-seen metadata**, so `/auth/sessions` can return useful entries like browser/device, IP, last activity, expiration, and whether it's the current session.

Yep — let's do **session/device metadata** next.

We'll add **IP address, user agent, and a friendly device label** to each session. This makes `/auth/sessions` much more useful.

## 1. Create the migration

```bash
diesel migration generate add_session_device_metadata
```

### `up.sql`

```sql
ALTER TABLE sessions
ADD COLUMN ip_address VARCHAR(45) NULL,
ADD COLUMN user_agent VARCHAR(500) NULL,
ADD COLUMN device_name VARCHAR(100) NULL;
```

### `down.sql`

```sql
ALTER TABLE sessions
DROP COLUMN device_name,
DROP COLUMN user_agent,
DROP COLUMN ip_address;
```

Run:

```bash
diesel migration run
diesel print-schema > src/schema.rs
```

---

## 2. Update `Session`

In `src/models/session.rs`:

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::sessions;

#[derive(Queryable)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = sessions)]
pub struct NewSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_name: Option<String>,
}
```

Because `NewSession` changed, **every place that creates a session now needs these three fields**.

---

## 3. Add a device-name helper

Create:

```text
src/auth/device.rs
```

```rust
pub fn device_name(user_agent: Option<&str>) -> String {
    let Some(user_agent) = user_agent else {
        return "Unknown device".to_string();
    };

    let ua = user_agent.to_lowercase();

    if ua.contains("iphone") {
        return "iPhone".to_string();
    }

    if ua.contains("ipad") {
        return "iPad".to_string();
    }

    if ua.contains("android") {
        return "Android device".to_string();
    }

    if ua.contains("windows") {
        return "Windows PC".to_string();
    }

    if ua.contains("macintosh") {
        return "Mac".to_string();
    }

    if ua.contains("linux") {
        return "Linux PC".to_string();
    }

    "Unknown device".to_string()
}
```

Export it:

```rust
pub mod device;
```

in `auth/mod.rs`.

This is intentionally simple. We don't need a giant user-agent parsing library yet.

---

## 4. Capture request metadata during login

Inside your login route:

```rust
let ip_address = request
    .client_ip()
    .map(|ip| ip.to_string());

let user_agent = request
    .headers()
    .get_one("User-Agent")
    .map(str::to_string);

let device_name = device::device_name(
    user_agent.as_deref()
);
```

Then when creating the session:

```rust
let new_session = NewSession {
    id: Uuid::new_v4(),
    user_id: user.id,
    token_hash,
    expires_at,
    ip_address,
    user_agent,
    device_name: Some(device_name),
};
```

---

## 5. Do the same for successful 2FA

Your TOTP verification creates the session after successful verification.

Capture:

```rust
let ip_address = request
    .client_ip()
    .map(|ip| ip.to_string());

let user_agent = request
    .headers()
    .get_one("User-Agent")
    .map(str::to_string);

let device_name = device::device_name(
    user_agent.as_deref()
);
```

Then:

```rust
let new_session = NewSession {
    id: Uuid::new_v4(),
    user_id: challenge.user_id,
    token_hash: new_token_hash,
    expires_at,
    ip_address,
    user_agent,
    device_name: Some(device_name),
};
```

---

# 6. Update `session_rotation.rs`

Your `rotate_session()` currently creates:

```rust
let new_session = NewSession {
    id: Uuid::new_v4(),
    user_id,
    token_hash: new_token_hash,
    expires_at,
};
```

Now it needs:

```rust
let new_session = NewSession {
    id: Uuid::new_v4(),
    user_id,
    token_hash: new_token_hash,
    expires_at,
    ip_address: None,
    user_agent: None,
    device_name: None,
};
```

Later we can pass request metadata into the rotation helper properly.

---

# 7. Improve the sessions API

Update the response:

```rust
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
```

Now the client gets useful information:

```json
[
  {
    "id": "8f...",
    "device_name": "Mac",
    "ip_address": "192.168.1.20",
    "user_agent": "Mozilla/5.0 ...",
    "created_at": "2026-09-18T18:20:00",
    "last_used_at": "2026-09-18T20:48:00",
    "expires_at": "2026-09-25T18:20:00",
    "current": true
  }
]
```

---

## 8. Don't expose the session token

Even with all this new metadata, **never return**:

```text
token_hash
raw session token
```

The API should only return descriptive metadata.

---

## 9. Important privacy detail

The IP and user-agent fields are useful for the user's security page, but don't treat `device_name` as authoritative.

For example:

```text
"Mac"
```

doesn't prove the person is actually using a Mac. It's just a lightweight interpretation of the browser's `User-Agent`.

So think of it as:

```text
device_name = convenience label
```

not:

```text
device_name = trusted identity
```

---

## 10. One more change: update `last_used_at`

Your `AuthenticatedUser` guard already updates:

```rust
sessions::last_used_at.eq(now)
```

Now that we have device metadata, this becomes the useful "last active" timestamp displayed in the UI.

So your session page can eventually look like:

```text
┌──────────────────────────────────────┐
│ Mac                                  │
│ 192.168.1.20                         │
│ Active now                           │
│                                      │
│ Created Sep 18                       │
│ Last active Sep 18, 8:48 PM          │
│ Expires Sep 25                       │
│                                      │
│              [ Revoke ]              │
└──────────────────────────────────────┘
```

That's a much nicer security/session-management experience.

### Next

We'll add **session idle expiration** in addition to the existing absolute 7-day expiration.

That gives us:

```text
absolute expiration → maximum 7 days
idle expiration     → e.g. 24 hours without activity
```

So an abandoned session doesn't remain usable for the full seven days.

Yep. Next we’ll add **idle session expiration** while keeping the existing 7-day absolute expiration.

That gives every session two independent expiration rules:

```text
Absolute lifetime: 7 days
Idle lifetime:     24 hours
```

A session must satisfy **both**.

---

## 1. Add `idle_expires_at`

Create a migration:

```bash
diesel migration generate add_session_idle_expiration
```

### `up.sql`

```sql
ALTER TABLE sessions
ADD COLUMN idle_expires_at TIMESTAMP NOT NULL
DEFAULT CURRENT_TIMESTAMP;
```

### `down.sql`

```sql
ALTER TABLE sessions
DROP COLUMN idle_expires_at;
```

Run:

```bash
diesel migration run
diesel print-schema > src/schema.rs
```

---

## 2. Update `Session`

In `src/models/session.rs`:

```rust
use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use uuid::Uuid;

use crate::schema::sessions;

#[derive(Queryable)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub idle_expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = sessions)]
pub struct NewSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub idle_expires_at: NaiveDateTime,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_name: Option<String>,
}
```

---

# 3. Define the session lifetime

Create:

```text
src/auth/session_policy.rs
```

```rust
use chrono::Duration;

pub const SESSION_ABSOLUTE_DAYS: i64 = 7;
pub const SESSION_IDLE_HOURS: i64 = 24;

pub fn absolute_lifetime() -> Duration {
    Duration::days(SESSION_ABSOLUTE_DAYS)
}

pub fn idle_lifetime() -> Duration {
    Duration::hours(SESSION_IDLE_HOURS)
}
```

Export it:

```rust
pub mod session_policy;
```

in `auth/mod.rs`.

Now we have one place controlling session lifetime.

---

# 4. Update session creation

Where you currently have:

```rust
let now = Utc::now().naive_utc();

let expires_at = now + Duration::days(7);
```

change it to:

```rust
use crate::auth::session_policy;

let now = Utc::now().naive_utc();

let expires_at =
    now + session_policy::absolute_lifetime();

let idle_expires_at =
    now + session_policy::idle_lifetime();
```

Then:

```rust
let new_session = NewSession {
    id: Uuid::new_v4(),
    user_id: user.id,
    token_hash,
    expires_at,
    idle_expires_at,
    ip_address,
    user_agent,
    device_name: Some(device_name),
};
```

Do this anywhere a brand-new session is created:

* normal login
* successful TOTP
* recovery-code login
* session rotation

---

# 5. Update `AuthenticatedUser`

This is the important part.

Your current query checks:

```rust
.filter(sessions::expires_at.gt(now))
```

Add:

```rust
.filter(sessions::idle_expires_at.gt(now))
```

So the session is valid only when:

```text
absolute expiration > now
AND
idle expiration > now
AND
revoked_at IS NULL
```

The relevant query becomes:

```rust
sessions::table
    .inner_join(
        users::table.on(
            users::id.eq(sessions::user_id)
        )
    )
    .filter(
        sessions::token_hash.eq(&token_hash)
    )
    .filter(
        sessions::revoked_at.is_null()
    )
    .filter(
        sessions::expires_at.gt(now)
    )
    .filter(
        sessions::idle_expires_at.gt(now)
    )
    .select((users::id, users::email))
    .first::<(Uuid, String)>(connection)
```

---

# 6. Refresh the idle expiration

When the session is successfully authenticated, extend its idle expiration.

Use:

```rust
let new_idle_expiration =
    now + session_policy::idle_lifetime();
```

Then:

```rust
diesel::update(
    sessions::table
        .filter(
            sessions::token_hash.eq(refresh_hash)
        )
        .filter(
            sessions::revoked_at.is_null()
        )
)
.set((
    sessions::last_used_at.eq(now),
    sessions::idle_expires_at.eq(new_idle_expiration),
))
.execute(connection)
```

So every valid request effectively says:

```text
User activity
     ↓
last_used_at = now
     ↓
idle_expires_at = now + 24 hours
```

But the absolute expiration remains unchanged.

---

# 7. Why absolute expiration must remain

Imagine:

```text
Created:
September 18

Absolute expiration:
September 25
```

The user keeps making requests every few hours.

Without absolute expiration:

```text
request
 ↓
extend 24h
 ↓
request
 ↓
extend 24h
 ↓
request
 ↓
extend 24h
```

The session could effectively live forever.

With both policies:

```text
                 September 25
                      │
                      ▼
Created ───────────────────────────→ absolute expiration
          ↘
           idle expiration keeps moving
```

The idle timeout can move forward, but **never past the absolute expiration**.

---

# 8. Don't let idle expiration exceed absolute expiration

Instead of:

```rust
let idle_expires_at =
    now + session_policy::idle_lifetime();
```

you can safely calculate:

```rust
let proposed_idle =
    now + session_policy::idle_lifetime();

let idle_expires_at =
    std::cmp::min(proposed_idle, expires_at);
```

That guarantees:

```text
idle_expires_at <= expires_at
```

Do the same when refreshing the session:

```rust
let proposed_idle =
    now + session_policy::idle_lifetime();

let new_idle_expiration =
    std::cmp::min(
        proposed_idle,
        expires_at,
    );
```

This is an important little detail.

---

# 9. Update session cleanup

Your cleanup currently checks:

```rust
sessions::expires_at.le(now)
```

Add idle expiration:

```rust
sessions::expires_at
    .le(now)
    .or(
        sessions::idle_expires_at.le(now)
    )
```

So:

```rust
diesel::delete(
    sessions::table.filter(
        sessions::expires_at
            .le(now)
            .or(
                sessions::idle_expires_at.le(now)
            )
            .or(
                sessions::revoked_at
                    .lt(retention_cutoff)
            )
    )
)
.execute(connection)
```

---

# 10. Update the session API

Add:

```rust
pub idle_expires_at: NaiveDateTime,
```

to:

```rust
ActiveSessionResponse
```

So:

```rust
#[derive(Serialize)]
pub struct ActiveSessionResponse {
    pub id: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub idle_expires_at: NaiveDateTime,
    pub current: bool,
}
```

Now the frontend can distinguish:

```text
Expires:
Sep 25

Idle timeout:
Sep 19
```

---

## The final session rules

Your session now behaves like this:

```text
                    SESSION
                       │
             ┌─────────┴─────────┐
             │                   │
      Absolute lifetime       Idle lifetime
          7 days                24 hours
             │                   │
             │              refreshed by
             │              valid activity
             │                   │
             └─────────┬─────────┘
                       │
                 BOTH must pass
                       │
                       ▼
                  AUTHORIZED
```

And if either expires:

```text
request
  ↓
session lookup
  ↓
expired?
  ├── yes → 401 Unauthorized
  └── no  → authenticated
```

One subtle security point: **don't refresh `last_used_at` or `idle_expires_at` if the session has already failed validation.** The validation query must happen first.

### Next

We'll add **session expiration/revocation audit events** and then build a proper **"log out all other devices"** endpoint, which is one of the most useful pieces of the session-management system.

Next: **“Log out all other devices.”** This is a useful security control and fits directly into the session system we just built.

The behavior will be:

```text
Current session
      │
      ├── stays active
      │
      └── every other session
              ↓
          revoked
```

## 1. Add the route

In `src/auth/routes.rs`:

```rust
#[post("/sessions/revoke-others")]
pub async fn revoke_other_sessions(
    _csrf: CsrfProtected,
    mut db: DbConn,
    user: AuthenticatedUser,
) -> Result<Status, String> {
    let now = Utc::now().naive_utc();

    db.run(move |connection| {
        connection.transaction::<(), String, _>(|connection| {
            diesel::update(
                sessions::table
                    .filter(sessions::user_id.eq(user.id))
                    .filter(sessions::revoked_at.is_null())
                    .filter(sessions::token_hash.ne(&current_token_hash)),
            )
            .set(sessions::revoked_at.eq(now))
            .execute(connection)
            .map_err(|error| error.to_string())?;

            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Status::NoContent)
}
```

There is one missing piece here: `current_token_hash`.

Let's make that explicit rather than trying to derive it from the user.

---

# 2. Read the current session token

Add:

```rust
use crate::auth::session::hash_token;
```

Then accept the cookies:

```rust
use rocket::http::{CookieJar, Status};
```

The route becomes:

```rust
#[post("/sessions/revoke-others")]
pub async fn revoke_other_sessions(
    _csrf: CsrfProtected,
    mut db: DbConn,
    user: AuthenticatedUser,
    cookies: &CookieJar<'_>,
) -> Result<Status, String> {
    let current_cookie = cookies
        .get("session")
        .ok_or_else(|| "Session cookie missing".to_string())?;

    let current_token_hash =
        hash_token(current_cookie.value());

    let now = Utc::now().naive_utc();

    db.run(move |connection| {
        diesel::update(
            sessions::table
                .filter(
                    sessions::user_id.eq(user.id)
                )
                .filter(
                    sessions::token_hash
                        .ne(&current_token_hash)
                )
                .filter(
                    sessions::revoked_at.is_null()
                ),
        )
        .set(
            sessions::revoked_at.eq(now)
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;

    Ok(Status::NoContent)
}
```

---

# 3. Add an audit event

Add this constant to `audit_events.rs`:

```rust
pub const OTHER_SESSIONS_REVOKED: &str =
    "auth.sessions.other_revoked";
```

Then after the database update:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::OTHER_SESSIONS_REVOKED,
        ip_address: None,
        user_agent: None,
        metadata: None,
    },
)
.await?;
```

But we actually want the request's IP and user-agent too.

So capture them before the database operation:

```rust
let ip_address = request
    .client_ip()
    .map(|ip| ip.to_string());

let user_agent = request
    .headers()
    .get_one("User-Agent")
    .map(str::to_string);
```

For that, add:

```rust
request: &Request<'_>,
```

or extract those values through the route's request context, depending on your current route signature.

---

# 4. Better: return how many sessions were revoked

This is more useful for the frontend.

Change the response:

```rust
#[derive(Serialize)]
pub struct RevokeOtherSessionsResponse {
    pub revoked_count: usize,
}
```

Then:

```rust
let revoked_count = db
    .run(move |connection| {
        diesel::update(
            sessions::table
                .filter(
                    sessions::user_id.eq(user.id)
                )
                .filter(
                    sessions::token_hash
                        .ne(&current_token_hash)
                )
                .filter(
                    sessions::revoked_at.is_null()
                ),
        )
        .set(
            sessions::revoked_at.eq(now)
        )
        .execute(connection)
    })
    .await
    .map_err(|error| error.to_string())?;
```

And return:

```rust
Ok(Json(
    RevokeOtherSessionsResponse {
        revoked_count,
    }
))
```

---

# 5. The endpoint

You now have:

```http
POST /auth/sessions/revoke-others
X-CSRF-Token: <csrf-token>
Cookie: session=<current-session>
```

Response:

```json
{
  "revoked_count": 4
}
```

The current session remains valid.

---

# 6. Add it to `routes![]`

```rust
routes![
    register,
    login,
    logout,
    logout_all,
    list_sessions,
    revoke_session,
    revoke_other_sessions,
    security_events,
    // ...
]
```

---

# 7. Important difference from `logout-all`

You now have two separate operations:

### Logout current session

```text
POST /auth/logout
```

```text
current session → revoked
```

### Logout all sessions

```text
POST /auth/logout-all
```

```text
every session → revoked
```

### Logout all other sessions

```text
POST /auth/sessions/revoke-others
```

```text
current session → stays active
other sessions  → revoked
```

That's a much nicer session-management API.

---

## One correction to the route above

Don't use `user.id` after moving it into a `move` closure and then try to use it for auditing afterward. Copy it first:

```rust
let user_id = user.id;
```

Then use `user_id` inside the closure and audit call.

Similarly, capture the current token hash before the closure.

---

## Final flow

```text
             Current browser
                   │
                   ▼
          POST /sessions/
            revoke-others
                   │
                   ▼
             Authenticated?
                   │
                   ▼
             CSRF valid?
                   │
                   ▼
          Find user's sessions
                   │
             ┌─────┴─────┐
             │           │
        current       others
             │           │
          KEEP        REVOKE
                         │
                         ▼
                  audit event
                         │
                         ▼
               revoked_count
```

### Next

We'll add **automatic session-revocation events** when a session expires or is revoked, then tighten the session query so revoked/idle-expired sessions are cleaned up consistently.

Next: let’s make **session revocation auditable and consistent**. We’ll record when a user explicitly revokes a session, logs out, logs out everywhere, or revokes other devices.

### 1. Add the missing audit events

In `src/auth/audit_events.rs`:

```rust
pub const SESSION_REVOKED: &str =
    "auth.session.revoked";

pub const SESSIONS_REVOKED_ALL: &str =
    "auth.sessions.revoked_all";

pub const OTHER_SESSIONS_REVOKED: &str =
    "auth.sessions.other_revoked";

pub const LOGOUT: &str =
    "auth.logout";

pub const LOGOUT_ALL: &str =
    "auth.logout_all";
```

---

### 2. Audit single-session revocation

In your `/auth/sessions/<id>` route, after successfully revoking the session:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::SESSION_REVOKED,
        ip_address: None,
        user_agent: None,
        metadata: Some(serde_json::json!({
            "session_id": session_id
        })),
    },
)
.await?;
```

`session_id` is okay to log because it is a database identifier, not an authentication secret.

**Do not log:**

```text
session token
token_hash
password
TOTP code
recovery code
reset token
verification token
```

---

### 3. Audit logout-all

After revoking all sessions:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGOUT_ALL,
        ip_address: None,
        user_agent: None,
        metadata: None,
    },
)
.await?;
```

Then remove the current cookie as you already do.

---

### 4. Audit logout

For normal logout:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGOUT,
        ip_address: None,
        user_agent: None,
        metadata: None,
    },
)
.await?;
```

Then:

```rust
cookies.remove(remove_session_cookie());
```

---

### 5. Audit “revoke other sessions”

Use:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::OTHER_SESSIONS_REVOKED,
        ip_address: None,
        user_agent: None,
        metadata: Some(serde_json::json!({
            "revoked_count": revoked_count
        })),
    },
)
.await?;
```

This gives you a useful security-history entry like:

```json
{
  "event_type": "auth.sessions.other_revoked",
  "metadata": {
    "revoked_count": 4
  }
}
```

---

## 6. Capture request metadata

Rather than leaving IP and user-agent empty, capture them consistently.

At the route level:

```rust
let ip_address = request
    .client_ip()
    .map(|ip| ip.to_string());

let user_agent = request
    .headers()
    .get_one("User-Agent")
    .map(str::to_string);
```

Then:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::OTHER_SESSIONS_REVOKED,
        ip_address: ip_address.as_deref(),
        user_agent: user_agent.as_deref(),
        metadata: Some(serde_json::json!({
            "revoked_count": revoked_count
        })),
    },
)
.await?;
```

Do the same for login, 2FA, password changes, password resets, and session revocation.

---

## 7. Important distinction: expiration vs revocation

Don't create an audit event every time the cleanup worker deletes an expired session.

For example:

```text
Session naturally expires
        ↓
cleanup worker deletes it
        ↓
NO individual audit event
```

Otherwise your audit table can grow enormously.

Explicit security actions should be audited:

```text
logout
logout-all
revoke session
revoke other sessions
password changed
password reset
2FA enabled
2FA disabled
recovery code used
```

Expiration can simply be represented by the session's `expires_at` and `idle_expires_at`.

---

## 8. One more important improvement

Your session cleanup currently deletes:

```rust
sessions::expires_at.le(now)
```

and:

```rust
sessions::idle_expires_at.le(now)
```

and old revoked sessions.

Keep that logic:

```rust
diesel::delete(
    sessions::table.filter(
        sessions::expires_at
            .le(now)
            .or(sessions::idle_expires_at.le(now))
            .or(
                sessions::revoked_at
                    .lt(retention_cutoff)
            )
    )
)
.execute(connection)
```

So the lifecycle is:

```text
             CREATE
                │
                ▼
             ACTIVE
             /     \
            /       \
       explicit     expiration
       revoke       or idle timeout
          │              │
          ▼              ▼
       REVOKED        EXPIRED
          │              │
          └──────┬───────┘
                 ▼
          cleanup worker
                 │
                 ▼
             database
              DELETE
```

That keeps the database from accumulating dead sessions while preserving recently revoked sessions for operational/security visibility.

**Next step:** we should wire the **IP + User-Agent audit metadata into login, 2FA, password reset, and session creation**, so the security history becomes genuinely useful.

Next: centralize **request metadata** so every authentication event gets the same IP address and User-Agent handling.

This avoids repeating the same code in every route.

### 1. Create `src/auth/request_metadata.rs`

```rust
use rocket::Request;

pub struct RequestMetadata {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl RequestMetadata {
    pub fn from_request(request: &Request<'_>) -> Self {
        Self {
            ip_address: request
                .client_ip()
                .map(|ip| ip.to_string()),

            user_agent: request
                .headers()
                .get_one("User-Agent")
                .map(str::to_string),
        }
    }

    pub fn ip(&self) -> Option<&str> {
        self.ip_address.as_deref()
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
}
```

Then export it in `src/auth/mod.rs`:

```rust
pub mod request_metadata;
```

---

### 2. Use it in login

At the beginning of your login route:

```rust
let metadata =
    RequestMetadata::from_request(request);
```

Then when creating the session:

```rust
let new_session = NewSession {
    id: Uuid::new_v4(),
    user_id: user.id,
    token_hash,
    expires_at,
    idle_expires_at,
    ip_address: metadata.ip_address.clone(),
    user_agent: metadata.user_agent.clone(),
    device_name: Some(
        device::device_name(
            metadata.user_agent.as_deref()
        )
    ),
};
```

This means every newly created session records:

```text
IP address
User-Agent
Device name
```

---

### 3. Audit successful login

After the session is created:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGIN_SUCCESS,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

---

### 4. Audit failed login

For an incorrect password:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::LOGIN_FAILED,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

For an unknown email, keep:

```rust
user_id: None
```

and **do not store the submitted email** in metadata.

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: None,
        event_type: audit_events::LOGIN_FAILED,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

That preserves useful security telemetry without turning your audit log into an account-enumeration database.

---

### 5. Use it for 2FA

At `/auth/2fa/verify`:

```rust
let metadata =
    RequestMetadata::from_request(request);
```

When 2FA succeeds:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(challenge.user_id),
        event_type: audit_events::LOGIN_SUCCESS,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: Some(serde_json::json!({
            "method": "totp"
        })),
    },
)
.await?;
```

And when the code is invalid:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(challenge.user_id),
        event_type: audit_events::TWO_FACTOR_FAILED,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

Never put the actual TOTP code into the metadata.

---

### 6. Recovery-code login

When a recovery code successfully authenticates the user:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(challenge.user_id),
        event_type: audit_events::RECOVERY_CODE_USED,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

You can also record the resulting login:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(challenge.user_id),
        event_type: audit_events::LOGIN_SUCCESS,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: Some(serde_json::json!({
            "method": "recovery_code"
        })),
    },
)
.await?;
```

---

### 7. Password change

For `/auth/password/change`:

```rust
let metadata =
    RequestMetadata::from_request(request);
```

After the password is successfully changed:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(user.id),
        event_type: audit_events::PASSWORD_CHANGED,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

---

### 8. Password reset

When `/auth/password/reset` succeeds:

```rust
audit::record_event(
    &mut db,
    audit::AuditEvent {
        user_id: Some(reset_token.user_id),
        event_type: audit_events::PASSWORD_RESET_COMPLETED,
        ip_address: metadata.ip(),
        user_agent: metadata.user_agent(),
        metadata: None,
    },
)
.await?;
```

For the forgot-password endpoint, even when the email doesn't exist, keep the external response generic.

---

### Result

Your security history can now look like:

```text
2026-09-18 20:41  auth.login.success
IP: 192.168.x.x
Device: Windows PC

2026-09-18 20:42  auth.2fa.failed
IP: 192.168.x.x
Device: Windows PC

2026-09-18 20:43  auth.login.success
method: totp

2026-09-18 21:10  auth.sessions.other_revoked
revoked_count: 3
```

The key architectural win is that **request metadata is now collected in one place**, rather than manually parsing headers throughout the authentication code.

Next we should tackle **secure session creation as a single reusable function**, so login, TOTP, recovery-code login, and session rotation all use exactly the same expiration, device metadata, and token-generation logic.

