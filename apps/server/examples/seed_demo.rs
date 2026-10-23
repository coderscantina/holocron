//! Adds demo workspaces and sites to an initialized data directory, for browser checks.
//! Development only; the packaged binary has no equivalent.
//!
//! `cargo run -p holocron-server --example seed_demo -- <data-dir> <admin-email> <viewer-password>`
//!
//! The admin becomes owner of "Roadhouse" and of a personal workspace. A viewer account
//! `viewer@example.test` joins "Roadhouse" with the given password.

use std::path::PathBuf;

use holocron_protocol::access::{InstallRole, Role, WorkspaceKind};
use holocron_protocol::ids::UserId;
use holocron_server::db::Db;
use holocron_server::{identity, sites};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(email), Some(viewer_password)) = (args.next(), args.next(), args.next())
    else {
        anyhow::bail!("usage: seed_demo <data-dir> <admin-email> <viewer-password>");
    };
    let db = Db::open(&PathBuf::from(dir)).await?;
    let mut tx = db.writer.begin_with("BEGIN IMMEDIATE").await?;

    let admin: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(identity::normalize_email(&email))
        .fetch_one(&mut *tx)
        .await?;
    let admin = UserId::new(admin);
    sqlx::query("UPDATE users SET name = 'Mike Wallner' WHERE id = ?")
        .bind(admin.as_str())
        .execute(&mut *tx)
        .await?;
    let viewer = identity::create_user(
        &mut tx,
        "viewer@example.test",
        "Vera Viewer",
        &viewer_password,
        InstallRole::User,
    )
    .await?;

    let roadhouse = sites::create_workspace(&mut tx, "Roadhouse", WorkspaceKind::Team).await?;
    sites::add_member(&mut tx, &roadhouse, &admin, Role::Owner).await?;
    sites::add_member(&mut tx, &roadhouse, &viewer, Role::Viewer).await?;
    for (name, domain) in [
        ("roadhouse.test", "roadhouse.test"),
        ("Roadhouse shop", "shop.roadhouse.test"),
        (
            "Roadhouse Konzertagentur und Veranstaltungstechnik Salzburg",
            "veranstaltungstechnik.roadhouse.test",
        ),
    ] {
        sites::create_site(&mut tx, &roadhouse, name, domain, "Europe/Vienna").await?;
    }

    let personal =
        sites::create_workspace(&mut tx, "Mike Wallner", WorkspaceKind::Personal).await?;
    sites::add_member(&mut tx, &personal, &admin, Role::Owner).await?;
    sites::create_site(
        &mut tx,
        &personal,
        "notes.test",
        "notes.test",
        "Europe/Vienna",
    )
    .await?;

    tx.commit().await?;
    db.close().await;
    println!("seeded Roadhouse (3 sites) and a personal workspace (1 site)");
    Ok(())
}
