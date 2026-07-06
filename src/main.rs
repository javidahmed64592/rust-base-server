#[macro_use]
extern crate rocket;

use argon2::password_hash::PasswordVerifier;
use argon2::{Argon2, PasswordHash};
use rocket::fairing::{self, AdHoc};
use rocket::http::{Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::{Build, Rocket};
use rocket_db_pools::{Connection, Database, sqlx};

#[derive(Database)]
#[database("users_db")]
struct UsersDb(sqlx::SqlitePool);

#[derive(Database)]
#[database("app_db")]
struct AppDb(sqlx::SqlitePool);

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct Message {
    message: String,
}

struct AuthenticatedUser {
    username: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.cookies().get_private("session") {
            Some(cookie) => Outcome::Success(AuthenticatedUser {
                username: cookie.value().to_string(),
            }),
            None => Outcome::Error((Status::Unauthorized, ())),
        }
    }
}

#[get("/health")]
fn health() -> &'static str {
    "OK"
}

#[post("/login", data = "<creds>")]
async fn login(
    creds: Json<Credentials>,
    mut db: Connection<UsersDb>,
    cookies: &CookieJar<'_>,
) -> Status {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT username, password_hash FROM users WHERE username = ?")
            .bind(&creds.username)
            .fetch_optional(&mut **db)
            .await
            .unwrap_or(None);

    let (username, password_hash) = match row {
        Some(row) => row,
        None => return Status::Unauthorized,
    };

    let parsed_hash = match PasswordHash::new(&password_hash) {
        Ok(hash) => hash,
        Err(_) => return Status::InternalServerError,
    };

    match Argon2::default().verify_password(creds.password.as_bytes(), &parsed_hash) {
        Ok(()) => {
            cookies.add_private(Cookie::new("session", username));
            Status::Ok
        }
        Err(_) => Status::Unauthorized,
    }
}

#[post("/logout")]
fn logout(cookies: &CookieJar<'_>) -> Status {
    cookies.remove_private("session");
    Status::Ok
}

#[get("/protected")]
fn protected(user: AuthenticatedUser) -> Json<Message> {
    Json(Message {
        message: format!("Hello, {}! This is protected data.", user.username),
    })
}

// app_db owns its own schema; safe to run on every startup.
async fn init_app_db(rocket: Rocket<Build>) -> fairing::Result {
    match AppDb::fetch(&rocket) {
        Some(db) => {
            let result = sqlx::query(
                "CREATE TABLE IF NOT EXISTS pi_bot_memory (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    content TEXT NOT NULL
                )",
            )
            .execute(&**db)
            .await;

            match result {
                Ok(_) => Ok(rocket),
                Err(e) => {
                    error!("Failed to initialize app_db: {}", e);
                    Err(rocket)
                }
            }
        }
        None => Err(rocket),
    }
}

// Fails fast, before the server binds a port, if users_db hasn't been created yet.
fn ensure_users_db_exists() {
    let figment = rocket::Config::figment();
    let url: String = figment
        .extract_inner("databases.users_db.url")
        .expect("set databases.users_db.url in Rocket.toml or ROCKET_DATABASES env var");

    // Strip the sqlite `file:...?mode=ro` wrapper down to a bare path for the existence check.
    let path = url
        .trim_start_matches("file:")
        .split('?')
        .next()
        .unwrap_or(&url);

    if !std::path::Path::new(path).exists() {
        eprintln!(
            "Users database not found at '{}'.\nRun the create-user tool first to create it.",
            path
        );
        std::process::exit(1);
    }
}

#[launch]
fn rocket() -> _ {
    ensure_users_db_exists();

    rocket::build()
        .attach(UsersDb::init())
        .attach(AppDb::init())
        .attach(AdHoc::try_on_ignite("App DB Init", init_app_db))
        .mount("/", routes![health, login, logout, protected])
}
