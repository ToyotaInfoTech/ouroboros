// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

/// Normalize Rust doc comment fragments into a printable description string.
/// Args:
///     lines (&[&str]): Doc comment fragments captured from `#[doc = ...]` attributes.
/// Returns:
///     String: Newline-joined text with per-line leading and trailing whitespace removed.
pub fn normalize_doc_comment_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build an input description map from static keys and generated descriptions.
/// Args:
///     entries ([(&str, String); N]): Input key and description pairs.
/// Returns:
///     HashMap<String, String>: Input descriptions keyed by argument name.
pub fn build_description_inputs<const N: usize>(
    entries: [(&str, String); N],
) -> HashMap<String, String> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[macro_export]
macro_rules! operation_descriptions {
    (
        $(
            $(#[doc = $operation_doc:literal])*
            $operation_name:literal {
                inputs {
                    $(
                        $(#[doc = $input_doc:literal])*
                        $input_name:literal
                    ),* $(,)?
                }
                output {
                    $(#[doc = $output_doc:literal])*
                }
            }
        ),* $(,)?
    ) => {
        vec![
            $(
                $crate::schema::cli::Description {
                    operation_name: $operation_name.to_string(),
                    description: $crate::runtime::cli::normalize_doc_comment_lines(&[$($operation_doc),*]),
                    inputs: $crate::runtime::cli::build_description_inputs([
                        $(
                            (
                                $input_name,
                                $crate::runtime::cli::normalize_doc_comment_lines(&[$($input_doc),*]),
                            )
                        ),*
                    ]),
                    output: $crate::runtime::cli::normalize_doc_comment_lines(&[$($output_doc),*]),
                }
            ),*
        ]
    };
}
