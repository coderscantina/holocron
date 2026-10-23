//! `account.*` operations: the caller's own profile, password and preferences.

use chrono_tz::Tz;
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::exploration::Range;
use holocron_protocol::ids::UserId;
use holocron_protocol::operations::account::{
    Accent, AccountHandlers, AccountResult, AccountUpdateArgs, ChangePasswordArgs, Locale,
    Preferences, Theme,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::{
    Caller, account, end_sessions, hash_password_blocking, signed_in, validate_password,
    validate_profile,
};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

impl AccountHandlers for AppState {
    async fn account_update(
        &self,
        ctx: &mut CommandCtx,
        args: AccountUpdateArgs,
    ) -> Result<AccountResult, ApiError> {
        let user = signed_in(ctx.caller())?;
        let (name, email) =
            validate_profile(ctx.conn(), &args.name, &args.email, Some(&user)).await?;
        sqlx::query("UPDATE users SET name = ?, email = ? WHERE id = ?")
            .bind(&name)
            .bind(&email)
            .bind(user.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(&Topic::user(&user), "account.updated", json!({}))
            .await?;
        account_result(ctx.conn(), &user).await
    }

    async fn account_change_password(
        &self,
        ctx: &mut CommandCtx,
        args: ChangePasswordArgs,
    ) -> Result<Empty, ApiError> {
        let Caller::Person { user, session, .. } = ctx.caller().clone() else {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        };
        ctx.require_fresh_login()?;
        validate_password("new_password", &args.new_password)?;
        let hash = hash_password_blocking(args.new_password).await?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash)
            .bind(user.as_str())
            .execute(ctx.conn())
            .await?;
        end_sessions(ctx.conn(), &user, Some(&session)).await?;
        ctx.record(&Topic::user(&user), "account.password_changed", json!({}))
            .await?;
        Ok(Empty {})
    }

    async fn account_update_preferences(
        &self,
        ctx: &mut CommandCtx,
        args: Preferences,
    ) -> Result<Preferences, ApiError> {
        let user = signed_in(ctx.caller())?;
        if let Some(timezone) = &args.timezone
            && timezone.parse::<Tz>().is_err()
        {
            return Err(FieldErrors::one(
                "timezone",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let default_range = args
            .default_range
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(ApiError::internal)?;
        sqlx::query(
            "INSERT INTO user_preferences (user_id, timezone, default_range, locale, theme, accent, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (user_id) DO UPDATE SET timezone = excluded.timezone,
                 default_range = excluded.default_range, locale = excluded.locale,
                 theme = excluded.theme, accent = excluded.accent, updated_at = excluded.updated_at",
        )
        .bind(user.as_str())
        .bind(&args.timezone)
        .bind(default_range)
        .bind(args.locale.map(locale_str))
        .bind(args.theme.map(theme_str))
        .bind(args.accent.map(accent_str))
        .bind(ctx.now())
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::user(&user),
            "account.preferences_updated",
            json!({}),
        )
        .await?;
        Ok(args)
    }

    async fn account_get(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<AccountResult>, ApiError> {
        let user = signed_in(ctx.caller())?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: account_result(ctx.conn(), &user).await?,
        })
    }

    async fn account_preferences(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<Preferences>, ApiError> {
        let user = signed_in(ctx.caller())?;
        let row: Option<PreferencesRow> = sqlx::query_as(
            "SELECT timezone, default_range, locale, theme, accent FROM user_preferences WHERE user_id = ?",
        )
        .bind(user.as_str())
        .fetch_optional(ctx.conn())
        .await?;
        let preferences = match row {
            None => Preferences::default(),
            Some(row) => Preferences {
                timezone: row.timezone,
                default_range: row
                    .default_range
                    .map(|range| serde_json::from_str::<Range>(&range))
                    .transpose()
                    .map_err(ApiError::internal)?,
                locale: row.locale.as_deref().and_then(parse_locale),
                theme: row.theme.as_deref().and_then(parse_theme),
                accent: row.accent.as_deref().and_then(parse_accent),
            },
        };
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: preferences,
        })
    }
}

#[derive(sqlx::FromRow)]
struct PreferencesRow {
    timezone: Option<String>,
    /// JSON text of a `Range`.
    default_range: Option<String>,
    locale: Option<String>,
    theme: Option<String>,
    accent: Option<String>,
}

async fn account_result(
    conn: &mut SqliteConnection,
    user: &UserId,
) -> Result<AccountResult, ApiError> {
    let created_at: i64 = sqlx::query_scalar("SELECT created_at FROM users WHERE id = ?")
        .bind(user.as_str())
        .fetch_one(&mut *conn)
        .await?;
    let account = account(conn, user)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
    Ok(AccountResult {
        account,
        created_at,
    })
}

fn locale_str(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "en",
        Locale::De => "de",
    }
}

fn parse_locale(value: &str) -> Option<Locale> {
    [Locale::En, Locale::De]
        .into_iter()
        .find(|locale| locale_str(*locale) == value)
}

fn theme_str(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

fn parse_theme(value: &str) -> Option<Theme> {
    [Theme::System, Theme::Light, Theme::Dark]
        .into_iter()
        .find(|theme| theme_str(*theme) == value)
}

fn accent_str(accent: Accent) -> &'static str {
    match accent {
        Accent::Indigo => "indigo",
        Accent::Blue => "blue",
        Accent::Violet => "violet",
        Accent::Rose => "rose",
        Accent::Orange => "orange",
        Accent::Amber => "amber",
        Accent::Emerald => "emerald",
        Accent::Cyan => "cyan",
    }
}

fn parse_accent(value: &str) -> Option<Accent> {
    [
        Accent::Indigo,
        Accent::Blue,
        Accent::Violet,
        Accent::Rose,
        Accent::Orange,
        Accent::Amber,
        Accent::Emerald,
        Accent::Cyan,
    ]
    .into_iter()
    .find(|accent| accent_str(*accent) == value)
}
