use std::fmt::Display;

/// Converts the error type in a Result into a string.
pub fn simplify_result<T>(io_result: Result<T, impl Display>) -> Result<T, String> {
    match io_result {
        Ok(v) => Ok(v),
        Err(err) => Err(format!("IO Error: {err}")),
    }
}
