//! Artist pictures from YouTube Music's own web search - the same unofficial
//! "Innertube" endpoint its website uses, no account or key of ours needed
//! (the site embeds one in its page, which is read fresh). It is undocumented
//! and may change, so every step fails soft: a miss is just "no picture".

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

const SEARCH_URL: &str = "https://music.youtube.com/youtubei/v1/search";
/// Restricts a search to the "Artists" category: a fixed, undocumented
/// parameter blob that unofficial YouTube Music clients all hardcode.
const ARTISTS_ONLY_PARAM: &str = "EgWKAQIgAWoKEAMQBBAJEAoQBQ==";
/// music.youtube.com serves an "upgrade your browser" stub to anything that
/// doesn't look like a modern browser.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

static CLIENT: Lazy<Result<reqwest::Client, String>> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(BROWSER_USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))
});

static API_KEY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#""INNERTUBE_API_KEY":\s*"([a-zA-Z0-9_-]+)""#).unwrap());
static VERSION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#""INNERTUBE_CLIENT_VERSION":"([^"]+)""#).unwrap());
static SIZE_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"=w\d+-h\d+[^/?]*$").unwrap());

/// The page's API key and client version, kept for the session once found.
static CONFIG: Lazy<tokio::sync::Mutex<Option<(String, String)>>> = Lazy::new(|| tokio::sync::Mutex::new(None));

async fn client_config(client: &reqwest::Client) -> Result<(String, String), String> {
    let mut cached = CONFIG.lock().await;
    if let Some(c) = cached.as_ref() {
        return Ok(c.clone());
    }
    let html = client
        .get("https://music.youtube.com/")
        .send()
        .await
        .map_err(|e| format!("couldn't reach YouTube Music: {e}"))?
        .text()
        .await
        .map_err(|e| format!("couldn't read YouTube Music's page: {e}"))?;
    let key = API_KEY_RE.captures(&html).map(|c| c[1].to_string()).ok_or("YouTube Music's page changed: no API key found")?;
    let version = VERSION_RE.captures(&html).map(|c| c[1].to_string()).ok_or("YouTube Music's page changed: no client version found")?;
    *cached = Some((key.clone(), version.clone()));
    Ok((key, version))
}

/// The URL of `name`'s picture, or `None` when no artist of exactly that
/// name comes up (a near-miss would put the wrong face on the wrong band).
pub(crate) async fn artist_picture_url(name: &str) -> Result<Option<String>, String> {
    let client = CLIENT.clone()?;
    let (key, version) = client_config(&client).await?;
    let body = json!({
        "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": version, "hl": "en" } },
        "query": name,
        "params": ARTISTS_ONLY_PARAM,
    });
    let resp = client
        .post(format!("{SEARCH_URL}?key={key}"))
        .header("Origin", "https://music.youtube.com")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("couldn't reach YouTube Music: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("YouTube Music returned {}", resp.status()));
    }
    let root: Value = resp.json().await.map_err(|e| format!("unexpected response from YouTube Music: {e}"))?;
    Ok(pick_artist_picture(&root, name))
}

/// Walks a search response for artist rows and returns the picture of the
/// first whose name matches `name` (ignoring case, spacing and punctuation),
/// enlarged.
pub(crate) fn pick_artist_picture(root: &Value, name: &str) -> Option<String> {
    let wanted = crate::library::group_key(name);
    let mut rows = Vec::new();
    collect_rows(root, &mut rows);
    rows.into_iter().find_map(|row| {
        let title = row.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")?.as_str()?;
        if crate::library::group_key(title) != wanted {
            return None;
        }
        let thumbs = row.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")?.as_array()?;
        let url = thumbs.last()?.get("url")?.as_str()?;
        Some(enlarge(url))
    })
}

fn collect_rows<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(row) = map.get("musicResponsiveListItemRenderer") {
                out.push(row);
            }
            map.values().for_each(|c| collect_rows(c, out));
        }
        Value::Array(items) => items.iter().for_each(|c| collect_rows(c, out)),
        _ => {}
    }
}

/// The thumbnails come small (`=w120-h120-...`); the same image is served at
/// any size by changing that suffix.
fn enlarge(url: &str) -> String {
    SIZE_SUFFIX_RE.replace(url, "=w544-h544-l90-rj").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: &str, url: &str) -> Value {
        json!({ "musicResponsiveListItemRenderer": {
            "flexColumns": [{ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": title }] } } }],
            "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [{ "url": url, "width": 120 }] } } }
        } })
    }

    #[test]
    fn picks_the_exact_name_not_the_first_row_and_enlarges_it() {
        let root = json!({ "contents": [ row("LILY", "https://x/lily=w120-h120-l90-rj"), row("NMIXX", "https://x/nmixx=w120-h120-l90-rj") ] });
        assert_eq!(pick_artist_picture(&root, "nmixx").as_deref(), Some("https://x/nmixx=w544-h544-l90-rj"));
    }

    #[test]
    fn a_near_miss_is_not_accepted() {
        let root = json!({ "contents": [ row("Dato' Sri Siti Nurhaliza", "https://x/a=w120-h120") ] });
        assert_eq!(pick_artist_picture(&root, "Siti Nurhaliza"), None);
    }

    #[test]
    fn matching_ignores_case_and_punctuation() {
        let root = json!({ "contents": [ row("M-FLO", "https://x/m=w120-h120-l90-rj") ] });
        assert!(pick_artist_picture(&root, "m flo").is_some());
    }

    /// Hits real YouTube Music. `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn finds_a_real_artist_picture() {
        let url = artist_picture_url("Arctic Monkeys").await.expect("lookup should work").expect("should find a match");
        assert!(url.contains("=w544-h544"));
    }
}
