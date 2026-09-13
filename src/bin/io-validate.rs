use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [mode, path] if mode == "package" => io::validate_package(Path::new(path)),
        [mode, path] if mode == "scene" => io::validate_scene(Path::new(path)),
        _ => {
            eprintln!("Usage: io-validate <package|scene> path.json");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("integer report is serializable")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Validation failed: {error}");
            ExitCode::FAILURE
        }
    }
}
