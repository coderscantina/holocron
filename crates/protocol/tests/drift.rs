//! The committed TypeScript contract and OpenAPI document match the registry.

use holocron_protocol::export;

#[test]
fn committed_contract_has_no_drift() {
    let output = export::generate().expect("export runs");
    let problems = export::drift(
        &output,
        &export::default_web_dir(),
        &export::default_openapi_path(),
    )
    .expect("committed files are readable");
    assert!(
        problems.is_empty(),
        "run `cargo run -p holocron-protocol --bin export-types`:\n{}",
        problems.join("\n")
    );
}
