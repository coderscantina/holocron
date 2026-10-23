//! `auth.*` operations.

use std::time::Duration;

use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::ids::UserId;
use holocron_protocol::limits::{LOGIN_CHALLENGE_MS, REAUTHENTICATION_WINDOW_MS};
use holocron_protocol::operations::auth::{
    AuthHandlers, LoginArgs, LoginResult, ReauthenticateArgs, Reauthenticated, SecondFactorProof,
    SessionState, SessionsRevoked,
};
use holocron_protocol::registry::Snapshot;

use super::{
    Caller, account, credentials, end_sessions, hash_password_blocking, is_imported_hash,
    new_session_token, password_hash, two_factor, two_factor_state, verify_password,
};
use crate::context::{CommandCtx, QueryCtx};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::state::AppState;

impl AuthHandlers for AppState {
    async fn auth_login(
        &self,
        ctx: &mut CommandCtx,
        args: LoginArgs,
    ) -> Result<LoginResult, ApiError> {
        self.login_limiter.check(ctx.caller().address())?;

        let (account, hash) = credentials(ctx.conn(), &args.email).await?;
        let imported = is_imported_hash(&hash);
        let verified = verify_password(hash, args.password.clone()).await;
        let Some(account) = account.filter(|_| verified) else {
            return Err(ApiError::new(ErrorCode::InvalidCredentials));
        };
        if imported {
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
                .bind(hash_password_blocking(args.password).await?)
                .bind(account.id.as_str())
                .execute(ctx.conn())
                .await?;
        }

        let state = two_factor_state(ctx.conn(), &account.id).await?;
        if state.enrolled {
            start_challenge(ctx, &account.id).await?;
            return Ok(LoginResult::SecondFactorRequired);
        }
        start_session(ctx, &account.id).await?;
        Ok(if state.required {
            LoginResult::EnrollmentRequired { account }
        } else {
            LoginResult::SignedIn { account }
        })
    }

    async fn auth_verify_second_factor(
        &self,
        ctx: &mut CommandCtx,
        args: SecondFactorProof,
    ) -> Result<LoginResult, ApiError> {
        self.login_limiter.check(ctx.caller().address())?;
        let Caller::Anonymous {
            challenge: Some(challenge),
            ..
        } = ctx.caller().clone()
        else {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        };
        let now = ctx.now();
        let row: Option<(String, u32)> = sqlx::query_as(
            "SELECT user_id, attempts FROM login_challenges WHERE id = ? AND expires_at > ?",
        )
        .bind(&challenge)
        .bind(now)
        .fetch_optional(ctx.conn())
        .await?;
        let lifetime = Duration::from_millis(LOGIN_CHALLENGE_MS as u64);
        let user = row
            .filter(|(_, stored)| self.challenge_attempts.take(&challenge, *stored, lifetime))
            .map(|(user, _)| UserId::new(user))
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;

        if !two_factor::consume(&self.config.data_dir, ctx.conn(), &user, &args, now).await? {
            // The command rolls back, so the durable count is written after it, on the same
            // writer. The in-memory count above already holds the limit.
            let writer = self.db.writer.clone();
            tokio::spawn(async move {
                let counted =
                    sqlx::query("UPDATE login_challenges SET attempts = attempts + 1 WHERE id = ?")
                        .bind(challenge)
                        .execute(&writer)
                        .await;
                if let Err(error) = counted {
                    tracing::warn!(%error, "could not count a wrong second factor");
                }
            });
            return Err(ApiError::new(ErrorCode::SecondFactorInvalid));
        }

        sqlx::query("DELETE FROM login_challenges WHERE id = ?")
            .bind(&challenge)
            .execute(ctx.conn())
            .await?;
        self.challenge_attempts.forget(&challenge);
        start_session(ctx, &user).await?;
        let account = account(ctx.conn(), &user)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        Ok(LoginResult::SignedIn { account })
    }

    async fn auth_logout(&self, ctx: &mut CommandCtx, _args: Empty) -> Result<Empty, ApiError> {
        if let Caller::Person { session, .. } = ctx.caller().clone() {
            sqlx::query("DELETE FROM sessions WHERE id = ?")
                .bind(session)
                .execute(ctx.conn())
                .await?;
        }
        ctx.end_session();
        Ok(Empty {})
    }

    async fn auth_reauthenticate(
        &self,
        ctx: &mut CommandCtx,
        args: ReauthenticateArgs,
    ) -> Result<Reauthenticated, ApiError> {
        self.login_limiter.check(ctx.caller().address())?;
        let Caller::Person { user, session, .. } = ctx.caller().clone() else {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        };
        let hash = password_hash(ctx.conn(), &user).await?;
        if !verify_password(hash, args.password).await {
            return Err(ApiError::new(ErrorCode::InvalidCredentials));
        }
        let now = ctx.now();
        if two_factor_state(ctx.conn(), &user).await?.enrolled {
            let proof = args
                .second_factor
                .ok_or_else(|| ApiError::new(ErrorCode::SecondFactorRequired))?;
            if !two_factor::consume(&self.config.data_dir, ctx.conn(), &user, &proof, now).await? {
                return Err(ApiError::new(ErrorCode::SecondFactorInvalid));
            }
        }
        sqlx::query("UPDATE sessions SET reauthenticated_at = ? WHERE id = ?")
            .bind(now)
            .bind(session)
            .execute(ctx.conn())
            .await?;
        Ok(Reauthenticated {
            reauthenticated_until: now + REAUTHENTICATION_WINDOW_MS,
        })
    }

    async fn auth_revoke_sessions(
        &self,
        ctx: &mut CommandCtx,
        _args: Empty,
    ) -> Result<SessionsRevoked, ApiError> {
        let Caller::Person { user, session, .. } = ctx.caller().clone() else {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        };
        let revoked = end_sessions(ctx.conn(), &user, Some(&session)).await?;
        Ok(SessionsRevoked {
            revoked: revoked as i64,
        })
    }

    async fn auth_session(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<SessionState>, ApiError> {
        let state = match ctx.caller().clone() {
            Caller::Person {
                user,
                enrollment_only,
                reauthenticated_at,
                ..
            } => SessionState {
                account: account(ctx.conn(), &user).await?,
                enrollment_only,
                reauthenticated_until: reauthenticated_at
                    .map(|at| at + REAUTHENTICATION_WINDOW_MS)
                    .filter(|until| *until > now_ms()),
            },
            _ => SessionState {
                account: None,
                enrollment_only: false,
                reauthenticated_until: None,
            },
        };
        Ok(Snapshot {
            topic: Topic::installation(),
            data: state,
        })
    }
}

/// Creates a login session and sets its cookie after commit.
async fn start_session(ctx: &mut CommandCtx, user: &UserId) -> Result<(), ApiError> {
    let now = ctx.now();
    let token = super::insert_session(ctx.conn(), user, now).await?;
    ctx.start_session(token);
    Ok(())
}

/// Creates the step between a correct password and the second factor, and sets its cookie.
/// Expired challenges of every account go at the same time.
async fn start_challenge(ctx: &mut CommandCtx, user: &UserId) -> Result<(), ApiError> {
    let (token, challenge) = new_session_token();
    let now = ctx.now();
    sqlx::query("DELETE FROM login_challenges WHERE expires_at <= ?")
        .bind(now)
        .execute(ctx.conn())
        .await?;
    sqlx::query(
        "INSERT INTO login_challenges (id, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&challenge)
    .bind(user.as_str())
    .bind(now)
    .bind(now + LOGIN_CHALLENGE_MS)
    .execute(ctx.conn())
    .await?;
    ctx.start_challenge(token);
    Ok(())
}
