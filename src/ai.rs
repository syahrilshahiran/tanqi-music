//! The AI provider behind the lyrics analysis and translation: one question in, the model's text out,
//! whichever service answers. Ollama's own API, anything that speaks the OpenAI chat-completions API, and
//! Anthropic's Messages API. What to ask and how to read the answer lives in `lyrics_ai`.

use crate::http::{Http, Reply};
use crate::models::{AiProvider, AppSettings};
use serde::{Deserialize, Serialize};

/// Where to send a question: the provider, its address, the model, and the API key where one is needed.
#[derive(Clone, PartialEq)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

// The key must never end up in a log or an error message.
impl std::fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiConfig")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

impl AiConfig {
    pub fn new(settings: &AppSettings, api_key: Option<String>) -> Self {
        AiConfig {
            provider: settings.ai_provider,
            base_url: settings.ai_url.clone(),
            model: settings.ai_model.clone(),
            api_key: api_key.filter(|k| !k.is_empty()),
        }
    }

    fn name(&self) -> &'static str {
        match self.provider {
            AiProvider::Ollama => "Ollama",
            AiProvider::OpenAi => "the AI provider",
            AiProvider::Anthropic => "Claude",
        }
    }

    /// Fails early, in words, when a provider that needs a key has none.
    fn require_key(&self) -> Result<&str, String> {
        match (&self.api_key, self.provider.needs_key()) {
            (Some(k), _) => Ok(k),
            (None, true) => Err(format!("no API key for {} - add it in Settings", self.name())),
            (None, false) => Ok(""),
        }
    }

    fn unreachable(&self, cause: &str) -> String {
        match self.provider {
            AiProvider::Ollama => format!(
                "couldn't reach Ollama at {} - is `ollama serve` running, and is the address right? ({cause})",
                self.base_url
            ),
            _ => format!("couldn't reach {} at {} - is the address right, and are you online? ({cause})", self.name(), self.base_url),
        }
    }

    fn failed(&self, reply: &Reply) -> String {
        let body = String::from_utf8_lossy(&reply.body);
        match self.provider {
            AiProvider::Ollama => format!(
                "Ollama returned {} - is `{}` pulled? (`ollama pull {}`) {body}",
                reply.status, self.model, self.model
            ),
            _ if reply.status.as_u16() == 401 || reply.status.as_u16() == 403 => {
                format!("{} refused the API key ({}) - check it in Settings. {}", self.name(), reply.status, short(&body))
            }
            _ => format!("{} returned {} {}", self.name(), reply.status, short(&body)),
        }
    }
}

fn short(s: &str) -> String {
    s.chars().take(300).collect()
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub model_found: bool,
    pub models: Vec<String>,
}

// ---- the three providers' reply shapes ----

#[derive(Deserialize)]
struct OllamaGenerate {
    response: String,
}
#[derive(Deserialize)]
struct OllamaTags {
    models: Vec<OllamaTag>,
}
#[derive(Deserialize)]
struct OllamaTag {
    name: String,
}

#[derive(Deserialize)]
struct OpenAiChat {
    choices: Vec<OpenAiChoice>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicMessage {
    content: Vec<AnthropicBlock>,
}
#[derive(Deserialize)]
struct AnthropicBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// `{"data": [{"id": ...}]}` - the model list of OpenAI-compatible and Anthropic APIs.
#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelEntry>,
}
#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

const ANTHROPIC_VERSION: &str = "2023-06-01";
/// The most a hosted model may write for one request (Anthropic requires a limit).
const MAX_OUTPUT_TOKENS: u32 = 8192;

/// What the lyrics need to be answered with: sends `prompt` and returns the model's reply text.
pub async fn complete(http: &impl Http, cfg: &AiConfig, prompt: &str) -> Result<String, String> {
    let base = cfg.base_url.trim_end_matches('/');
    match cfg.provider {
        AiProvider::Ollama => {
            let resp = http
                .post_json(
                    &format!("{base}/api/generate"),
                    &serde_json::json!({
                        "model": cfg.model,
                        "prompt": prompt,
                        "stream": false,
                        "format": "json",
                        // Don't spend the answer's token budget on reasoning, and give
                        // the model room for a long lyric plus its answer (the default
                        // window is small enough to cut a reply off mid-JSON).
                        "think": false,
                        "options": { "num_ctx": 8192 },
                    }),
                )
                .await
                .map_err(|e| cfg.unreachable(&e.to_string()))?;
            if !resp.is_success() {
                return Err(cfg.failed(&resp));
            }
            let body: OllamaGenerate = resp.json().map_err(|e| format!("unexpected response from Ollama: {e}"))?;
            Ok(body.response)
        }
        AiProvider::OpenAi => {
            let key = cfg.require_key()?;
            let auth = format!("Bearer {key}");
            let resp = http
                .post_json_with_headers(
                    &format!("{base}/chat/completions"),
                    &[("Authorization", auth.as_str())],
                    &serde_json::json!({
                        "model": cfg.model,
                        "messages": [{"role": "user", "content": prompt}],
                    }),
                )
                .await
                .map_err(|e| cfg.unreachable(&e.to_string()))?;
            if !resp.is_success() {
                return Err(cfg.failed(&resp));
            }
            let body: OpenAiChat = resp.json().map_err(|e| format!("unexpected response from {}: {e}", cfg.name()))?;
            body.choices
                .into_iter()
                .find_map(|c| c.message.content)
                .ok_or_else(|| format!("{} sent an empty answer", cfg.name()))
        }
        AiProvider::Anthropic => {
            let key = cfg.require_key()?;
            let resp = http
                .post_json_with_headers(
                    &format!("{base}/v1/messages"),
                    &[("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION)],
                    &serde_json::json!({
                        "model": cfg.model,
                        "max_tokens": MAX_OUTPUT_TOKENS,
                        "messages": [{"role": "user", "content": prompt}],
                    }),
                )
                .await
                .map_err(|e| cfg.unreachable(&e.to_string()))?;
            if !resp.is_success() {
                return Err(cfg.failed(&resp));
            }
            let body: AnthropicMessage = resp.json().map_err(|e| format!("unexpected response from {}: {e}", cfg.name()))?;
            let text: String = body.content.into_iter().filter(|b| b.kind == "text").map(|b| b.text).collect();
            if text.is_empty() {
                Err(format!("{} sent an empty answer", cfg.name()))
            } else {
                Ok(text)
            }
        }
    }
}

/// Checks that the provider is reachable (and the key accepted) and whether the chosen model is among the
/// ones it lists. Used by Settings' "Test connection".
pub async fn check_connection(http: &impl Http, cfg: &AiConfig) -> Result<ConnectionStatus, String> {
    let base = cfg.base_url.trim_end_matches('/');
    let models: Vec<String> = match cfg.provider {
        AiProvider::Ollama => {
            let resp = http.get(&format!("{base}/api/tags"), &[]).await.map_err(|e| cfg.unreachable(&e.to_string()))?;
            if !resp.is_success() {
                return Err(format!("Ollama returned {}", resp.status));
            }
            let body: OllamaTags = resp.json().map_err(|e| format!("unexpected response from Ollama: {e}"))?;
            body.models.into_iter().map(|m| m.name).collect()
        }
        AiProvider::OpenAi => {
            let key = cfg.require_key()?;
            let auth = format!("Bearer {key}");
            let resp = http
                .get_with_headers(&format!("{base}/models"), &[("Authorization", auth.as_str())])
                .await
                .map_err(|e| cfg.unreachable(&e.to_string()))?;
            model_ids(cfg, &resp)?
        }
        AiProvider::Anthropic => {
            let key = cfg.require_key()?;
            let resp = http
                .get_with_headers(&format!("{base}/v1/models"), &[("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION)])
                .await
                .map_err(|e| cfg.unreachable(&e.to_string()))?;
            model_ids(cfg, &resp)?
        }
    };
    // Some providers list ids with a prefix ("models/gemini-2.0-flash"); the bare id counts as found.
    let model_found = models.iter().any(|m| m == &cfg.model || m.strip_prefix("models/") == Some(cfg.model.as_str()));
    Ok(ConnectionStatus { model_found, models })
}

fn model_ids(cfg: &AiConfig, resp: &Reply) -> Result<Vec<String>, String> {
    if !resp.is_success() {
        return Err(cfg.failed(resp));
    }
    let body: ModelList = resp.json().map_err(|e| format!("unexpected response from {}: {e}", cfg.name()))?;
    Ok(body.data.into_iter().map(|m| m.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use serde_json::json;

    fn cfg(provider: AiProvider, key: Option<&str>) -> AiConfig {
        AiConfig {
            provider,
            base_url: provider.default_url().to_string(),
            model: "some-model".to_string(),
            api_key: key.map(String::from),
        }
    }

    #[tokio::test]
    async fn an_openai_compatible_service_gets_a_chat_completion_with_the_key_in_a_header() {
        let http = FakeHttp::default().json("/chat/completions", json!({"choices": [{"message": {"content": "{\"a\":1}"}}]}));
        let c = cfg(AiProvider::OpenAi, Some("sk-secret"));
        assert_eq!(complete(&http, &c, "hello").await.unwrap(), "{\"a\":1}");

        let sent = http.requests();
        assert!(sent[0].starts_with("POST https://api.openai.com/v1/chat/completions "), "{sent:?}");
        assert!(sent[0].contains("\"model\":\"some-model\"") && sent[0].contains("\"content\":\"hello\""));
        assert_eq!(http.headers_sent(), vec!["Authorization: Bearer sk-secret".to_string()]);
        // The key travels in a header only, never in the URL or the body.
        assert!(!sent[0].contains("sk-secret"));
    }

    #[tokio::test]
    async fn claude_gets_a_messages_request_and_its_text_blocks_are_joined() {
        let http = FakeHttp::default().json(
            "/v1/messages",
            json!({"content": [{"type": "thinking", "thinking": "hm"}, {"type": "text", "text": "{\"a\":"}, {"type": "text", "text": "1}"}]}),
        );
        let c = cfg(AiProvider::Anthropic, Some("sk-ant"));
        assert_eq!(complete(&http, &c, "hello").await.unwrap(), "{\"a\":1}");

        let sent = http.requests();
        assert!(sent[0].starts_with("POST https://api.anthropic.com/v1/messages "), "{sent:?}");
        assert!(sent[0].contains("\"max_tokens\":8192"));
        assert_eq!(
            http.headers_sent(),
            vec!["x-api-key: sk-ant".to_string(), "anthropic-version: 2023-06-01".to_string()]
        );
    }

    #[tokio::test]
    async fn ollama_is_asked_in_its_own_way_with_no_key() {
        let http = FakeHttp::default().json("/api/generate", json!({"response": "{}"}));
        assert_eq!(complete(&http, &cfg(AiProvider::Ollama, None), "hi").await.unwrap(), "{}");
        assert!(http.requests()[0].contains("http://localhost:11434/api/generate"));
        assert!(http.headers_sent().is_empty());
    }

    #[tokio::test]
    async fn a_hosted_provider_without_a_key_fails_before_any_request() {
        let http = FakeHttp::default();
        for p in [AiProvider::OpenAi, AiProvider::Anthropic] {
            let err = complete(&http, &cfg(p, None), "hi").await.unwrap_err();
            assert!(err.contains("no API key") && err.contains("Settings"), "{err}");
            assert!(check_connection(&http, &cfg(p, None)).await.is_err());
        }
        assert!(http.requests().is_empty());
    }

    #[tokio::test]
    async fn a_refused_key_says_so_and_the_key_is_not_in_the_message() {
        let http = FakeHttp::default().status("/chat/completions", 401);
        let err = complete(&http, &cfg(AiProvider::OpenAi, Some("sk-secret")), "hi").await.unwrap_err();
        assert!(err.contains("refused the API key"), "{err}");
        assert!(!err.contains("sk-secret"));

        let http = FakeHttp::default().unreachable("/v1/messages", "dns error");
        let err = complete(&http, &cfg(AiProvider::Anthropic, Some("sk-secret")), "hi").await.unwrap_err();
        assert!(err.starts_with("couldn't reach Claude at https://api.anthropic.com"), "{err}");
        assert!(!err.contains("sk-secret"));
    }

    #[test]
    fn the_config_never_prints_its_key() {
        let shown = format!("{:?}", cfg(AiProvider::OpenAi, Some("sk-secret")));
        assert!(!shown.contains("sk-secret") && shown.contains("<set>"), "{shown}");
    }

    #[tokio::test]
    async fn the_connection_check_lists_models_and_finds_the_chosen_one() {
        let http = FakeHttp::default().json("/models", json!({"data": [{"id": "models/some-model"}, {"id": "other"}]}));
        let status = check_connection(&http, &cfg(AiProvider::OpenAi, Some("k"))).await.unwrap();
        assert!(status.model_found);
        assert_eq!(status.models.len(), 2);
        assert_eq!(http.headers_sent(), vec!["Authorization: Bearer k".to_string()]);

        let http = FakeHttp::default().json("/v1/models", json!({"data": [{"id": "claude-x"}]}));
        let status = check_connection(&http, &cfg(AiProvider::Anthropic, Some("k"))).await.unwrap();
        assert!(!status.model_found);

        let http = FakeHttp::default().json("/api/tags", json!({"models": [{"name": "some-model"}]}));
        assert!(check_connection(&http, &cfg(AiProvider::Ollama, None)).await.unwrap().model_found);
    }
}
