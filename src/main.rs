#[macro_use]
extern crate rocket;

mod auth;
mod faring;
mod models;
mod routes;
mod schema;

use rocket_sync_db_pools::{database, diesel};

use crate::routes::auth::{
    change_password, confirm_2fa, csrf_token, disable_2fa, list_sessions, login, logout,
    logout_all, options, profile, regenerate_recovery_codes, register, revoke_session, setup_2fa,
    verify_2fa, verify_recovery_code,
};

#[database("postgres")]
pub struct DbConn(diesel::PgConnection);

#[get("/")]
async fn index(conn: DbConn) -> String {
    conn.run(|_| "Connected to PostgreSQL!".to_string()).await
}

#[launch]
fn rocket() -> _ {
    dotenvy::dotenv().ok();

    rocket::build()
        .attach(DbConn::fairing())
        .attach(faring::cors::Cors)
        .mount("/", routes![options])
        .mount(
            "/api",
            routes![
                index,
                csrf_token,
                register,
                login,
                profile,
                logout,
                logout_all,
                setup_2fa,
                confirm_2fa,
                verify_2fa,
                disable_2fa,
                verify_recovery_code,
                regenerate_recovery_codes,
                list_sessions,
                revoke_session,
                change_password
            ],
        )
}
