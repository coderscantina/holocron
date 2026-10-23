//! Who is calling: session cookies, API keys, share tokens, password hashing, second factors
//! and the account records behind them. Owns `auth.*`, `account.*`, `twoFactor.*`,
//! `apiKeys.*`, `users.*` and `security.*`.

/// SQL condition, true when a 2FA policy covers the user whose ID is in column `$user`: the
/// installation's, the account's own (`u` must be the user's `users` row) or any of their teams'.
macro_rules! two_factor_required_sql {
    ($user:literal) => {
        concat!(
            "(u.two_factor_required = 1
              OR (SELECT two_factor_required FROM installation_settings WHERE id = 1) = 1
              OR EXISTS (SELECT 1 FROM memberships m JOIN workspaces w ON w.id = m.workspace_id
                         WHERE m.user_id = ",
            $user,
            " AND w.two_factor_required = 1))"
        )
    };
}

mod account;
mod api_keys;
mod auth;
mod caller;
mod limiter;
pub mod secrets;
mod security;
pub mod totp;
mod two_factor;
mod users;

pub use caller::{Caller, KeyCaller, has_api_key, resolve_caller};
pub use holocron_protocol::limits::{
    PASSWORD_MIN_CHARS as MIN_PASSWORD_CHARS, SESSION_LIFETIME_MS,
};
pub use limiter::{ChallengeAttempts, RateLimiter};
pub use secrets::SECRETS_KEY_FILE;

use std::sync::OnceLock;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use holocron_protocol::access::InstallRole;
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::UserId;
use holocron_protocol::limits::{ACCOUNT_NAME_MAX_CHARS, EMAIL_MAX_CHARS, PASSWORD_MAX_CHARS};
use holocron_protocol::operations::auth::Account;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::db::{new_id, now_ms};
use crate::error::{ApiError, FieldErrors};

pub const SESSION_COOKIE: &str = "holocron_session";
/// Carries the login challenge between a correct password and the second factor.
pub const CHALLENGE_COOKIE: &str = "holocron_challenge";
const GENERATED_PASSWORD_CHARS: usize = 24;

/// A new random token (256 bits, base64url) and its digest, the only form stored. Used for
/// sessions, challenges, key secrets, share tokens and invitation codes.
pub fn new_session_token() -> (String, String) {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS provides randomness");
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let digest = token_digest(&token);
    (token, digest)
}

/// SHA-256 hex of a high-entropy token: session IDs, key secrets, share tokens, codes.
pub fn token_digest(token: &str) -> String {
    let hash = Sha256::digest(token.as_bytes());
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn hash_password(password: &str) -> String {
    Argon2::default()
        .hash_password(password.as_bytes())
        .expect("argon2 hashes any password with default parameters")
        .to_string()
}

/// Runs on the blocking pool: argon2 is deliberately slow. Accounts imported from Umami keep
/// their bcrypt hash until their first login replaces it ([`is_imported_hash`], decision 0013).
pub async fn verify_password(hash: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || {
        if is_imported_hash(&hash) {
            return bcrypt::verify(password.as_bytes(), &hash).unwrap_or(false);
        }
        Argon2::default()
            .verify_password(password.as_bytes(), hash.as_str())
            .is_ok()
    })
    .await
    .unwrap_or(false)
}

/// A bcrypt hash as Umami stores it, replaced with argon2 at the next successful login.
pub fn is_imported_hash(hash: &str) -> bool {
    hash.starts_with("$2")
}

/// Verified when the email is unknown, so both failures take the same time.
fn decoy_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("decoy password that never matches"))
        .clone()
}

/// 24 characters from a 62-character alphabet, about 143 bits.
pub fn generate_password() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut password = String::with_capacity(GENERATED_PASSWORD_CHARS);
    while password.len() < GENERATED_PASSWORD_CHARS {
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).expect("the OS provides randomness");
        // 248 is the largest multiple of 62 below 256, so every character is equally likely.
        if byte[0] < 248 {
            password.push(ALPHABET[(byte[0] % 62) as usize] as char);
        }
    }
    password
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub async fn create_user(
    conn: &mut SqliteConnection,
    email: &str,
    name: &str,
    password: &str,
    install_role: InstallRole,
) -> Result<UserId, sqlx::Error> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO users (id, email, name, password_hash, install_role, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(normalize_email(email))
    .bind(name)
    .bind(hash_password(password))
    .bind(install_role.as_str())
    .bind(now_ms())
    .execute(conn)
    .await?;
    Ok(UserId::new(id))
}

#[derive(sqlx::FromRow)]
struct AccountRow {
    id: String,
    email: String,
    name: String,
    install_role: String,
}

impl AccountRow {
    fn into_account(self) -> Account {
        Account {
            id: UserId::new(self.id),
            email: self.email,
            name: self.name,
            install_role: InstallRole::parse(&self.install_role).unwrap_or(InstallRole::User),
        }
    }
}

pub async fn account(
    conn: &mut SqliteConnection,
    user: &UserId,
) -> Result<Option<Account>, sqlx::Error> {
    let row: Option<AccountRow> =
        sqlx::query_as("SELECT id, email, name, install_role FROM users WHERE id = ?")
            .bind(user.as_str())
            .fetch_optional(conn)
            .await?;
    Ok(row.map(AccountRow::into_account))
}

/// The account and password hash for an email, or the decoy hash when the email is unknown.
async fn credentials(
    conn: &mut SqliteConnection,
    email: &str,
) -> Result<(Option<Account>, String), sqlx::Error> {
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, email, name, install_role, password_hash FROM users WHERE email = ?",
    )
    .bind(normalize_email(email))
    .fetch_optional(conn)
    .await?;
    Ok(match row {
        Some((id, email, name, install_role, hash)) => (
            Some(
                AccountRow {
                    id,
                    email,
                    name,
                    install_role,
                }
                .into_account(),
            ),
            hash,
        ),
        None => (None, decoy_hash()),
    })
}

/// A person's second factor: confirmed or not, and whether a policy requires one.
pub(crate) struct TwoFactorState {
    pub enrolled: bool,
    pub required: bool,
}

pub(crate) async fn two_factor_state(
    conn: &mut SqliteConnection,
    user: &UserId,
) -> Result<TwoFactorState, sqlx::Error> {
    let (enrolled, required): (bool, bool) = sqlx::query_as(concat!(
        "SELECT EXISTS (SELECT 1 FROM two_factor t WHERE t.user_id = u.id AND t.secret IS NOT NULL), ",
        two_factor_required_sql!("u.id"),
        " FROM users u WHERE u.id = ?"
    ))
    .bind(user.as_str())
    .fetch_one(conn)
    .await?;
    Ok(TwoFactorState { enrolled, required })
}

/// The password hash of an existing account.
async fn password_hash(conn: &mut SqliteConnection, user: &UserId) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.as_str())
        .fetch_one(conn)
        .await
}

/// [`hash_password`] on the blocking pool, so a command does not stall other requests.
async fn hash_password_blocking(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(ApiError::internal)
}

/// Ends every session of `user` except `keep`, and any login challenge between password and
/// second factor. Returns the number of sessions ended.
async fn end_sessions(
    conn: &mut SqliteConnection,
    user: &UserId,
    keep: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let ended = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id IS NOT ?")
        .bind(user.as_str())
        .bind(keep)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    sqlx::query("DELETE FROM login_challenges WHERE user_id = ?")
        .bind(user.as_str())
        .execute(conn)
        .await?;
    Ok(ended)
}

/// A trimmed name and a normalized email, checked as `account.update`, `users.create` and
/// `users.update` take them. An email another account holds is `invalid_value` on `email`.
async fn validate_profile(
    conn: &mut SqliteConnection,
    name: &str,
    email: &str,
    except: Option<&UserId>,
) -> Result<(String, String), ApiError> {
    let name = name.trim().to_owned();
    let email = normalize_email(email);
    let mut errors = FieldErrors::default();
    match name.chars().count() {
        0 => errors.add("name", FieldErrorCode::Required, None),
        n if n > ACCOUNT_NAME_MAX_CHARS => errors.add(
            "name",
            FieldErrorCode::TooLong,
            Some(ACCOUNT_NAME_MAX_CHARS as u32),
        ),
        _ => {}
    }
    if let Some((code, limit)) = email_error(&email) {
        errors.add("email", code, limit);
    } else {
        let taken: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM users WHERE email = ? AND id IS NOT ?")
                .bind(&email)
                .bind(except.map(UserId::as_str))
                .fetch_optional(conn)
                .await?;
        if taken.is_some() {
            errors.add("email", FieldErrorCode::InvalidValue, None);
        }
    }
    errors.finish()?;
    Ok((name, email))
}

/// What is wrong with a normalized email address, before anyone checks it is free.
pub(crate) fn email_error(email: &str) -> Option<(FieldErrorCode, Option<u32>)> {
    let well_formed = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
        && !email.chars().any(char::is_whitespace);
    if email.is_empty() {
        Some((FieldErrorCode::Required, None))
    } else if email.chars().count() > EMAIL_MAX_CHARS {
        Some((FieldErrorCode::TooLong, Some(EMAIL_MAX_CHARS as u32)))
    } else if !well_formed {
        Some((FieldErrorCode::InvalidFormat, None))
    } else {
        None
    }
}

/// Creates a login session for `user` and returns the token for its cookie.
pub(crate) async fn insert_session(
    conn: &mut SqliteConnection,
    user: &UserId,
    now: i64,
) -> Result<String, sqlx::Error> {
    let (token, session) = new_session_token();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, created_at, expires_at, last_seen_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&session)
    .bind(user.as_str())
    .bind(now)
    .bind(now + SESSION_LIFETIME_MS)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(token)
}

/// Signs in the admin setup created while setup mode waits for an import (decision 0021) and
/// answers the session token. Only an install admin with a Holocron password hash qualifies:
/// imported Umami accounts keep their bcrypt hash until their first sign-in after setup. Setup
/// mode asks for no second factor, so an account with one is `forbidden` here.
pub(crate) async fn setup_sign_in(
    conn: &mut SqliteConnection,
    email: &str,
    password: String,
    now: i64,
) -> Result<(String, Account), ApiError> {
    let (account, hash) = credentials(&mut *conn, email).await?;
    let imported = is_imported_hash(&hash);
    let verified = verify_password(hash, password).await;
    let Some(account) = account.filter(|account| {
        verified && !imported && account.install_role == InstallRole::InstallAdmin
    }) else {
        return Err(ApiError::new(ErrorCode::InvalidCredentials));
    };
    if two_factor_state(&mut *conn, &account.id).await?.enrolled {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    let token = insert_session(conn, &account.id, now).await?;
    Ok((token, account))
}

/// `invalid_arguments` on `field` unless the password has 12 to 256 characters.
pub(crate) fn validate_password(field: &str, password: &str) -> Result<(), ApiError> {
    match password.chars().count() {
        n if n < MIN_PASSWORD_CHARS => Err(FieldErrors::one(
            field,
            FieldErrorCode::TooShort,
            Some(MIN_PASSWORD_CHARS as u32),
        )),
        n if n > PASSWORD_MAX_CHARS => Err(FieldErrors::one(
            field,
            FieldErrorCode::TooLong,
            Some(PASSWORD_MAX_CHARS as u32),
        )),
        _ => Ok(()),
    }
}

/// The signed-in person. Account operations are theirs alone; the registry keeps keys and
/// share viewers out before a handler runs.
fn signed_in(caller: &Caller) -> Result<UserId, ApiError> {
    caller
        .user()
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))
}
