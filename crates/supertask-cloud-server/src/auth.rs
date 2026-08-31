use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use getrandom::getrandom;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use crate::error::AppError;

pub const ACCESS_SECS: i64 = 900;
pub const REFRESH_SECS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LoginResponse {
    pub account_id: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, serde::Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

pub async fn seed_account(pool: &SqlitePool, email: &str, password: &str) -> Result<(), AppError> {
    let hash = hash_password(password)?;
    let id = format!("acct-{}", hex_hash(email.as_bytes())[..24].to_string());
    sqlx::query("INSERT INTO accounts(id,email,password_hash,created_at,disabled) VALUES(?,?,?,?,0) ON CONFLICT(email) DO UPDATE SET password_hash=excluded.password_hash, disabled=0")
        .bind(id).bind(email).bind(hash).bind(now())
        .execute(pool).await?;
    Ok(())
}

pub async fn login(
    pool: &SqlitePool,
    req: LoginRequest,
    device: &str,
) -> Result<LoginResponse, AppError> {
    let email = req.email.trim().to_ascii_lowercase();
    if email.is_empty() || req.password.is_empty() {
        return Err(AppError::Unauthorized);
    }
    let row = sqlx::query("SELECT id,email,password_hash,disabled FROM accounts WHERE email=?")
        .bind(&email)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };
    let disabled: i64 = row.try_get("disabled")?;
    let hash: String = row.try_get("password_hash")?;
    let valid = disabled == 0
        && PasswordHash::new(&hash).ok().is_some_and(|parsed| {
            Argon2::default()
                .verify_password(req.password.as_bytes(), &parsed)
                .is_ok()
        });
    if !valid {
        return Err(AppError::Unauthorized);
    }
    issue_tokens(pool, row.try_get("id")?, row.try_get("email")?, device).await
}

pub async fn refresh(
    pool: &SqlitePool,
    req: RefreshRequest,
    device: &str,
) -> Result<LoginResponse, AppError> {
    let hash = hex_hash(req.refresh_token.as_bytes());
    let row = sqlx::query("SELECT r.account_id,r.device_id,a.email,a.disabled FROM refresh_tokens r JOIN accounts a ON a.id=r.account_id WHERE r.token_hash=? AND r.revoked_at IS NULL AND r.expires_at>? AND a.disabled=0")
        .bind(hash).bind(now()).fetch_optional(pool).await?;
    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };
    sqlx::query("UPDATE refresh_tokens SET revoked_at=? WHERE token_hash=?")
        .bind(now())
        .bind(hex_hash(req.refresh_token.as_bytes()))
        .execute(pool)
        .await?;
    issue_tokens(
        pool,
        row.try_get("account_id")?,
        row.try_get("email")?,
        device,
    )
    .await
}

async fn issue_tokens(
    pool: &SqlitePool,
    account_id: String,
    email: String,
    device: &str,
) -> Result<LoginResponse, AppError> {
    let access = random_token();
    let refresh = random_token();
    sqlx::query(
        "INSERT INTO access_tokens(token_hash,account_id,device_id,expires_at) VALUES(?,?,?,?)",
    )
    .bind(hex_hash(access.as_bytes()))
    .bind(&account_id)
    .bind(device)
    .bind(now() + ACCESS_SECS)
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO refresh_tokens(token_hash,account_id,device_id,expires_at,revoked_at) VALUES(?,?,?,?,NULL)")
        .bind(hex_hash(refresh.as_bytes())).bind(&account_id).bind(device).bind(now()+REFRESH_SECS).execute(pool).await?;
    Ok(LoginResponse {
        account_id,
        email,
        access_token: access,
        refresh_token: refresh,
        expires_in_secs: ACCESS_SECS as u64,
    })
}

pub async fn account_from_bearer(
    pool: &SqlitePool,
    header: Option<&str>,
) -> Result<String, AppError> {
    let token = header
        .and_then(|v| v.strip_prefix("Bearer ").map(str::trim))
        .filter(|v| !v.is_empty())
        .ok_or(AppError::Unauthorized)?;
    let row = sqlx::query("SELECT account_id FROM access_tokens JOIN accounts ON accounts.id=access_tokens.account_id WHERE token_hash=? AND expires_at>? AND disabled=0")
        .bind(hex_hash(token.as_bytes())).bind(now()).fetch_optional(pool).await?;
    row.map(|r| r.try_get("account_id"))
        .transpose()?
        .ok_or(AppError::Unauthorized)
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let mut salt = [0u8; 16];
    getrandom(&mut salt).map_err(|e| AppError::Internal(e.to_string()))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| AppError::Internal(e.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(e.to_string()))
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom(&mut bytes).expect("OS RNG");
    hex_hash(&bytes)
}

pub fn hex_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
