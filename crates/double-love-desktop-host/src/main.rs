use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

fn app_data_dir_argument(
    mut arguments: impl Iterator<Item = OsString>,
) -> Result<Option<PathBuf>, String> {
    let Some(flag) = arguments.next() else {
        return Ok(None);
    };
    if flag != "--app-data-dir" {
        return Err(format!(
            "unknown desktop host argument: {}",
            flag.to_string_lossy()
        ));
    }
    let directory = arguments
        .next()
        .ok_or_else(|| "--app-data-dir requires a path".to_string())?;
    if arguments.next().is_some() {
        return Err("desktop host accepts only --app-data-dir <path>".to_string());
    }
    Ok(Some(PathBuf::from(directory)))
}

fn main() -> ExitCode {
    let app_data_dir = match app_data_dir_argument(std::env::args_os().skip(1)) {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("double-love desktop host failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let stdin = io::stdin();
    let stdout = io::stdout();

    match double_love_desktop_host::run_host(&mut stdin.lock(), stdout, app_data_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("double-love desktop host failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_app_data_directory() {
        let directory = app_data_dir_argument(
            [OsString::from("--app-data-dir"), OsString::from("/tmp/dl")].into_iter(),
        )
        .expect("parse host arguments");
        assert_eq!(directory, Some(PathBuf::from("/tmp/dl")));
    }

    #[test]
    fn absent_app_data_directory_is_left_for_service_validation() {
        assert_eq!(
            app_data_dir_argument(std::iter::empty()).expect("parse empty host arguments"),
            None
        );
    }
}
