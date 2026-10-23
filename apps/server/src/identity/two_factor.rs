//! `twoFactor.*` operations: TOTP enrollment, confirmation, recovery codes and removal, and the
//! second-factor check that login and reauthentication share.

use std::path::Path;

use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::UserId;
use holocron_protocol::limits::{TOTP_DIGITS, TOTP_PERIOD_SECONDS};
use holocron_protocol::operations::auth::SecondFactorProof;
use holocron_protocol::operations::two_factor::{
    RecoveryCodes, TotpSecret, TwoFactorConfirmArgs, TwoFactorEnrollment, TwoFactorHandlers,
    TwoFactorStatus,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;
use url::form_urlencoded::byte_serialize;

use super::secrets::SecretKey;
use super::{account, signed_in, totp, two_factor_state};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

impl TwoFactorHandlers for AppState {
    async fn two_factor_begin(
        &self,
        ctx: &mut CommandCtx,
        _args: Empty,
    ) -> Result<TwoFactorEnrollment, ApiError> {
        let user = signed_in(ctx.caller())?;
        require_fresh_login_to_replace(ctx, &user).await?;
        let email = account(ctx.conn(), &user)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?
            .email;
        let secret = totp::new_secret();
        let sealed = secret_key(&self.config.data_dir)?.seal(&user, &secret);
        sqlx::query(
            "INSERT INTO two_factor (user_id, pending_secret, pending_created_at) VALUES (?, ?, ?)
             ON CONFLICT (user_id) DO UPDATE SET pending_secret = excluded.pending_secret,
                                                 pending_created_at = excluded.pending_created_at",
        )
        .bind(user.as_str())
        .bind(sealed)
        .bind(ctx.now())
        .execute(ctx.conn())
        .await?;

        let key = totp::base32(&secret);
        let issuer = encode(&self.config.instance_name);
        let uri = format!(
            "otpauth://totp/{issuer}:{}?secret={key}&issuer={issuer}&algorithm=SHA1&digits={TOTP_DIGITS}&period={TOTP_PERIOD_SECONDS}",
            encode(&email)
        );
        Ok(TwoFactorEnrollment {
            secret: Some(TotpSecret { key, uri }),
            secret_withheld: false,
        })
    }

    async fn two_factor_confirm(
        &self,
        ctx: &mut CommandCtx,
        args: TwoFactorConfirmArgs,
    ) -> Result<RecoveryCodes, ApiError> {
        let user = signed_in(ctx.caller())?;
        require_fresh_login_to_replace(ctx, &user).await?;
        let pending: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT pending_secret FROM two_factor WHERE user_id = ? AND pending_secret IS NOT NULL",
        )
        .bind(user.as_str())
        .fetch_optional(ctx.conn())
        .await?;
        let pending =
            pending.ok_or_else(|| FieldErrors::one("code", FieldErrorCode::InvalidValue, None))?;
        let secret = secret_key(&self.config.data_dir)?
            .open(&user, &pending)
            .ok_or_else(|| ApiError::new(ErrorCode::SecondFactorInvalid))?;
        let now = ctx.now();
        let step = totp::verify(&secret, &args.code, now, None)
            .ok_or_else(|| ApiError::new(ErrorCode::SecondFactorInvalid))?;

        sqlx::query(
            "UPDATE two_factor SET secret = pending_secret, confirmed_at = ?, last_step = ?,
                                   pending_secret = NULL, pending_created_at = NULL
             WHERE user_id = ?",
        )
        .bind(now)
        .bind(step)
        .bind(user.as_str())
        .execute(ctx.conn())
        .await?;
        let codes = replace_recovery_codes(ctx.conn(), &user).await?;
        ctx.record(&Topic::user(&user), "two_factor.enrolled", json!({}))
            .await?;
        Ok(RecoveryCodes {
            secret: Some(codes),
            secret_withheld: false,
        })
    }

    async fn two_factor_disable(
        &self,
        ctx: &mut CommandCtx,
        _args: Empty,
    ) -> Result<TwoFactorStatus, ApiError> {
        let user = signed_in(ctx.caller())?;
        ctx.require_fresh_login()?;
        let removed = remove(ctx.conn(), &user).await?;
        if removed {
            ctx.record(&Topic::user(&user), "two_factor.removed", json!({}))
                .await?;
        }
        status(ctx.conn(), &user).await
    }

    async fn two_factor_regenerate_recovery_codes(
        &self,
        ctx: &mut CommandCtx,
        _args: Empty,
    ) -> Result<RecoveryCodes, ApiError> {
        let user = signed_in(ctx.caller())?;
        ctx.require_fresh_login()?;
        if !two_factor_state(ctx.conn(), &user).await?.enrolled {
            return Err(ApiError::with_message(
                ErrorCode::InvalidArguments,
                "Two-factor authentication is not set up.",
            ));
        }
        let codes = replace_recovery_codes(ctx.conn(), &user).await?;
        ctx.record(
            &Topic::user(&user),
            "two_factor.recovery_codes_replaced",
            json!({}),
        )
        .await?;
        Ok(RecoveryCodes {
            secret: Some(codes),
            secret_withheld: false,
        })
    }

    async fn two_factor_status(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<TwoFactorStatus>, ApiError> {
        let user = signed_in(ctx.caller())?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: status(ctx.conn(), &user).await?,
        })
    }
}

/// Replacing a confirmed factor needs a fresh login (decision 0007). A first enrollment does
/// not, so an enrollment-only session can finish it.
async fn require_fresh_login_to_replace(
    ctx: &mut CommandCtx,
    user: &UserId,
) -> Result<(), ApiError> {
    if two_factor_state(ctx.conn(), user).await?.enrolled {
        ctx.require_fresh_login()?;
    }
    Ok(())
}

/// Checks a second-factor proof and spends it: a time step is accepted once, a recovery code
/// once. `false` for a wrong, used or unreadable proof. Without `secrets.key` time-based codes
/// fail and recovery codes keep working (decision 0012).
pub(super) async fn consume(
    data_dir: &Path,
    conn: &mut SqliteConnection,
    user: &UserId,
    proof: &SecondFactorProof,
    now: i64,
) -> Result<bool, ApiError> {
    match proof {
        SecondFactorProof::Totp { code } => {
            let row: Option<(Vec<u8>, Option<i64>)> = sqlx::query_as(
                "SELECT secret, last_step FROM two_factor WHERE user_id = ? AND secret IS NOT NULL",
            )
            .bind(user.as_str())
            .fetch_optional(&mut *conn)
            .await?;
            let Some((sealed, last_step)) = row else {
                return Ok(false);
            };
            let step = secret_key(data_dir)?
                .open(user, &sealed)
                .and_then(|secret| totp::verify(&secret, code, now, last_step));
            let Some(step) = step else {
                return Ok(false);
            };
            sqlx::query("UPDATE two_factor SET last_step = ? WHERE user_id = ?")
                .bind(step)
                .bind(user.as_str())
                .execute(conn)
                .await?;
            Ok(true)
        }
        SecondFactorProof::RecoveryCode { code } => {
            let spent = sqlx::query(
                "UPDATE recovery_codes SET used_at = ?
                 WHERE user_id = ? AND code_hash = ? AND used_at IS NULL
                   AND EXISTS (SELECT 1 FROM two_factor WHERE user_id = ? AND secret IS NOT NULL)",
            )
            .bind(now)
            .bind(user.as_str())
            .bind(totp::recovery_code_digest(code))
            .bind(user.as_str())
            .execute(conn)
            .await?;
            Ok(spent.rows_affected() == 1)
        }
    }
}

/// Removes the factor, a pending enrollment and every recovery code. `true` when a confirmed
/// factor existed.
pub(super) async fn remove(conn: &mut SqliteConnection, user: &UserId) -> Result<bool, ApiError> {
    let removed: Option<bool> =
        sqlx::query_scalar("DELETE FROM two_factor WHERE user_id = ? RETURNING secret IS NOT NULL")
            .bind(user.as_str())
            .fetch_optional(&mut *conn)
            .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
        .bind(user.as_str())
        .execute(conn)
        .await?;
    Ok(removed == Some(true))
}

async fn replace_recovery_codes(
    conn: &mut SqliteConnection,
    user: &UserId,
) -> Result<Vec<String>, ApiError> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
        .bind(user.as_str())
        .execute(&mut *conn)
        .await?;
    let mut shown = Vec::new();
    for (code, digest) in totp::new_recovery_codes() {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?, ?)")
            .bind(user.as_str())
            .bind(digest)
            .execute(&mut *conn)
            .await?;
        shown.push(code);
    }
    Ok(shown)
}

async fn status(conn: &mut SqliteConnection, user: &UserId) -> Result<TwoFactorStatus, ApiError> {
    let required = two_factor_state(&mut *conn, user).await?.required;
    let (enrolled_at, recovery_codes_left): (Option<i64>, i64) = sqlx::query_as(
        "SELECT (SELECT confirmed_at FROM two_factor WHERE user_id = ?1),
                (SELECT COUNT(*) FROM recovery_codes WHERE user_id = ?1 AND used_at IS NULL)",
    )
    .bind(user.as_str())
    .fetch_one(conn)
    .await?;
    Ok(TwoFactorStatus {
        enrolled: enrolled_at.is_some(),
        enrolled_at,
        required,
        recovery_codes_left: recovery_codes_left as u32,
    })
}

fn secret_key(data_dir: &Path) -> Result<SecretKey, ApiError> {
    SecretKey::load_or_create(data_dir).map_err(ApiError::internal)
}

fn encode(value: &str) -> String {
    byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
