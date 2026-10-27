//! `holocron query` against a served instance: the answer on stdout, a refusal on stderr.

mod common;

use std::process::{Command, Output};

use common::Harness;

/// Serves the harness on a local port and runs the real binary against it.
async fn holocron(harness: &Harness, key: Option<&str>, operation: &str) -> Output {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = holocron_server::router(harness.state.clone());
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    });

    let input = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(input.path(), "{}").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_holocron"));
    command
        .args(["query", "--url", &url, "--operation", operation])
        .arg(input.path())
        .env_remove("HOLOCRON_API_KEY");
    if let Some(key) = key {
        command.env("HOLOCRON_API_KEY", key);
    }
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn query_prints_the_answer() {
    let harness = Harness::new().await;
    let key = harness.api_key("p_member", &["sites.list"], None).await;
    let key = key.strip_prefix("Bearer ").unwrap();

    let output = holocron(&harness, Some(key), "sites.list").await;
    assert!(output.status.success(), "{output:?}");
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(answer.is_object(), "{answer}");
}

#[tokio::test(flavor = "multi_thread")]
async fn query_names_the_refusal_and_exits_non_zero() {
    let harness = Harness::new().await;
    let key = harness.api_key("p_member", &["sites.get"], None).await;
    let key = key.strip_prefix("Bearer ").unwrap();

    let output = holocron(&harness, Some(key), "sites.list").await;
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error: operation_not_granted"),
        "{stderr}"
    );

    let missing = holocron(&harness, None, "sites.list").await;
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("HOLOCRON_API_KEY"));
}
