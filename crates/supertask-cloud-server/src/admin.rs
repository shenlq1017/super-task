use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::{auth, error::AppError};

pub const ROLE_USER: &str = "user";
pub const ROLE_ADMIN: &str = "admin";

const MIN_PASSWORD_CHARS: usize = 12;
const MAX_PASSWORD_BYTES: usize = 1024;
const MAX_EMAIL_BYTES: usize = 254;
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

#[derive(Debug, Clone)]
pub struct AdminActor {
    pub account_id: String,
}

#[derive(Debug, Serialize)]
pub struct AccountRow {
    pub id: String,
    pub email: String,
    pub role: String,
    pub disabled: bool,
    pub created_at: i64,
    pub entity_count: i64,
    pub entity_bytes: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateAccount {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetRole {
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct SetDisabled {
    pub disabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetPassword {
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AdminStatus {
    pub admin_available: bool,
    pub console_ready: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminMe {
    pub account_id: String,
    pub email: String,
    pub role: String,
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get("authorization").and_then(|v| v.to_str().ok())
}

/// Role gate for every admin entry point. Only reached after the caller presented a
/// valid credential, so distinguishing "you are not an admin" from "this deployment
/// has no admin yet" leaks nothing to unauthenticated probes.
async fn ensure_admin_role(pool: &SqlitePool, account_id: &str) -> Result<(), AppError> {
    if account_role(pool, account_id).await? == ROLE_ADMIN {
        return Ok(());
    }
    if any_enabled_admin(pool).await? {
        Err(AppError::AdminForbidden)
    } else {
        Err(AppError::AdminNotConfigured)
    }
}

fn valid_email(email: &str) -> bool {
    if email.is_empty()
        || email.len() > MAX_EMAIL_BYTES
        // `%` would make the operator's substring search ambiguous against stored rows.
        || email.contains('%')
        || email.bytes().any(|b| {
            b.is_ascii_whitespace() || b.is_ascii_control() || matches!(b, b'"' | b',' | b';' | b':')
        })
    {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.starts_with('-')
        && !domain.ends_with('-')
        && !domain.contains('@')
}

fn check_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(AppError::BadRequest(format!(
            "口令至少 {MIN_PASSWORD_CHARS} 个字符"
        )));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(AppError::BadRequest("口令过长".into()));
    }
    Ok(())
}

fn normalize_role(role: Option<&str>) -> Result<String, AppError> {
    match role.unwrap_or(ROLE_USER) {
        ROLE_USER => Ok(ROLE_USER.to_string()),
        ROLE_ADMIN => Ok(ROLE_ADMIN.to_string()),
        _ => Err(AppError::BadRequest("role 只接受 user 或 admin".into())),
    }
}

fn account_id_for(email: &str) -> String {
    format!("acct-{}", &auth::hex_hash(email.as_bytes())[..24])
}

const COUNT_ENABLED_ADMINS: &str = "SELECT COUNT(*) FROM accounts WHERE role=? AND disabled=0";

async fn any_enabled_admin(pool: &SqlitePool) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(COUNT_ENABLED_ADMINS)
        .bind(ROLE_ADMIN)
        .fetch_one(pool)
        .await?;
    Ok(count > 0)
}

async fn enabled_admin_count(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(COUNT_ENABLED_ADMINS)
        .bind(ROLE_ADMIN)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Returns `(role, disabled)` for the target account or `NotFound`.
async fn target_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> Result<(String, bool), AppError> {
    let row = sqlx::query("SELECT role,disabled FROM accounts WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let disabled: i64 = row.try_get("disabled")?;
    Ok((row.try_get("role")?, disabled != 0))
}

fn audit(actor: &AdminActor, action: &str, target: &str) {
    tracing::info!(actor = %actor.account_id, action, target, "cloud admin mutation");
}

pub async fn require_admin(pool: &SqlitePool, headers: &HeaderMap) -> Result<AdminActor, AppError> {
    let account_id = auth::account_from_bearer(pool, bearer(headers)).await?;
    ensure_admin_role(pool, &account_id).await?;
    Ok(AdminActor { account_id })
}

/// Login that only succeeds for accounts holding the admin role. A credential error
/// stays indistinguishable from `/auth/login`; the role error is only reported after
/// the caller proved ownership of that account.
pub async fn admin_login(
    pool: &SqlitePool,
    req: auth::LoginRequest,
    device: &str,
) -> Result<auth::LoginResponse, AppError> {
    let issued = auth::login(pool, req, device).await?;
    // auth::login already minted a short-lived account token. It is dropped here
    // and grants no admin power: require_admin re-checks role on every request.
    ensure_admin_role(pool, &issued.account_id).await?;
    Ok(issued)
}

pub async fn admin_refresh(
    pool: &SqlitePool,
    req: auth::RefreshRequest,
    device: &str,
) -> Result<auth::LoginResponse, AppError> {
    let issued = auth::refresh(pool, req, device).await?;
    ensure_admin_role(pool, &issued.account_id).await?;
    Ok(issued)
}

pub async fn account_role(pool: &SqlitePool, account_id: &str) -> Result<String, AppError> {
    let row = sqlx::query("SELECT role FROM accounts WHERE id=?")
        .bind(account_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(row.try_get("role")?)
}

pub async fn status(
    pool: &SqlitePool,
    config: &crate::config::Config,
) -> Result<AdminStatus, AppError> {
    Ok(AdminStatus {
        admin_available: any_enabled_admin(pool).await?,
        console_ready: config.console_ready(),
    })
}

pub async fn me(pool: &SqlitePool, headers: &HeaderMap) -> Result<AdminMe, AppError> {
    let actor = require_admin(pool, headers).await?;
    let row = sqlx::query("SELECT email,role FROM accounts WHERE id=?")
        .bind(&actor.account_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(AdminMe {
        account_id: actor.account_id,
        email: row.try_get("email")?,
        role: row.try_get("role")?,
    })
}

const ACCOUNT_SELECT: &str = "SELECT a.id,a.email,a.role,a.disabled,a.created_at,\
    (SELECT COUNT(*) FROM entities e WHERE e.account_id=a.id) AS entity_count,\
    (SELECT COALESCE(SUM(e.byte_size),0) FROM entities e WHERE e.account_id=a.id) AS entity_bytes \
    FROM accounts a";

pub async fn list(
    pool: &SqlitePool,
    query: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<AccountRow>, AppError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    let rows = match query.map(str::trim).filter(|value| !value.is_empty()) {
        Some(needle) => {
            // `%` cannot appear in a stored email, so no LIKE escape is required.
            let pattern = format!("%{needle}%");
            sqlx::query(&format!(
                "{ACCOUNT_SELECT} WHERE a.email LIKE ? OR a.id LIKE ? \
                 ORDER BY a.created_at DESC, a.id LIMIT ? OFFSET ?"
            ))
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query(&format!(
                "{ACCOUNT_SELECT} ORDER BY a.created_at DESC, a.id LIMIT ? OFFSET ?"
            ))
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
    };
    rows.into_iter().map(row_account).collect()
}

pub async fn get_one(pool: &SqlitePool, id: &str) -> Result<AccountRow, AppError> {
    let row = sqlx::query(&format!("{ACCOUNT_SELECT} WHERE a.id=?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    row_account(row)
}

fn row_account(row: sqlx::sqlite::SqliteRow) -> Result<AccountRow, AppError> {
    let disabled: i64 = row.try_get("disabled")?;
    Ok(AccountRow {
        id: row.try_get("id")?,
        email: row.try_get("email")?,
        role: row.try_get("role")?,
        disabled: disabled != 0,
        created_at: row.try_get("created_at")?,
        entity_count: row.try_get("entity_count")?,
        entity_bytes: row.try_get("entity_bytes")?,
    })
}

/// Identity bootstrap: idempotent upsert that always ends with an enabled admin.
pub async fn bootstrap_admin(
    pool: &SqlitePool,
    email: &str,
    password: &str,
) -> Result<(), AppError> {
    let email = email.trim().to_ascii_lowercase();
    if !valid_email(&email) {
        return Err(AppError::BadRequest(
            "SUPERTASK_ADMIN_EMAIL 不是合法邮箱".into(),
        ));
    }
    check_password(password)?;
    let hash = auth::hash_password(password)?;
    sqlx::query("INSERT INTO accounts(id,email,password_hash,created_at,disabled,role) VALUES(?,?,?,?,0,?) ON CONFLICT(email) DO UPDATE SET password_hash=excluded.password_hash, disabled=0, role=excluded.role")
        .bind(account_id_for(&email))
        .bind(&email)
        .bind(hash)
        .bind(auth::now())
        .bind(ROLE_ADMIN)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn create(pool: &SqlitePool, req: CreateAccount) -> Result<AccountRow, AppError> {
    let email = req.email.trim().to_ascii_lowercase();
    if !valid_email(&email) {
        return Err(AppError::BadRequest("邮箱格式无效".into()));
    }
    check_password(&req.password)?;
    let role = normalize_role(req.role.as_deref())?;
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE email=?")
        .bind(&email)
        .fetch_one(pool)
        .await?;
    if exists > 0 {
        return Err(AppError::BadRequest("该邮箱已存在账号".into()));
    }
    let id = account_id_for(&email);
    sqlx::query(
        "INSERT INTO accounts(id,email,password_hash,created_at,disabled,role) VALUES(?,?,?,?,0,?)",
    )
    .bind(&id)
    .bind(&email)
    .bind(auth::hash_password(&req.password)?)
    .bind(auth::now())
    .bind(&role)
    .execute(pool)
    .await?;
    get_one(pool, &id).await
}

pub async fn set_role(
    pool: &SqlitePool,
    actor: &AdminActor,
    id: &str,
    req: SetRole,
) -> Result<AccountRow, AppError> {
    let role = normalize_role(Some(&req.role))?;
    let mut tx = pool.begin().await?;
    let (current, disabled) = target_account(&mut tx, id).await?;
    if disabled {
        return Err(AppError::BadRequest(
            "该账号已停用，请先启用后再修改角色".into(),
        ));
    }
    if role != ROLE_ADMIN {
        if actor.account_id == id {
            return Err(AppError::BadRequest("不能撤销自己的管理员角色".into()));
        }
        if current == ROLE_ADMIN && enabled_admin_count(&mut tx).await? <= 1 {
            return Err(AppError::BadRequest("必须保留至少一个启用的管理员".into()));
        }
    }
    sqlx::query("UPDATE accounts SET role=? WHERE id=?")
        .bind(&role)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit(actor, "set_role", id);
    get_one(pool, id).await
}

pub async fn set_disabled(
    pool: &SqlitePool,
    actor: &AdminActor,
    id: &str,
    req: SetDisabled,
) -> Result<AccountRow, AppError> {
    let mut tx = pool.begin().await?;
    let (role, _) = target_account(&mut tx, id).await?;
    if req.disabled {
        if actor.account_id == id {
            return Err(AppError::BadRequest("不能停用当前登录的管理员账号".into()));
        }
        if role == ROLE_ADMIN && enabled_admin_count(&mut tx).await? <= 1 {
            return Err(AppError::BadRequest("必须保留至少一个启用的管理员".into()));
        }
    }
    sqlx::query("UPDATE accounts SET disabled=? WHERE id=?")
        .bind(i64::from(req.disabled))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit(actor, if req.disabled { "disable" } else { "enable" }, id);
    get_one(pool, id).await
}

pub async fn set_password(
    pool: &SqlitePool,
    actor: &AdminActor,
    id: &str,
    req: SetPassword,
) -> Result<(), AppError> {
    check_password(&req.password)?;
    let found = sqlx::query("UPDATE accounts SET password_hash=? WHERE id=?")
        .bind(auth::hash_password(&req.password)?)
        .bind(id)
        .execute(pool)
        .await?;
    if found.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    // Only the sign-in secret changed; issued tokens stay valid. Session revocation
    // is a separate capability and out of scope for this round.
    audit(actor, "set_password", id);
    Ok(())
}

pub async fn delete(pool: &SqlitePool, actor: &AdminActor, id: &str) -> Result<(), AppError> {
    if actor.account_id == id {
        return Err(AppError::BadRequest("不能删除当前登录的管理员账号".into()));
    }
    let mut tx = pool.begin().await?;
    let (role, disabled) = target_account(&mut tx, id).await?;
    // A disabled admin is not part of `enabled_admin_count`, so deleting it can never
    // be the operation that leaves the deployment without an operator.
    if role == ROLE_ADMIN && !disabled && enabled_admin_count(&mut tx).await? <= 1 {
        return Err(AppError::BadRequest("必须保留至少一个启用的管理员".into()));
    }
    sqlx::query("DELETE FROM accounts WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit(actor, "delete", id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_guard_accepts_and_rejects() {
        assert!(valid_email("demo@supertask.local"));
        assert!(valid_email("a.b+c@x-io.io"));
        assert!(!valid_email(""));
        assert!(!valid_email("no-at-sign"));
        assert!(!valid_email("@supertask.local"));
        assert!(!valid_email("demo@"));
        assert!(!valid_email("demo@local"));
        assert!(!valid_email("demo@.local"));
        assert!(!valid_email("demo@local."));
        assert!(!valid_email("de mo@local.io"));
        assert!(!valid_email("demo%2@local.io"));
        assert!(!valid_email("a@b@c.io"));
        assert!(!valid_email("demo@-local.io"));
    }

    #[test]
    fn password_and_role_guards() {
        assert!(check_password("short1234").is_err());
        assert!(check_password("long-enough-password").is_ok());
        assert_eq!(normalize_role(None).unwrap(), ROLE_USER);
        assert_eq!(normalize_role(Some("admin")).unwrap(), ROLE_ADMIN);
        assert!(normalize_role(Some("superuser")).is_err());
        assert!(normalize_role(Some("Admin")).is_err());
    }

    #[test]
    fn account_id_is_stable_per_email() {
        assert_eq!(account_id_for("a@b.io"), account_id_for("a@b.io"));
        assert_ne!(account_id_for("a@b.io"), account_id_for("c@d.io"));
        assert_eq!(
            account_id_for("a@b.io"),
            account_id_for("A@B.IO".to_lowercase().as_str())
        );
        assert!(account_id_for("a@b.io").starts_with("acct-"));
        assert_eq!(account_id_for("a@b.io").len(), "acct-".len() + 24);
    }
}
