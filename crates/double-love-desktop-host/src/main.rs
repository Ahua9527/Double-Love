use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use double_love_desktop_host::HostRuntimeConfig;

const SIDECAR_MOCK_ENVIRONMENT: [&str; 2] = ["DOUBLELOVE_ASR_MOCK", "DOUBLELOVE_SPEAKER_MOCK"];

#[derive(Debug, Default, PartialEq, Eq)]
struct HostArguments {
    app_data_dir: Option<PathBuf>,
    resource_dir: Option<PathBuf>,
    test_transcribe_mock: bool,
    test_speaker_mock: bool,
}

fn host_arguments(mut arguments: impl Iterator<Item = OsString>) -> Result<HostArguments, String> {
    let mut parsed = HostArguments::default();
    while let Some(flag) = arguments.next() {
        if flag == "--app-data-dir" {
            if parsed.app_data_dir.is_some() {
                return Err("--app-data-dir may only be supplied once".to_string());
            }
            parsed.app_data_dir =
                Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    "--app-data-dir requires a path".to_string()
                })?));
        } else if flag == "--resource-dir" {
            if parsed.resource_dir.is_some() {
                return Err("--resource-dir may only be supplied once".to_string());
            }
            parsed.resource_dir =
                Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    "--resource-dir requires a path".to_string()
                })?));
        } else if flag == "--test-transcribe-mock" {
            if !cfg!(debug_assertions) {
                return Err("--test-transcribe-mock is unavailable in release builds".to_string());
            }
            parsed.test_transcribe_mock = true;
        } else if flag == "--test-speaker-mock" {
            if !cfg!(debug_assertions) {
                return Err("--test-speaker-mock is unavailable in release builds".to_string());
            }
            parsed.test_speaker_mock = true;
        } else {
            return Err(format!(
                "unknown desktop host argument: {}",
                flag.to_string_lossy()
            ));
        }
    }
    Ok(parsed)
}

fn sanitize_mock_environment(test_transcribe_mock: bool) {
    if test_transcribe_mock {
        return;
    }
    for variable in SIDECAR_MOCK_ENVIRONMENT {
        // SAFETY: main calls this during single-threaded startup, before the service or any
        // sidecar worker threads exist. Test mode is configured explicitly after sanitization.
        unsafe { std::env::remove_var(variable) };
    }
}

fn main() -> ExitCode {
    let arguments = match host_arguments(std::env::args_os().skip(1)) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("double-love desktop host failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    sanitize_mock_environment(arguments.test_transcribe_mock);
    let stdin = io::stdin();
    let stdout = io::stdout();

    match double_love_desktop_host::run_host_with_config(
        &mut stdin.lock(),
        stdout,
        arguments.app_data_dir,
        HostRuntimeConfig {
            resource_dir: arguments.resource_dir,
            test_transcribe_mock: arguments.test_transcribe_mock,
            test_speaker_mock: arguments.test_speaker_mock,
        },
    ) {
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
    fn parses_explicit_runtime_directories() {
        let arguments = host_arguments(
            [
                OsString::from("--app-data-dir"),
                OsString::from("/tmp/dl"),
                OsString::from("--resource-dir"),
                OsString::from("/tmp/resources"),
            ]
            .into_iter(),
        )
        .expect("parse host arguments");
        assert_eq!(
            arguments,
            HostArguments {
                app_data_dir: Some(PathBuf::from("/tmp/dl")),
                resource_dir: Some(PathBuf::from("/tmp/resources")),
                test_transcribe_mock: false,
                test_speaker_mock: false,
            }
        );
    }

    #[test]
    fn debug_host_accepts_explicit_test_mock_modes() {
        let arguments = host_arguments(
            [
                OsString::from("--test-transcribe-mock"),
                OsString::from("--test-speaker-mock"),
            ]
            .into_iter(),
        )
        .expect("parse test modes");
        assert!(arguments.test_transcribe_mock);
        assert!(arguments.test_speaker_mock);
    }

    #[test]
    fn absent_app_data_directory_is_left_for_service_validation() {
        assert_eq!(
            host_arguments(std::iter::empty()).expect("parse empty host arguments"),
            HostArguments::default()
        );
    }
}
