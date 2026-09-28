use rocket::http::{Cookie, SameSite};

pub fn session_cookie(token: String) -> Cookie<'static> {
  Cookie::build(("session", token))
    .path("/")
    .http_only(true)
    .secure(false)
    .same_site(SameSite::Lax)
    .build()
}

pub fn remove_session_cookie() -> Cookie<'static> {
  Cookie::build(("session", ""))
    .path("/")
    .http_only(true)
    .secure(false)
    .same_site(SameSite::Lax)
    .build()
}