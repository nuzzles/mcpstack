//! Load a stack from a local file or an explicitly supplied HTTP(S) URL.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::builder::{TypedValueParser, ValueParserFactory};

use crate::error::AppError;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub(crate) enum StackSource {
    File(PathBuf),
    Url(url::Url),
}

#[derive(Clone)]
pub(crate) struct StackSourceParser;

impl TypedValueParser for StackSourceParser {
    type Value = StackSource;

    fn parse_ref(
        &self,
        command: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let path = clap::builder::PathBufValueParser::new().parse_ref(command, arg, value)?;
        StackSource::parse(&path).map_err(|error| {
            let mut diagnostic =
                clap::Error::new(clap::error::ErrorKind::ValueValidation).with_cmd(command);
            // CLI diagnostics removes validator sources and redacts values. These
            // parse errors have static messages, so retain one as a safe hint.
            diagnostic.insert(
                clap::error::ContextKind::InvalidArg,
                clap::error::ContextValue::String(format!("SOURCE: {error}")),
            );
            diagnostic.insert(
                clap::error::ContextKind::InvalidValue,
                clap::error::ContextValue::String("<redacted>".into()),
            );
            diagnostic
        })
    }
}

impl ValueParserFactory for StackSource {
    type Parser = StackSourceParser;

    fn value_parser() -> Self::Parser {
        StackSourceParser
    }
}

impl StackSource {
    pub(crate) fn parse(input: &Path) -> Result<Self, AppError> {
        if let Some(text) = input.to_str()
            && let Some((scheme, _)) = text.split_once("://")
        {
            // A Windows drive followed by two separators is still a file path.
            if scheme.len() == 1 && scheme.as_bytes()[0].is_ascii_alphabetic() {
                return Ok(Self::File(input.to_path_buf()));
            }
            if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
                return Err(AppError::StackUrlScheme);
            }
            let url = url::Url::parse(text).map_err(|_| AppError::StackUrlInvalid)?;
            return Ok(Self::Url(url));
        }
        Ok(Self::File(input.to_path_buf()))
    }

    pub(crate) async fn read(&self) -> Result<String, AppError> {
        match self {
            Self::File(path) => tokio::fs::read_to_string(path)
                .await
                .map_err(AppError::StackRead),
            Self::Url(url) => read_url(url).await,
        }
    }
}

async fn read_url(url: &url::Url) -> Result<String, AppError> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("mcpstack/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5
                || attempt.url().scheme() != "http" && attempt.url().scheme() != "https"
                || attempt
                    .previous()
                    .last()
                    .is_some_and(|previous| previous.scheme() == "https")
                    && attempt.url().scheme() == "http"
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| AppError::StackRemoteRead)?;
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|_| AppError::StackRemoteRead)?;
    if !response.status().is_success() {
        return Err(AppError::StackRemoteStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err(AppError::StackRemoteSize);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::StackRemoteRead)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(AppError::StackRemoteSize);
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| AppError::StackRemoteUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_https_urls_from_windows_drive_paths() {
        assert!(matches!(
            StackSource::parse(Path::new("https://example.com/stack.yaml")).unwrap(),
            StackSource::Url(_)
        ));
        assert!(matches!(
            StackSource::parse(Path::new("C://work/stack.yaml")).unwrap(),
            StackSource::File(_)
        ));
    }
}
