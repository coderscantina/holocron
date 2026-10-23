//! Writes the generated TypeScript contract and OpenAPI document, or checks them for drift.
//!
//! `cargo run -p holocron-protocol --bin export-types` regenerates.
//! `cargo run -p holocron-protocol --bin export-types -- --check` exits 1 on any difference.

use std::process::ExitCode;

use holocron_protocol::export;

fn main() -> ExitCode {
    let check = std::env::args().skip(1).any(|arg| arg == "--check");
    let web_dir = export::default_web_dir();
    let openapi_path = export::default_openapi_path();

    let output = match export::generate() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("export failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    if check {
        return match export::drift(&output, &web_dir, &openapi_path) {
            Ok(problems) if problems.is_empty() => {
                println!(
                    "generated contract is up to date ({} files)",
                    output.web.len() + 1
                );
                ExitCode::SUCCESS
            }
            Ok(problems) => {
                eprintln!("generated contract drifted; run export-types and commit the result:");
                for problem in problems {
                    eprintln!("  {problem}");
                }
                ExitCode::FAILURE
            }
            Err(error) => {
                eprintln!("drift check failed: {error}");
                ExitCode::FAILURE
            }
        };
    }

    match export::write(&output, &web_dir, &openapi_path) {
        Ok(()) => {
            println!("wrote {} files to {}", output.web.len(), web_dir.display());
            println!("wrote {}", openapi_path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write failed: {error}");
            ExitCode::FAILURE
        }
    }
}
