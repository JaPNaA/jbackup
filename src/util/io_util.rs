use std::{ffi::OsStr, fmt::Display, process};

/// Converts the error type in a Result into a string.
pub fn simplify_result<T>(io_result: Result<T, impl Display>) -> Result<T, String> {
    match io_result {
        Ok(v) => Ok(v),
        Err(err) => Err(format!("IO Error: {err}")),
    }
}

pub fn run_command_handle_failures(
    command: &mut process::Command,
) -> Result<process::Output, String> {
    let output_result = command.output();
    let output = match output_result {
        Err(err) => {
            return Err(format!(
                "Failed to start command: {}: {}",
                format_command_debug(command),
                err
            ));
        }
        Ok(x) => x,
    };

    if output.status.success() {
        Ok(output)
    } else {
        let stdout_str = simplify_result(String::from_utf8(output.stdout))?;
        let stderr_str = simplify_result(String::from_utf8(output.stderr))?;
        eprintln!("Stdout from {:?}:\n{}", command.get_program(), stdout_str);
        eprintln!("Stderr from {:?}:\n{}", command.get_program(), stderr_str);
        Err(format!("Command failed: {}", format_command_debug(command)))
    }
}

pub fn format_command_debug(command: &process::Command) -> String {
    format!(
        "{:?}, arguments: {:?}",
        command.get_program(),
        command.get_args().collect::<Vec<&OsStr>>()
    )
}
