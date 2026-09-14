use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    let (path, frames) = match args.as_slice() {
        [path] => (path, 360),
        [path, frames] => match frames.parse::<usize>() {
            Ok(frames) => (path, frames),
            Err(_) => {
                eprintln!("frames must be an integer");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("Usage: io-worker-probe scene.json [frames]");
            return ExitCode::from(2);
        }
    };
    match io::probe_worker(Path::new(path), frames) {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Worker probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}
