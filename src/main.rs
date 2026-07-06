#[macro_use]
extern crate rocket;

use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use rocket::State;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use std::sync::Mutex;

// --- "database" (single account, in memory) ---

struct Account {
    username: String,
    password_hash: String,
}

struct AppState {
    user: Mutex<Option<Account>>,
}

// --- request/response bodies ---

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct Message {
    message: String,
}

// --- request guard: protects routes behind a valid session cookie ---

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

// --- routes ---

#[get("/health")]
fn health() -> &'static str {
    "OK"
}

#[post("/register", data = "<creds>")]
fn register(creds: Json<Credentials>, state: &State<AppState>) -> Status {
    let mut user = state.user.lock().unwrap();

    if user.is_some() {
        // Single-user app: refuse to overwrite an existing account.
        return Status::Conflict;
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(creds.password.as_bytes(), &salt)
        .expect("hashing should not fail")
        .to_string();

    *user = Some(Account {
        username: creds.username.clone(),
        password_hash,
    });

    Status::Created
}

#[post("/login", data = "<creds>")]
fn login(creds: Json<Credentials>, state: &State<AppState>, cookies: &CookieJar<'_>) -> Status {
    let user = state.user.lock().unwrap();

    let account = match &*user {
        Some(account) if account.username == creds.username => account,
        _ => return Status::Unauthorized,
    };

    let parsed_hash = PasswordHash::new(&account.password_hash).expect("stored hash is valid");
    match Argon2::default().verify_password(creds.password.as_bytes(), &parsed_hash) {
        Ok(()) => {
            cookies.add_private(Cookie::new("session", account.username.clone()));
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

#[launch]
fn rocket() -> _ {
    rocket::build()
        .manage(AppState {
            user: Mutex::new(None),
        })
        .mount("/", routes![health, register, login, logout, protected])
}
