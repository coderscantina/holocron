//! Sets `HOLOCRON_VERSION`, the version `holocron --version` prints: the crate version, plus the
//! release commit when the build passes `HOLOCRON_GIT_SHA` (the Docker image does), e.g.
//! `26.10.19 (1a2b3c4)`.

fn main() {
    println!("cargo:rerun-if-env-changed=HOLOCRON_GIT_SHA");
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    let full = match std::env::var("HOLOCRON_GIT_SHA") {
        Ok(sha) if !sha.is_empty() => format!("{version} ({sha})"),
        _ => version,
    };
    println!("cargo:rustc-env=HOLOCRON_VERSION={full}");
}
