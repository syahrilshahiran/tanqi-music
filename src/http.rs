//! The seam between the metadata providers and the network. Providers talk
//! to `Http`; production uses `ReqwestHttp` (one shared client, so one
//! connection pool and one User-Agent), tests use a scripted fake.

use serde::de::DeserializeOwned;
use std::future::Future;

/// Sent on every request. MusicBrainz and LRCLIB ask clients for a
/// descriptive User-Agent (etiquette, not auth) - kept generic and
/// non-personal since this is a local desktop app, not a hosted service.
const USER_AGENT: &str = "Tanqi/0.1 (Dioxus desktop app)";

/// Lookups (GET/HEAD) against public services give up after this, so a hung
/// one fails (and can be retried) instead of leaving the UI waiting.
const GET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

static HTTP_CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        // A server that doesn't answer at all should fail quickly. There is
        // deliberately no overall timeout here: a local model can take
        // minutes to answer a POST (see `GET_TIMEOUT` for the lookups).
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        // Only fails if the TLS backend can't initialize, which would break
        // every network feature in the app regardless.
        .expect("failed to build the shared HTTP client")
});

/// The shared client: one connection pool and one User-Agent for every request.
fn client() -> reqwest::Client {
    HTTP_CLIENT.clone()
}

/// The request never produced a response (offline, DNS, TLS, a dropped body).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpError(pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A response that arrived, whatever its status.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: reqwest::StatusCode,
    pub content_length: Option<u64>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_slice(&self.body).map_err(|e| e.to_string())
    }
}

pub trait Http: Send + Sync {
    fn get(&self, url: &str, query: &[(&str, &str)]) -> impl Future<Output = Result<Reply, HttpError>> + Send;
    fn head(&self, url: &str) -> impl Future<Output = Result<Reply, HttpError>> + Send;
    fn post_json(&self, url: &str, body: &serde_json::Value) -> impl Future<Output = Result<Reply, HttpError>> + Send;
    /// `get` with extra request headers (an API key, say).
    fn get_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> impl Future<Output = Result<Reply, HttpError>> + Send;
    /// `post_json` with extra request headers (an API key, say).
    fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &serde_json::Value,
    ) -> impl Future<Output = Result<Reply, HttpError>> + Send;
}

pub struct ReqwestHttp;

impl Http for ReqwestHttp {
    async fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<Reply, HttpError> {
        let resp = client().get(url).query(query).timeout(GET_TIMEOUT).send().await.map_err(|e| HttpError(e.to_string()))?;
        let status = resp.status();
        let body = resp.bytes().await.map_err(|e| HttpError(format!("failed to read the response: {e}")))?;
        Ok(Reply { status, content_length: Some(body.len() as u64), body: body.to_vec() })
    }

    async fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<Reply, HttpError> {
        let resp = client().post(url).json(body).send().await.map_err(|e| HttpError(e.to_string()))?;
        let status = resp.status();
        let body = resp.bytes().await.map_err(|e| HttpError(format!("failed to read the response: {e}")))?;
        Ok(Reply { status, content_length: Some(body.len() as u64), body: body.to_vec() })
    }

    async fn get_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, HttpError> {
        let mut req = client().get(url).timeout(GET_TIMEOUT);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.map_err(|e| HttpError(e.to_string()))?;
        let status = resp.status();
        let body = resp.bytes().await.map_err(|e| HttpError(format!("failed to read the response: {e}")))?;
        Ok(Reply { status, content_length: Some(body.len() as u64), body: body.to_vec() })
    }

    async fn post_json_with_headers(&self, url: &str, headers: &[(&str, &str)], body: &serde_json::Value) -> Result<Reply, HttpError> {
        let mut req = client().post(url).json(body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.map_err(|e| HttpError(e.to_string()))?;
        let status = resp.status();
        let body = resp.bytes().await.map_err(|e| HttpError(format!("failed to read the response: {e}")))?;
        Ok(Reply { status, content_length: Some(body.len() as u64), body: body.to_vec() })
    }

    async fn head(&self, url: &str) -> Result<Reply, HttpError> {
        let resp = client().head(url).timeout(GET_TIMEOUT).send().await.map_err(|e| HttpError(e.to_string()))?;
        let content_length = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        Ok(Reply { status: resp.status(), content_length, body: Vec::new() })
    }
}

/// Scripted network for tests: routes match on `"GET <url>?<k=v&k=v>"` (or
/// `HEAD <url>`, or `POST <url> <json body>`) by substring, first match wins, anything unmatched is a
/// 404. Every request is recorded.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeHttp {
        routes: Vec<(String, Result<Reply, HttpError>)>,
        requests: Mutex<Vec<String>>,
        /// The headers of each request that sent any, as `name: value` lines.
        headers: Mutex<Vec<String>>,
    }

    fn reply(status: u16, content_length: Option<u64>, body: Vec<u8>) -> Reply {
        Reply { status: reqwest::StatusCode::from_u16(status).unwrap(), content_length, body }
    }

    impl FakeHttp {
        pub fn json(mut self, pattern: &str, body: serde_json::Value) -> Self {
            let body = serde_json::to_vec(&body).unwrap();
            self.routes.push((pattern.to_string(), Ok(reply(200, Some(body.len() as u64), body))));
            self
        }
        pub fn status(mut self, pattern: &str, status: u16) -> Self {
            self.routes.push((pattern.to_string(), Ok(reply(status, None, Vec::new()))));
            self
        }
        pub fn bytes(mut self, pattern: &str, body: &[u8]) -> Self {
            self.routes.push((pattern.to_string(), Ok(reply(200, Some(body.len() as u64), body.to_vec()))));
            self
        }
        pub fn head(mut self, pattern: &str, status: u16, content_length: u64) -> Self {
            self.routes.push((pattern.to_string(), Ok(reply(status, Some(content_length), Vec::new()))));
            self
        }
        pub fn unreachable(mut self, pattern: &str, message: &str) -> Self {
            self.routes.push((pattern.to_string(), Err(HttpError(message.to_string()))));
            self
        }
        pub fn headers_sent(&self) -> Vec<String> {
            self.headers.lock().unwrap().clone()
        }
        pub fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }

        fn respond(&self, line: String) -> Result<Reply, HttpError> {
            self.requests.lock().unwrap().push(line.clone());
            self.routes
                .iter()
                .find(|(pattern, _)| line.contains(pattern.as_str()))
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| Ok(reply(404, None, Vec::new())))
        }
    }

    impl Http for FakeHttp {
        async fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<Reply, HttpError> {
            let q: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
            self.respond(format!("GET {url}?{}", q.join("&")))
        }
        async fn head(&self, url: &str) -> Result<Reply, HttpError> {
            self.respond(format!("HEAD {url}"))
        }
        async fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<Reply, HttpError> {
            self.respond(format!("POST {url} {body}"))
        }
        async fn get_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, HttpError> {
            self.record_headers(headers);
            self.respond(format!("GET {url}?"))
        }
        async fn post_json_with_headers(&self, url: &str, headers: &[(&str, &str)], body: &serde_json::Value) -> Result<Reply, HttpError> {
            self.record_headers(headers);
            self.respond(format!("POST {url} {body}"))
        }
    }

    impl FakeHttp {
        fn record_headers(&self, headers: &[(&str, &str)]) {
            self.headers.lock().unwrap().extend(headers.iter().map(|(k, v)| format!("{k}: {v}")));
        }
    }
}
