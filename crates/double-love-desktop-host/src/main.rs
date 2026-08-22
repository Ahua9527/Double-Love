use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();

    match double_love_desktop_host::run_host(&mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("double-love desktop host failed: {error}");
            ExitCode::FAILURE
        }
    }
}
