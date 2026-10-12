//! Recognises the app-server's configuration-load failures in an error message.
//!
//! A request can fail because the configuration the server builds for it is
//! invalid, for instance a thread recorded under a model provider the current
//! `config.toml` no longer defines. The server words every such failure
//! `failed to load configuration: <detail>`, where the detail may start with
//! the file, line and column. The desktop shows these as one actionable
//! message instead of the raw protocol error, as the ChatGPT App does.

use std::path::PathBuf;

use serde_json::Value;

const DEFAULT_DETAIL: &str = "the file contains invalid configuration";
const CONFIG_FILE: &str = "config.toml";

/// A failure to load the configuration, with the location when the server named one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigLoadFailure {
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub detail: String,
}

impl ConfigLoadFailure {
    /// `path:line:column` as far as the server gave it, else `config.toml`.
    pub fn location(&self) -> String {
        match (&self.file, self.line, self.column) {
            (None, ..) => CONFIG_FILE.to_owned(),
            (Some(file), Some(line), Some(column)) => {
                format!("{}:{line}:{column}", file.display())
            }
            (Some(file), ..) => file.display().to_string(),
        }
    }
}

/// The configuration failure an adapter error reports, if it reports one. The
/// message is either the server's own text or a wrapper that embeds the
/// JSON-RPC error object, whose `message` carries the server's text.
pub fn config_load_failure(error: &str) -> Option<ConfigLoadFailure> {
    let embedded = embedded_rpc_message(error);
    let message = embedded.as_deref().unwrap_or(error);
    parse_message(message)
}

fn embedded_rpc_message(error: &str) -> Option<String> {
    error.match_indices('{').find_map(|(start, _)| {
        let value = serde_json::Deserializer::from_str(&error[start..])
            .into_iter::<Value>()
            .next()?
            .ok()?;
        value.get("message")?.as_str().map(str::to_owned)
    })
}

fn parse_message(message: &str) -> Option<ConfigLoadFailure> {
    let rest = after_load_failure_marker(message)?;
    if rest.is_empty() {
        return Some(ConfigLoadFailure {
            file: None,
            line: None,
            column: None,
            detail: DEFAULT_DETAIL.to_owned(),
        });
    }
    Some(located_failure(rest).unwrap_or_else(|| ConfigLoadFailure {
        file: None,
        line: None,
        column: None,
        detail: rest.to_owned(),
    }))
}

/// The text after `failed to load configuration:` (or the older
/// `error loading config…:` wording), case-insensitively, or `None` when the
/// message is not a configuration failure. Empty when nothing follows.
fn after_load_failure_marker(message: &str) -> Option<&str> {
    const MARKERS: [&str; 2] = ["failed to load configuration", "error loading "];
    let lowered = message.to_ascii_lowercase();
    let (start, marker) = MARKERS
        .iter()
        .filter_map(|marker| lowered.find(marker).map(|start| (start, *marker)))
        .min_by_key(|(start, _)| *start)?;
    let mut rest = &message[start + marker.len()..];
    if marker.ends_with(' ') {
        // `error loading [default ]config[.toml][ after config error]`.
        let lowered_rest = rest.to_ascii_lowercase();
        let mut consumed = 0;
        if lowered_rest.starts_with("default ") {
            consumed += "default ".len();
        }
        if !lowered_rest[consumed..].starts_with("config") {
            return None;
        }
        consumed += "config".len();
        if lowered_rest[consumed..].starts_with(".toml") {
            consumed += ".toml".len();
        }
        if lowered_rest[consumed..].starts_with(" after config error") {
            consumed += " after config error".len();
        }
        rest = &rest[consumed..];
    }
    match rest.strip_prefix(':') {
        Some(detail) => Some(detail.trim_start()),
        None if rest.is_empty() => Some(rest),
        None => None,
    }
}

/// `…config.toml[:line:column]: detail`, the server's way of naming the file.
fn located_failure(rest: &str) -> Option<ConfigLoadFailure> {
    if rest.contains('\n') {
        return None;
    }
    rest.rmatch_indices(CONFIG_FILE).find_map(|(start, _)| {
        // A file name, not a word of the sentence that merely mentions it.
        if start != 0 && !matches!(rest.as_bytes()[start - 1], b'/' | b'\\') {
            return None;
        }
        let end = start + CONFIG_FILE.len();
        let mut tail = &rest[end..];
        let (mut line, mut column) = (None, None);
        if let Some((found_line, found_column, remainder)) = line_and_column(tail) {
            line = Some(found_line);
            column = Some(found_column);
            tail = remainder;
        }
        let detail = tail.strip_prefix(':')?.trim_start();
        (!detail.is_empty()).then(|| ConfigLoadFailure {
            file: Some(PathBuf::from(&rest[..end])),
            line,
            column,
            detail: detail.to_owned(),
        })
    })
}

fn line_and_column(tail: &str) -> Option<(u32, u32, &str)> {
    let tail = tail.strip_prefix(':')?;
    let (line, tail) = number_prefix(tail)?;
    let (column, tail) = number_prefix(tail.strip_prefix(':')?)?;
    Some((line, column, tail))
}

fn number_prefix(source: &str) -> Option<(u32, &str)> {
    let digits = source.bytes().take_while(u8::is_ascii_digit).count();
    let number = source[..digits].parse().ok()?;
    Some((number, &source[digits..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_provider_is_recognised_inside_the_wrapped_rpc_error() {
        // The text the adapter produced for the thread that sat in the issue.
        let error = concat!(
            "thread/resume `01a10cff-a54d-76f2-b092-125e0dbe3823` 失败: ",
            "Codex JSON-RPC `thread/resume` 请求 248 失败：",
            r#"{"code":-32600,"message":"failed to load configuration: Model provider `aipixel` not found"}"#,
        );
        let failure = config_load_failure(error).expect("configuration failure");
        assert_eq!(failure.detail, "Model provider `aipixel` not found");
        assert_eq!(failure.file, None);
        assert_eq!(failure.location(), "config.toml");
    }

    #[test]
    fn the_server_text_alone_is_recognised() {
        let failure = config_load_failure("failed to load configuration: bad value")
            .expect("configuration failure");
        assert_eq!(failure.detail, "bad value");
    }

    #[test]
    fn the_file_line_and_column_are_taken_from_the_detail() {
        let failure = config_load_failure(
            "failed to load configuration: /Users/me/.codex/config.toml:12:5: invalid type: expected a string",
        )
        .expect("configuration failure");
        assert_eq!(
            failure.file,
            Some(PathBuf::from("/Users/me/.codex/config.toml"))
        );
        assert_eq!((failure.line, failure.column), (Some(12), Some(5)));
        assert_eq!(failure.detail, "invalid type: expected a string");
        assert_eq!(failure.location(), "/Users/me/.codex/config.toml:12:5");

        let without_position = config_load_failure(
            "failed to load configuration: /Users/me/.codex/config.toml: duplicate key",
        )
        .expect("configuration failure");
        assert_eq!(without_position.line, None);
        assert_eq!(without_position.detail, "duplicate key");
        assert_eq!(without_position.location(), "/Users/me/.codex/config.toml");
    }

    #[test]
    fn a_sentence_mentioning_the_file_does_not_name_a_path() {
        let failure = config_load_failure(
            "failed to load configuration: unknown field in config.toml: model_providr",
        )
        .expect("configuration failure");
        assert_eq!(failure.file, None);
        assert_eq!(
            failure.detail,
            "unknown field in config.toml: model_providr"
        );
    }

    #[test]
    fn an_empty_detail_falls_back_to_the_generic_reason() {
        let failure = config_load_failure("failed to load configuration").expect("failure");
        assert_eq!(failure.detail, DEFAULT_DETAIL);
        let failure = config_load_failure("failed to load configuration:").expect("failure");
        assert_eq!(failure.detail, DEFAULT_DETAIL);
    }

    #[test]
    fn the_older_config_loading_wording_is_recognised() {
        let failure =
            config_load_failure("Error: error loading default config after config error: oops")
                .expect("configuration failure");
        assert_eq!(failure.detail, "oops");
    }

    #[test]
    fn other_errors_are_not_configuration_failures() {
        assert_eq!(
            config_load_failure("thread/resume 失败: thread not found: abc"),
            None
        );
        assert_eq!(
            config_load_failure(r#"{"code":-32600,"message":"Invalid request"}"#),
            None
        );
        assert_eq!(
            config_load_failure("failed to load configurations are fun"),
            None
        );
    }
}
