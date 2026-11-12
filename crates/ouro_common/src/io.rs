// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use tracing::instrument;

/// Newtype wrapper around raw CBOR-encoded bytes.
///
/// Enables ergonomic conversion between binary CBOR payloads and strongly-typed values
/// via [`into_type`](CborBytes::into_type) and [`from_value`](CborBytes::from_value).
#[derive(Debug, Clone)]
pub struct CborBytes(pub Vec<u8>);

impl CborBytes {
    /// Convert CBOR bytes to a specific type
    ///
    /// # Type Parameters
    /// * `T` - Target type that implements FromCbor
    ///
    /// # Errors
    /// Returns an error if deserialization fails
    pub fn into_type<T: FromCbor>(self) -> Result<T, ciborium::de::Error<std::io::Error>> {
        T::from_cbor(&self.0)
    }

    /// Create CborBytes from a serializable value
    ///
    /// # Type Parameters
    /// * `T` - Source type that implements Serialize
    ///
    /// # Errors
    /// Returns an error if serialization fails
    pub fn from_value<T: ToCbor>(value: &T) -> Result<Self, ciborium::ser::Error<std::io::Error>> {
        let cbor_bytes = value.to_cbor()?;
        Ok(CborBytes(cbor_bytes))
    }
}

/// Trait for types that can be deserialized from CBOR format
pub trait FromCbor
where
    Self: Sized,
    for<'de> Self: Deserialize<'de>,
{
    /// Deserialize from CBOR bytes
    ///
    /// # Arguments
    /// * `cbor` - CBOR-encoded bytes
    ///
    /// # Errors
    /// Returns an error if deserialization fails
    fn from_cbor(cbor: &[u8]) -> Result<Self, ciborium::de::Error<std::io::Error>> {
        ciborium::from_reader(cbor)
    }
}

/// Trait for types that can be serialized to CBOR format
pub trait ToCbor
where
    Self: Serialize + Sized,
{
    /// Serialize to CBOR bytes
    ///
    /// # Errors
    /// Returns an error if serialization fails
    fn to_cbor(&self) -> Result<Vec<u8>, ciborium::ser::Error<std::io::Error>> {
        let mut buffer = Vec::new();
        match ciborium::into_writer(self, &mut buffer) {
            Ok(_) => Ok(buffer),
            Err(e) => Err(e),
        }
    }
}

/// Expands `${VAR}` and `${VAR:-default}` placeholders in a string with
/// environment variable values.
///
/// - `${VAR}` is replaced with the value of `VAR`, or left unchanged if unset.
/// - `${VAR:-default}` is replaced with the value of `VAR`, or `default` if unset.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut token = String::new();
            let mut found_close = false;
            for ch in chars.by_ref() {
                if ch == '}' {
                    found_close = true;
                    break;
                }
                token.push(ch);
            }
            if found_close {
                let (var_name, default_val) = if let Some(pos) = token.find(":-") {
                    (&token[..pos], Some(&token[pos + 2..]))
                } else {
                    (token.as_str(), None)
                };
                match std::env::var(var_name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        if let Some(def) = default_val {
                            result.push_str(def);
                        } else {
                            // Preserve the original placeholder when the var is unset
                            result.push_str(&format!("${{{}}}", token));
                        }
                    }
                }
            } else {
                // Unclosed brace — preserve literally
                result.push('$');
                result.push('{');
                result.push_str(&token);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Trait for types that can be deserialized from a TOML file on disk.
///
/// Before parsing, `${VAR}` and `${VAR:-default}` placeholders in the file
/// content are expanded with environment variable values.
pub trait FromTomlFile {
    #[instrument]
    fn from_toml_file(path: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
        for<'de> Self: Deserialize<'de>,
    {
        let raw_toml = std::fs::read_to_string(path)?;
        let config_toml = expand_env_vars(&raw_toml);
        let config: Self = match toml::from_str(&config_toml) {
            Ok(c) => c,
            Err(e) => {
                let span = e.span();
                let line_col = match span {
                    Some(span) => {
                        let prefix = config_toml.get(..span.start).unwrap_or(&config_toml);
                        let line_no = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                        let col_no = prefix
                            .rsplit('\n')
                            .next()
                            .map(|line| line.chars().count() + 1)
                            .unwrap_or(1);
                        format!("{}:{}", line_no, col_no)
                    }
                    None => "unknown".to_string(),
                };

                return Err(anyhow::anyhow!(
                    "Failed to parse: {}:{}: {}",
                    path,
                    line_col,
                    e.message()
                ));
            }
        };
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars_with_set_var() {
        std::env::set_var("_TEST_IO_VAR", "hello");
        assert_eq!(expand_env_vars("${_TEST_IO_VAR}"), "hello");
        std::env::remove_var("_TEST_IO_VAR");
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        std::env::remove_var("_TEST_IO_UNSET");
        assert_eq!(expand_env_vars("${_TEST_IO_UNSET:-fallback}"), "fallback");
    }

    #[test]
    fn test_expand_env_vars_set_ignores_default() {
        std::env::set_var("_TEST_IO_SET", "actual");
        assert_eq!(expand_env_vars("${_TEST_IO_SET:-fallback}"), "actual");
        std::env::remove_var("_TEST_IO_SET");
    }

    #[test]
    fn test_expand_env_vars_unset_preserves_placeholder() {
        std::env::remove_var("_TEST_IO_MISSING");
        assert_eq!(
            expand_env_vars("${_TEST_IO_MISSING}"),
            "${_TEST_IO_MISSING}"
        );
    }

    #[test]
    fn test_expand_env_vars_no_placeholders() {
        assert_eq!(expand_env_vars("plain text"), "plain text");
    }

    #[test]
    fn test_expand_env_vars_in_url() {
        std::env::set_var("_TEST_DB_URL", "postgres://u:p@host/db");
        let input = "url = \"${_TEST_DB_URL:-postgres://default@localhost/db}\"";
        let result = expand_env_vars(input);
        assert_eq!(result, "url = \"postgres://u:p@host/db\"");
        std::env::remove_var("_TEST_DB_URL");
    }

    #[test]
    fn test_expand_env_vars_unclosed_brace() {
        assert_eq!(expand_env_vars("${UNCLOSED"), "${UNCLOSED");
    }
}
