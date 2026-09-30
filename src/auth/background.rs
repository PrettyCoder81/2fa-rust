use std::time::Duration;

use rocket::tokio::time::interval;

use crate::{DbConn, auth::session_cleanup::cleanup_sessions};

#[allow(dead_code)]
pub async fn session_cleanup_loop(rocket: rocket::Rocket<rocket::Orbit>) {
    let mut timer = interval(Duration::from_secs(60 * 60));

    loop {
        timer.tick().await;

        match DbConn::get_one(&rocket).await {
            Some(db) => match db.run(cleanup_sessions).await {
                Ok(deleted) => {
                    if deleted > 0 {
                        println!("Session cleanup removed {} sessions", deleted);
                    }
                }

                Err(error) => {
                    eprintln!("Session cleanup failed: {}", error);
                }
            },

            None => {
                eprintln!("Session cleanup could not acquire database connection");
            }
        }
    }
}

