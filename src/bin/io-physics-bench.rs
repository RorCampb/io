use std::{env, path::Path, process::ExitCode};
fn main() -> ExitCode {
    let mut args: Vec<_> = env::args().skip(1).collect();
    let diagnostics = args
        .last()
        .is_some_and(|arg| arg == "--contact-diagnostics");
    if diagnostics {
        args.pop();
    }
    let (path, frames) = match args.as_slice() {
        [path] => (path, 300),
        [path, frames] => match frames.parse::<usize>() {
            Ok(n) => (path, n),
            Err(_) => {
                eprintln!("frames must be an integer");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("Usage: io-physics-bench scene.json [frames] [--contact-diagnostics]");
            return ExitCode::from(2);
        }
    };
    match io::benchmark_physics_with_diagnostics(Path::new(path), frames, diagnostics) {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Physics benchmark failed: {error}");
            ExitCode::FAILURE
        }
    }
}
