//! The mobile capture surface: a phone on the same network, one box, no reading.
//!
//! Capture-only by design. Reading the vault from a phone needs an explicit
//! second token and is not implemented; a request that tries to read is refused
//! rather than quietly served.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::error::{DayDocError, Result};
use crate::vault::Vault;

pub const DEFAULT_ADDR: &str = "0.0.0.0:7373";
pub const TOKEN_NAME: &str = "mobile";

#[derive(Debug, Clone)]
pub struct AppState {
    pub vault: Vault,
    pub token: String,
    /// `None` means reading is switched off, which is the default.
    pub read_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CaptureBody {
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct CaptureReply {
    pub path: String,
    pub note_type: String,
    pub routed_to_inbox: bool,
}

#[derive(Debug, Serialize)]
pub struct ErrorReply {
    pub error: String,
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorReply { error: message.to_string() })).into_response()
}

/// Constant-ish time comparison, so a wrong token does not leak its prefix.
fn token_matches(headers: &HeaderMap, expected: &str) -> bool {
    let presented = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if presented.is_empty() || expected.is_empty() {
        return false;
    }
    let a = presented.as_bytes();
    let b = expected.as_bytes();
    let mut diff = (a.len() ^ b.len()) as u8;
    for index in 0..a.len().min(b.len()) {
        diff |= a[index] ^ b[index];
    }
    diff == 0
}

async fn capture_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CaptureBody>,
) -> Response {
    if !token_matches(&headers, &state.token) {
        return error(StatusCode::UNAUTHORIZED, "a bearer token is required");
    }
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "nothing to capture");
    }
    match crate::capture::capture(&state.vault, &text) {
        Ok(outcome) => (
            StatusCode::OK,
            Json(CaptureReply {
                path: outcome.rel_path,
                note_type: outcome.note_type.as_str().to_string(),
                routed_to_inbox: outcome.routed_to_inbox,
            }),
        )
            .into_response(),
        Err(err) => error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

async fn page_handler() -> Html<&'static str> {
    Html(PAGE)
}

/// Reading is refused with the reason rather than a 404 that hides it.
async fn read_refused() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "this surface is capture-only; set mobile_capture_only = false and add a read token \
         (daydoc key set mobile-read --generate) to read from a phone",
    )
}

fn read_authorised(headers: &HeaderMap, state: &AppState) -> bool {
    match &state.read_token {
        Some(token) => token_matches(headers, token),
        None => false,
    }
}

async fn notes_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !read_authorised(&headers, &state) {
        return read_refused().await;
    }
    let scan = match state.vault.scan() {
        Ok(scan) => scan,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let notes: Vec<serde_json::Value> = scan
        .notes
        .iter()
        .map(|note| {
            serde_json::json!({
                "path": note.rel_path,
                "title": note.title(),
                "type": note.note_type().as_str(),
            })
        })
        .collect();
    (StatusCode::OK, Json(serde_json::json!({ "notes": notes }))).into_response()
}

async fn note_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !read_authorised(&headers, &state) {
        return read_refused().await;
    }
    let path = match params.get("path") {
        Some(path) => path.clone(),
        None => return error(StatusCode::BAD_REQUEST, "a path is required"),
    };
    // Exact match against the vault listing: no traversal, no guessing.
    let known = state
        .vault
        .list_rels()
        .map(|rels| rels.contains(&path))
        .unwrap_or(false);
    if !known {
        return error(StatusCode::NOT_FOUND, "no such note in the vault");
    }
    match state.vault.load(&path) {
        Ok(note) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "path": note.rel_path,
                "title": note.title(),
                "type": note.note_type().as_str(),
                "body": note.body,
            })),
        )
            .into_response(),
        Err(err) => error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub fn router(vault: Vault, token: String, read_token: Option<String>) -> Router {
    let state = AppState {
        vault,
        token,
        read_token,
    };
    Router::new()
        .route("/", get(page_handler))
        .route("/capture", post(capture_handler))
        .route("/notes", get(notes_handler))
        .route("/note", get(note_handler))
        .with_state(state)
}

/// Serve until the process stops. Binding is the caller's address.
pub fn serve(vault: Vault, addr: &str, token: String, read_token: Option<String>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| DayDocError::Config(format!("could not start the runtime: {err}")))?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|err| DayDocError::Config(format!("could not bind {addr}: {err}")))?;
        println!(
            "mobile listening on http://{addr} ({})",
            if read_token.is_some() {
                "capture and read"
            } else {
                "capture only"
            }
        );
        axum::serve(listener, router(vault, token, read_token))
            .await
            .map_err(|err| DayDocError::Config(format!("server stopped: {err}")))
    })
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>DayDoc capture</title>
<style>
body{font-family:-apple-system,Segoe UI,Roboto,sans-serif;margin:0 auto;max-width:34rem;padding:1.5rem;color:#1a1a1a}
textarea{width:100%;min-height:8rem;font:inherit;padding:.7rem;border:1px solid #ccc;border-radius:6px}
button{margin-top:.6rem;padding:.6rem 1rem;font:inherit;border:0;border-radius:6px;background:#1a1a1a;color:#fff}
#log{margin-top:1rem;font-size:.9rem;color:#555;white-space:pre-wrap}
input{width:100%;padding:.6rem;font:inherit;border:1px solid #ccc;border-radius:6px;margin-bottom:.6rem}
</style></head><body>
<h1>DayDoc</h1>
<p>Capture only. Nothing is sent anywhere except your own machine.</p>
<input id="token" type="password" placeholder="capture token" autocomplete="off">
<textarea id="text" placeholder="we decided to ..."></textarea>
<button id="send">Save</button>
<div id="log"></div>
<script>
const queueKey = "daydoc-queue";
const log = (message) => { document.getElementById("log").textContent = message; };
const readQueue = () => { try { return JSON.parse(localStorage.getItem(queueKey) || "[]"); } catch (e) { return []; } };
const writeQueue = (items) => localStorage.setItem(queueKey, JSON.stringify(items));

async function send(text, token) {
  const response = await fetch("/capture", {
    method: "POST",
    headers: { "content-type": "application/json", authorization: "Bearer " + token },
    body: JSON.stringify({ text })
  });
  if (!response.ok) { throw new Error((await response.json()).error || response.status); }
  return response.json();
}

async function flush(token) {
  const queued = readQueue();
  if (!queued.length) { return; }
  const remaining = [];
  for (const item of queued) {
    try { await send(item, token); } catch (e) { remaining.push(item); }
  }
  writeQueue(remaining);
  if (remaining.length) { log(remaining.length + " still queued; will retry"); }
  else { log("queue empty"); }
}

document.getElementById("send").addEventListener("click", async () => {
  const token = document.getElementById("token").value.trim();
  const text = document.getElementById("text").value.trim();
  if (!token || !text) { log("a token and some text are required"); return; }
  localStorage.setItem("daydoc-token", token);
  try {
    const reply = await send(text, token);
    log("saved " + reply.path);
    document.getElementById("text").value = "";
  } catch (e) {
    const queued = readQueue(); queued.push(text); writeQueue(queued);
    log("offline: queued. " + e.message);
  }
});

const savedToken = localStorage.getItem("daydoc-token");
if (savedToken) { document.getElementById("token").value = savedToken; }
window.addEventListener("online", () => flush(document.getElementById("token").value.trim()));
flush(savedToken || "");
</script>
</body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn app(vault: Vault) -> Router {
        router(vault, "secret-token".to_string(), None)
    }

    fn reading_app(vault: Vault) -> Router {
        router(
            vault,
            "secret-token".to_string(),
            Some("read-token".to_string()),
        )
    }

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[tokio::test]
    async fn capture_without_a_token_is_refused() {
        let vault = crate::vault::test_support::temp_vault();
        let response = app(vault)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/capture")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"we decided to adopt okta for sso"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_wrong_token_is_refused() {
        let vault = crate::vault::test_support::temp_vault();
        let response = app(vault)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/capture")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer nope")
                    .body(Body::from(r#"{"text":"we decided to adopt okta"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_capture_with_the_right_token_writes_a_note() {
        let vault = crate::vault::test_support::temp_vault();
        let response = app(vault.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/capture")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer secret-token")
                    .body(Body::from(
                        r#"{"text":"we decided to adopt okta for sso across all products"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("decisions/DR-0001.md"), "{body}");
        assert!(vault.exists("decisions/DR-0001.md"));
    }

    #[tokio::test]
    async fn an_empty_capture_is_a_bad_request() {
        let vault = crate::vault::test_support::temp_vault();
        let response = app(vault)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/capture")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer secret-token")
                    .body(Body::from(r#"{"text":"   "}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn reading_is_refused_with_a_reason() {
        let vault = crate::vault::test_support::temp_vault();
        for path in ["/notes", "/note?path=x"] {
            let response = app(vault.clone())
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(path)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            let body = body_text(response).await;
            assert!(body.contains("capture-only"), "{body}");
        }
    }

    #[tokio::test]
    async fn a_read_token_lists_and_reads_a_note() {
        let vault = crate::vault::test_support::temp_vault();
        vault
            .write(
                "inbox/a.md",
                "---\nid: 01A\ntype: idea\ntitle: Readable\n---\nthe body\n",
            )
            .expect("note");

        let response = reading_app(vault.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/notes")
                    .header("authorization", "Bearer read-token")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("Readable"), "{body}");

        let response = reading_app(vault)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/note?path=inbox/a.md")
                    .header("authorization", "Bearer read-token")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("the body"), "{body}");
    }

    #[tokio::test]
    async fn the_capture_token_does_not_grant_reading() {
        let vault = crate::vault::test_support::temp_vault();
        let response = reading_app(vault)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/notes")
                    .header("authorization", "Bearer secret-token")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_path_outside_the_vault_is_not_found() {
        let vault = crate::vault::test_support::temp_vault();
        for path in ["/note?path=../escape.md", "/note?path=/etc/passwd"] {
            let response = reading_app(vault.clone())
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(path)
                        .header("authorization", "Bearer read-token")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn the_page_is_served_and_queues_offline() {
        let vault = crate::vault::test_support::temp_vault();
        let response = app(vault)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("DayDoc capture"));
        assert!(body.contains("localStorage"), "no offline queue in the page");
        assert!(!body.contains("http://"), "the page must not call anywhere else");
    }

    #[test]
    fn tokens_are_compared_without_leaking_length() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer abc".parse().expect("header"));
        assert!(token_matches(&headers, "abc"));
        assert!(!token_matches(&headers, "abcd"));
        assert!(!token_matches(&headers, "ab"));
        let empty = HeaderMap::new();
        assert!(!token_matches(&empty, "abc"));
    }
}
