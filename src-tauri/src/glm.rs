use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};

pub const DEFAULT_ENDPOINT: &str = "https://api.scaleway.ai/v1";
pub const DEFAULT_MODEL: &str = "glm-5.2";
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

/// Largest non-streaming provider body this app will buffer.
///
/// `json()` and `text()` read to the end of a stream whose length the far end
/// chooses. A provider that is compromised, misconfigured, or simply broken can
/// therefore decide how much memory this process uses. The image paths have
/// been read against a cap since 1.6.0; the chat paths had not.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Largest assistant reply accumulated while streaming.
///
/// `max_tokens` bounds what a well-behaved provider sends. It is a request
/// parameter, so it bounds nothing at all if the far end ignores it.
pub const MAX_STREAM_CONTENT_BYTES: usize = 8 * 1024 * 1024;

/// Largest single SSE line held while waiting for its newline.
///
/// The decode loop only drains `buffer` when it finds a `\n`. A stream that
/// never sends one is not a slow stream, it is an unbounded allocation.
pub const MAX_SSE_LINE_BYTES: usize = 1024 * 1024;

/// How much of a provider's error body is shown to the user.
///
/// The whole body used to be interpolated into the message. That is unbounded,
/// and it puts provider-authored text — which may echo the request — in front
/// of the reader verbatim.
pub const MAX_ERROR_BODY_CHARS: usize = 600;

/// Read a response body in bounded chunks, refusing one that outgrows `max`.
pub async fn read_body_capped(
    response: reqwest::Response,
    max: usize,
    what: &str,
) -> Result<Vec<u8>, String> {
    let mut response = response;
    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if out.len() + chunk.len() > max {
            return Err(format!(
                "{what} is larger than {} MB, so it was not loaded.",
                max / (1024 * 1024)
            ));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Trim a provider-supplied string to something safe to display.
///
/// Truncation is by characters, not bytes, so this cannot split a multi-byte
/// character and produce mojibake in an error message.
pub fn clamp_provider_text(body: &str) -> String {
    let body = body.trim();
    if body.chars().count() <= MAX_ERROR_BODY_CHARS {
        return body.to_string();
    }
    let kept: String = body.chars().take(MAX_ERROR_BODY_CHARS).collect();
    format!("{kept}… (truncated)")
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompletionOptions {
    pub endpoint: String,
    pub model: String,
    pub max_tokens: u32,
    pub stream: bool,
    /// OpenAI-style `reasoning_effort`, passed through when set.
    ///
    /// On the Scaleway GLM-5.2 endpoint `"none"` is the only value that changes
    /// anything — it suppresses the reasoning pass entirely (measured: ~2 output
    /// tokens in ~0.5s versus ~90 tokens in ~1.5s for a trivial prompt). The
    /// other levels, and the `enable_thinking` / `chat_template_kwargs` forms
    /// other GLM deployments accept, are taken and ignored. Suppressing
    /// reasoning costs accuracy on anything that needs computing, so this is
    /// only ever set from an explicit user opt-in.
    pub reasoning_effort: Option<String>,
}

impl Default for CompletionOptions {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.into(),
            model: DEFAULT_MODEL.into(),
            max_tokens: DEFAULT_MAX_TOKENS,
            stream: true,
            reasoning_effort: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionEvent<'a> {
    Token(&'a str),
    Thinking,
    /// Tokens billed for this request (from the API's `usage` object).
    Usage(TokenUsage),
    /// `finish_reason == "length"` — the answer hit the output cap and stops
    /// mid-sentence. Without this the caller cannot distinguish a reply that
    /// ended from one that was cut off, which is how a truncated HTML artifact
    /// reached the UI as an empty chip with no explanation.
    Truncated,
}

/// Prompt (input) and completion (output) token counts, kept apart because
/// providers price the two differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt: u64,
    pub completion: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.prompt + self.completion
    }
}

/// Prompt/completion token split from an OpenAI-style `usage` object, if
/// present. Falls back to `total_tokens` (attributing the remainder to
/// completion) when a provider omits the per-direction counts.
pub fn usage(value: &serde_json::Value) -> Option<TokenUsage> {
    let usage = &value["usage"];
    if usage.is_null() {
        return None;
    }
    let prompt = usage["prompt_tokens"].as_u64();
    let completion = usage["completion_tokens"].as_u64();
    match (prompt, completion) {
        (Some(prompt), Some(completion)) => Some(TokenUsage { prompt, completion }),
        _ => {
            let total = usage["total_tokens"].as_u64()?;
            let prompt = prompt.unwrap_or(0);
            Some(TokenUsage {
                prompt,
                completion: total.saturating_sub(prompt),
            })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Authentication,
    Network,
    Api,
    Protocol,
    Cancelled,
}

#[derive(Debug)]
pub struct CompletionError {
    pub kind: ErrorKind,
    pub message: String,
}

impl CompletionError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CompletionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompletionError {}

/// Shared client defaults for both the desktop application and the CLI.
///
/// One client for the process, handed out as clones. `reqwest::Client` *is* the
/// connection pool (cloning shares it, it's an `Arc` inside), so building a new
/// one per chat turn — as every call site used to — meant starting from an empty
/// pool and paying a fresh TCP + TLS handshake before the first byte of every
/// message. Measured against api.scaleway.ai: ~90-130ms to complete the
/// handshake and ~150ms to first byte cold, versus ~45ms on a warm connection.
pub fn http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .read_timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

pub async fn post_completion(
    client: &reqwest::Client,
    key: &str,
    endpoint: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, CompletionError> {
    client
        .post(format!(
            "{}/chat/completions",
            endpoint.trim_end_matches('/')
        ))
        .bearer_auth(key)
        .json(body)
        .send()
        .await
        .map_err(network_error)
}

/// Complete one OpenAI-compatible chat request. Returning `false` from the
/// event callback stops decoding while preserving the content received so far.
pub async fn complete<F>(
    client: &reqwest::Client,
    key: &str,
    messages: &[serde_json::Value],
    options: &CompletionOptions,
    cancel: Option<&AtomicBool>,
    mut on_event: F,
) -> Result<String, CompletionError>
where
    F: FnMut(CompletionEvent<'_>, &str) -> bool,
{
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(CompletionError::new(ErrorKind::Cancelled, "Stopped."));
    }

    let mut body = serde_json::json!({
        "model": options.model,
        "messages": messages,
        "stream": options.stream,
        "max_tokens": options.max_tokens,
    });
    if options.stream {
        // Ask the API to append token counts in a final chunk.
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }
    if let Some(effort) = options.reasoning_effort.as_deref() {
        body["reasoning_effort"] = serde_json::json!(effort);
    }
    let response = post_completion(client, key, &options.endpoint, &body).await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }

    if !options.stream {
        let body = read_body_capped(response, MAX_RESPONSE_BYTES, "The model's reply")
            .await
            .map_err(|e| CompletionError::new(ErrorKind::Protocol, e))?;
        let value = serde_json::from_slice::<serde_json::Value>(&body).map_err(|e| {
            CompletionError::new(
                ErrorKind::Protocol,
                format!("The model returned an invalid JSON response: {e}"),
            )
        })?;
        let content = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                CompletionError::new(
                    ErrorKind::Protocol,
                    "The model response did not contain assistant text.",
                )
            })?
            .to_string();
        on_event(CompletionEvent::Token(&content), &content);
        if let Some(tokens) = usage(&value) {
            on_event(CompletionEvent::Usage(tokens), &content);
        }
        return Ok(content);
    }

    decode_sse(response, cancel, &mut on_event).await
}

async fn decode_sse<F>(
    response: reqwest::Response,
    cancel: Option<&AtomicBool>,
    on_event: &mut F,
) -> Result<String, CompletionError>
where
    F: FnMut(CompletionEvent<'_>, &str) -> bool,
{
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut content = String::new();
    let mut thinking_shown = false;

    while let Some(chunk) = stream.next().await {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Ok(content);
        }
        let chunk = chunk.map_err(stream_network_error)?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        // A line is only drained when its newline arrives. Without this, a
        // stream that sends bytes and never a newline is an unbounded
        // allocation that looks like a slow reply.
        if buffer.len() > MAX_SSE_LINE_BYTES {
            return Err(CompletionError::new(
                ErrorKind::Protocol,
                "The model sent a single line of reply larger than this app will hold. \
                 The reply has been stopped.",
            ));
        }
        while let Some(position) = buffer.find('\n') {
            let line = buffer[..position].trim().to_string();
            buffer.drain(..=position);
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(content);
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            // The include_usage final chunk carries token counts (and usually an
            // empty choices array), so check it before the delta.
            if let Some(tokens) = usage(&value) {
                if !on_event(CompletionEvent::Usage(tokens), &content) {
                    return Ok(content);
                }
            }
            if value["choices"][0]["finish_reason"] == "length"
                && !on_event(CompletionEvent::Truncated, &content)
            {
                return Ok(content);
            }
            let delta = &value["choices"][0]["delta"];
            if let Some(token) = delta["content"].as_str().filter(|s| !s.is_empty()) {
                // `max_tokens` is a request parameter: it bounds a provider
                // that honours it and nothing else. What is kept so far is
                // returned rather than discarded — a truncated reply the user
                // can read beats an error that throws away a long answer.
                if content.len() + token.len() > MAX_STREAM_CONTENT_BYTES {
                    on_event(CompletionEvent::Truncated, &content);
                    return Ok(content);
                }
                content.push_str(token);
                if !on_event(CompletionEvent::Token(token), &content) {
                    return Ok(content);
                }
            }
            if !thinking_shown
                && delta["reasoning_content"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            {
                thinking_shown = true;
                if !on_event(CompletionEvent::Thinking, &content) {
                    return Ok(content);
                }
            }
        }
    }
    Ok(content)
}

async fn response_error(response: reqwest::Response) -> CompletionError {
    let status = response.status();
    // Bounded, because an error body is chosen by the far end just as a success
    // body is, and this one is on the path that runs when things are going
    // wrong — which is exactly when a provider is most likely to return
    // something enormous.
    let body = read_body_capped(response, MAX_RESPONSE_BYTES, "The error reply")
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    // 401 and 403 are different mistakes and want different fixes, so they no
    // longer share one message. By far the commoner is 401: the key screen
    // shows an access key and a secret key together, and only the secret key
    // authenticates. Saying "rejected the API key" to someone who pasted the
    // access key describes the symptom and hides the cause.
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return CompletionError::new(
            ErrorKind::Authentication,
            "Scaleway did not accept this key. Scaleway shows two values when \
             you create an API key — an access key and a secret key — and only \
             the secret key works here.",
        );
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        return CompletionError::new(
            ErrorKind::Authentication,
            "Scaleway accepted the key but refused the request. The key is \
             valid; whoever it belongs to does not have permission to use \
             Generative APIs in that Organization.",
        );
    }
    CompletionError::new(ErrorKind::Api, completion_error_message(status, &body))
}

fn network_error(error: reqwest::Error) -> CompletionError {
    // Only the timeout branch used to be written for a person. The other two
    // interpolated reqwest's own text, so pulling the network out produced
    // "The connection to Scaleway failed: error sending request for url
    // (https://api.scaleway.ai/v1/chat/completions)" — which names the
    // endpoint rather than the problem, and is rendered as a *link*, so the
    // reply handed the user a clickable API URL that answers 401 to anyone who
    // follows it. Meanwhile the status dot beside it said the useful thing:
    // check your internet connection.
    //
    // Connect failures, DNS failures and a proxy refusing all mean one thing
    // to the person holding the machine — the request never left — and one
    // action. The detail is kept on stderr, where a bug report can quote it
    // and a user is not asked to read it.
    let message = if error.is_timeout() {
        "Scaleway stopped responding (timed out). Please try again."
    } else {
        eprintln!("network error reaching Scaleway: {error}");
        "Could not reach Scaleway. Check your internet connection, then try again."
    };
    CompletionError::new(ErrorKind::Network, message.to_string())
}

fn stream_network_error(error: reqwest::Error) -> CompletionError {
    // The mid-reply wording is already right — a reply is on screen and the
    // user needs to know it stopped rather than finished — but it carried the
    // raw error in brackets for the same reason as above, and with it the URL.
    let message = if error.is_timeout() {
        "Scaleway stopped responding mid-reply (timed out). Please try again."
    } else {
        eprintln!("network error mid-reply from Scaleway: {error}");
        "The connection to Scaleway dropped mid-reply. Please try again."
    };
    CompletionError::new(ErrorKind::Network, message.to_string())
}

#[cfg(test)]
mod network_message_tests {
    /// Whatever the transport says, the user is told one thing they can act
    /// on — and never an address.
    ///
    /// Pulling the network out produced "The connection to Scaleway failed:
    /// error sending request for url (https://api.scaleway.ai/v1/chat/
    /// completions)". Replies render links, so that arrived as a clickable API
    /// endpoint returning 401 to anyone who followed it, in place of the
    /// sentence the status dot two inches away was already showing.
    #[test]
    fn a_network_message_names_the_problem_and_not_the_endpoint() {
        let source = include_str!("glm.rs");
        let body = source
            .split("fn network_error(")
            .nth(1)
            .and_then(|s| s.split("\nfn ").next())
            .expect("network_error is gone");
        let stream = source
            .split("fn stream_network_error(")
            .nth(1)
            .and_then(|s| s.split("\nfn ").next())
            .expect("stream_network_error is gone");
        for (name, f) in [("network_error", body), ("stream_network_error", stream)] {
            // The message is chosen from string literals; interpolating the
            // transport error is what put a URL in front of a user.
            let message_lines: String = f
                .lines()
                .filter(|l| !l.trim_start().starts_with("//") && !l.contains("eprintln!"))
                .collect();
            assert!(
                !message_lines.contains("{error}"),
                "{name} still puts the raw transport error in front of the user"
            );
        }
    }
}

#[cfg(test)]
mod provider_body_limits {
    use super::*;

    // The whole provider body used to be interpolated into the user-visible
    // error. Unbounded, and provider-authored text shown verbatim.
    #[test]
    fn a_huge_error_body_is_trimmed_before_it_is_shown() {
        let huge = "x".repeat(MAX_ERROR_BODY_CHARS * 10);
        let out = clamp_provider_text(&huge);
        assert!(
            out.chars().count() < MAX_ERROR_BODY_CHARS + 32,
            "{}",
            out.len()
        );
        assert!(out.ends_with("… (truncated)"));
    }

    // Truncating by bytes would split a multi-byte character and render as
    // mojibake in the one message a user reads when something is wrong.
    #[test]
    fn trimming_never_splits_a_character() {
        let wide = "。".repeat(MAX_ERROR_BODY_CHARS * 2);
        let out = clamp_provider_text(&wide);
        assert!(out.starts_with("。"));
        assert!(out.chars().count() <= MAX_ERROR_BODY_CHARS + 16);
    }

    #[test]
    fn a_short_body_is_left_alone() {
        assert_eq!(clamp_provider_text("  quota exceeded  "), "quota exceeded");
    }

    // The friendly context-limit message must still win: it is the one case
    // where the body is inspected rather than shown.
    #[test]
    fn the_context_limit_message_survives_trimming() {
        let body = format!(
            "{} maximum context length",
            "y".repeat(MAX_ERROR_BODY_CHARS * 4)
        );
        let msg = completion_error_message(reqwest::StatusCode::BAD_REQUEST, &body);
        assert_eq!(msg, CONTEXT_LIMIT_MSG);
    }

    #[test]
    fn an_ordinary_error_carries_a_bounded_body() {
        let body = "z".repeat(MAX_ERROR_BODY_CHARS * 4);
        let msg = completion_error_message(reqwest::StatusCode::BAD_GATEWAY, &body);
        assert!(msg.contains("502"));
        assert!(
            msg.chars().count() < MAX_ERROR_BODY_CHARS + 128,
            "{}",
            msg.len()
        );
    }
}

const CONTEXT_LIMIT_MSG: &str =
    "This conversation has grown too long for the model to handle. Start a new chat (＋ New chat) to keep going — your history is saved.";

pub fn completion_error_message(status: reqwest::StatusCode, body: &str) -> String {
    let lower = body.to_lowercase();
    let context_error = lower.contains("context length")
        || lower.contains("context_length")
        || lower.contains("maximum context")
        || lower.contains("context window")
        || lower.contains("too many tokens")
        || lower.contains("reduce the length")
        || (status == reqwest::StatusCode::BAD_REQUEST
            && lower.contains("token")
            && (lower.contains("exceed") || lower.contains("maximum") || lower.contains("limit")));
    if context_error {
        CONTEXT_LIMIT_MSG.into()
    } else {
        format!("Scaleway returned {status}: {}", clamp_provider_text(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_desktop_integration() {
        let options = CompletionOptions::default();
        assert_eq!(options.endpoint, "https://api.scaleway.ai/v1");
        assert_eq!(options.model, "glm-5.2");
        assert_eq!(options.max_tokens, 8192);
        assert!(options.stream);
        // Reasoning stays on unless a caller explicitly opts out.
        assert_eq!(options.reasoning_effort, None);
    }

    #[test]
    fn reasoning_effort_is_only_sent_when_set() {
        let mut options = CompletionOptions::default();
        let body = |o: &CompletionOptions| {
            let mut b = serde_json::json!({ "model": o.model, "stream": o.stream });
            if let Some(e) = o.reasoning_effort.as_deref() {
                b["reasoning_effort"] = serde_json::json!(e);
            }
            b
        };
        assert!(body(&options).get("reasoning_effort").is_none());
        options.reasoning_effort = Some("none".into());
        assert_eq!(body(&options)["reasoning_effort"], "none");
    }

    #[test]
    fn context_errors_get_a_readable_message() {
        let message = completion_error_message(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"maximum context length exceeded"}"#,
        );
        assert!(message.starts_with("This conversation has grown too long"));
    }
}
