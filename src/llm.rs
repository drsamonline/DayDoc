//! The optional LLM: opt-in, off by default, and never the only way to do
//! anything.
//!
//! DayDoc sends the top-k retrieved notes — never the vault — and shows what it
//! will send before it sends it. `llm_send_content` is a second lock on the same
//! door: with it false, nothing leaves the machine.

use serde_json::{json, Value};

use crate::error::{DayDocError, Result};
use crate::search::{self, Filters};
use crate::vault::Vault;

/// Characters of note text allowed into one request.
pub const MAX_CONTEXT_CHARS: usize = 8_000;
pub const DEFAULT_K: usize = 6;

#[derive(Debug, Clone)]
pub struct LlmSettings {
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub send_content: bool,
}

impl LlmSettings {
    pub fn from_config(config: &crate::config::Config) -> LlmSettings {
        let provider = if config.llm_provider.trim().is_empty() {
            "openai".to_string()
        } else {
            config.llm_provider.trim().to_ascii_lowercase()
        };
        LlmSettings {
            mode: config.llm_mode.clone(),
            base_url: default_base_url(&provider),
            model: default_model(&provider).to_string(),
            provider,
            send_content: config.llm_send_content,
        }
    }

    pub fn enabled(&self) -> bool {
        self.mode != "off"
    }
}

fn default_base_url(provider: &str) -> String {
    match provider {
        "anthropic" => "https://api.anthropic.com".to_string(),
        "groq" => "https://api.groq.com/openai".to_string(),
        "openrouter" => "https://openrouter.ai/api".to_string(),
        "gemini" => "https://generativelanguage.googleapis.com".to_string(),
        "local" => "http://localhost:11434/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    }
}

fn default_model(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-3-5-sonnet-latest",
        "groq" => "llama-3.3-70b-versatile",
        "gemini" => "gemini-2.0-flash",
        "local" => "llama3.1",
        _ => "gpt-4o-mini",
    }
}

#[derive(Debug, Clone)]
pub struct RetrievedNote {
    pub rel_path: String,
    pub title: String,
    pub body: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct PromptPlan {
    pub question: String,
    pub notes: Vec<RetrievedNote>,
    pub system: String,
    pub user: String,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

pub trait Transport {
    fn send(&self, request: &ChatRequest) -> Result<String>;
}

const SYSTEM: &str = "You answer questions about a personal decision ledger. \
Use only the notes provided. Every decision has a reference like DR-0007 with a \
status (proposed, accepted, superseded, rejected) and may supersede an older \
decision. If the notes do not answer the question, say so plainly and name what \
is missing. Quote decision references exactly. Never invent a decision.";

fn truncate_chars(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}

/// Retrieve, assemble, and describe — without sending anything.
pub fn plan(vault: &Vault, question: &str, k: usize) -> Result<PromptPlan> {
    if !vault.config().llm_send_content {
        return Err(DayDocError::Llm(
            "llm_send_content = false: refusing to send note text to a model. Set \
             `daydoc config set llm_send_content true` if you want this."
                .to_string(),
        ));
    }

    let hits = search::search(vault, question, &Filters::default())?;
    let mut notes: Vec<RetrievedNote> = Vec::new();
    let mut budget = MAX_CONTEXT_CHARS;
    for hit in hits.into_iter().take(k.max(1)) {
        if budget == 0 {
            break;
        }
        let note = match vault.load(&hit.rel_path) {
            Ok(note) => note,
            Err(_) => continue,
        };
        let body = truncate_chars(&note.body, budget);
        budget = budget.saturating_sub(body.len());
        notes.push(RetrievedNote {
            rel_path: note.rel_path.clone(),
            title: note.title(),
            body,
            score: hit.score,
        });
    }

    let mut user = String::new();
    user.push_str(&format!("Question: {question}\n\nNotes:\n"));
    if notes.is_empty() {
        user.push_str("(no notes matched)\n");
    }
    for (index, note) in notes.iter().enumerate() {
        user.push_str(&format!(
            "\n--- note {} ({}) ---\n{}\n{}",
            index + 1,
            note.rel_path,
            note.title,
            note.body.trim()
        ));
    }

    let bytes: usize = notes.iter().map(|note| note.body.len()).sum();
    let target = if vault.config().llm_provider.trim().is_empty() {
        "the configured provider".to_string()
    } else {
        vault.config().llm_provider.clone()
    };
    let preview = format!(
        "would send {} note(s), {} characters, to {}{}\n  {}",
        notes.len(),
        bytes,
        target,
        if vault.config().llm_mode == "auto" {
            " (auto-detected local server)"
        } else {
            ""
        },
        notes
            .iter()
            .map(|note| note.rel_path.clone())
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    Ok(PromptPlan {
        question: question.to_string(),
        notes,
        system: SYSTEM.to_string(),
        user,
        preview,
    })
}

/// Build the provider-specific HTTP request. Pure: no key ever enters the body.
pub fn build_request(settings: &LlmSettings, plan: &PromptPlan, api_key: &str) -> Result<ChatRequest> {
    let base = settings.base_url.trim_end_matches('/');
    match settings.provider.as_str() {
        "anthropic" => Ok(ChatRequest {
            url: format!("{base}/v1/messages"),
            headers: vec![
                ("x-api-key".to_string(), api_key.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ],
            body: json!({
                "model": settings.model,
                "max_tokens": 1024,
                "system": plan.system,
                "messages": [{ "role": "user", "content": plan.user }],
            }),
        }),
        "gemini" => Ok(ChatRequest {
            url: format!("{base}/v1beta/models/{}:generateContent", settings.model),
            headers: vec![
                ("x-goog-api-key".to_string(), api_key.to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ],
            body: json!({
                "systemInstruction": { "parts": [{ "text": plan.system }] },
                "contents": [{ "parts": [{ "text": plan.user }] }],
            }),
        }),
        _ => Ok(ChatRequest {
            url: format!("{base}/chat/completions"),
            headers: vec![
                ("authorization".to_string(), format!("Bearer {api_key}")),
                ("content-type".to_string(), "application/json".to_string()),
            ],
            body: json!({
                "model": settings.model,
                "temperature": 0,
                "messages": [
                    { "role": "system", "content": plan.system },
                    { "role": "user", "content": plan.user },
                ],
            }),
        }),
    }
}

/// Read the answer out of a provider response.
pub fn parse_response(provider: &str, value: &Value) -> Result<String> {
    let text = match provider {
        "anthropic" => value
            .get("content")
            .and_then(|content| content.get(0))
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str),
        "gemini" => value
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(Value::as_str),
        _ => value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str),
    };
    text.map(|text| text.trim().to_string())
        .ok_or_else(|| DayDocError::Llm(format!("{provider} response had no text")))
}

/// The key for this vault: the secret store first, then the environment
/// variable that belongs to *this* provider.
///
/// Provider matching matters: an `ANTHROPIC_API_KEY` in the environment must
/// never be sent to `api.openai.com` just because it was found first.
pub fn api_key(vault: &Vault, provider: &str) -> Option<String> {
    if let Some(key) = crate::secret::resolve(vault, crate::secret::LLM) {
        return Some(key);
    }
    api_key_from_env_for(provider)
}

/// The environment variables that can hold a key for `provider`, in order.
pub fn env_names_for(provider: &str) -> &'static [&'static str] {
    match provider {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openai" | "groq" | "openrouter" | "local" => &["OPENAI_API_KEY"],
        _ => &["OPENAI_API_KEY"],
    }
}

pub fn api_key_from_env_for(provider: &str) -> Option<String> {
    if let Ok(value) = std::env::var("DAYDOC_LLM_KEY") {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    for name in env_names_for(provider) {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

pub struct HttpTransport;

impl Transport for HttpTransport {
    fn send(&self, request: &ChatRequest) -> Result<String> {
        let mut builder = ureq::post(&request.url);
        for (name, value) in &request.headers {
            builder = builder.set(name, value);
        }
        let response = builder
            .send_json(request.body.clone())
            .map_err(|err| DayDocError::Llm(format!("request failed: {err}")))?;
        let text = response
            .into_string()
            .map_err(|err| DayDocError::Llm(format!("unreadable response: {err}")))?;
        serde_json::from_str(&text)
            .map_err(|err| DayDocError::Llm(format!("response was not json: {err}")))
    }
}

/// Ask a model, using whatever transport the caller provides.
pub fn ask_with(
    vault: &Vault,
    question: &str,
    k: usize,
    transport: &dyn Transport,
) -> Result<String> {
    let settings = LlmSettings::from_config(vault.config());
    if !settings.enabled() {
        return Err(DayDocError::Llm(
            "LLM is off. `daydoc config set llm_mode byok` (and set DAYDOC_LLM_KEY) \
             or leave it off — every other feature works without it."
                .to_string(),
        ));
    }
    let key = api_key(vault, &settings.provider).ok_or_else(|| {
        DayDocError::Llm(
            "no API key found: `daydoc key set llm` (reads stdin) or DAYDOC_LLM_KEY".to_string(),
        )
    })?;
    let plan = plan(vault, question, k)?;
    let request = build_request(&settings, &plan, &key)?;
    let raw = transport.send(&request)?;
    let value: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|_| Value::String(raw.trim().to_string()));
    if value.is_string() {
        return Ok(value.as_str().unwrap_or_default().to_string());
    }
    parse_response(&settings.provider, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{self, NewDecision};
    use crate::vault::test_support::temp_vault;

    struct FakeTransport {
        answer: String,
    }

    impl Transport for FakeTransport {
        fn send(&self, _request: &ChatRequest) -> Result<String> {
            Ok(self.answer.clone())
        }
    }

    fn vault_with_decisions() -> Vault {
        let mut vault = temp_vault();
        vault.config_mut().llm_mode = "byok".to_string();
        vault.config_mut().llm_provider = "openai".to_string();
        vault.config_mut().llm_send_content = true;
        ledger::create(
            &vault,
            &NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_decision("Okta won on price and admin tooling."),
        )
        .expect("okta");
        ledger::create(
            &vault,
            &NewDecision::new("Move billing to Razorpay")
                .accepted()
                .with_decision("Razorpay handles INR mandates."),
        )
        .expect("billing");
        vault
    }

    #[test]
    fn content_cannot_leave_the_machine_by_default() {
        let mut vault = vault_with_decisions();
        vault.config_mut().llm_send_content = false;
        let err = plan(&vault, "what did we decide about okta", 4).expect_err("refused");
        assert!(err.to_string().contains("llm_send_content"));
    }

    #[test]
    fn the_plan_sends_only_the_top_k_notes() {
        let vault = vault_with_decisions();
        let plan = plan(&vault, "okta", 1).expect("plan");
        assert_eq!(plan.notes.len(), 1);
        assert!(plan.notes[0].title.contains("Okta"));
        assert!(plan.user.contains("Question: okta"));
        assert!(plan.preview.contains("would send 1 note(s)"));
        assert!(!plan.user.contains("Razorpay handles INR mandates"));
    }

    #[test]
    fn openai_requests_put_the_key_in_a_header_not_the_body() {
        let settings = LlmSettings::from_config(&crate::config::Config::default());
        let plan = PromptPlan {
            question: "q".into(),
            notes: Vec::new(),
            system: "s".into(),
            user: "u".into(),
            preview: String::new(),
        };
        let request = build_request(&settings, &plan, "sk-secret").expect("request");
        assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer sk-secret"));
        assert!(!request.body.to_string().contains("sk-secret"));
    }

    #[test]
    fn anthropic_and_gemini_requests_differ_where_they_should() {
        let plan = PromptPlan {
            question: "q".into(),
            notes: Vec::new(),
            system: "sys".into(),
            user: "usr".into(),
            preview: String::new(),
        };
        let mut anthropic = LlmSettings::from_config(&crate::config::Config::default());
        anthropic.provider = "anthropic".to_string();
        anthropic.base_url = default_base_url("anthropic");
        let request = build_request(&anthropic, &plan, "k").expect("request");
        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(request.body["system"], "sys");

        let mut gemini = LlmSettings::from_config(&crate::config::Config::default());
        gemini.provider = "gemini".to_string();
        gemini.base_url = default_base_url("gemini");
        gemini.model = "gemini-2.0-flash".to_string();
        let request = build_request(&gemini, &plan, "k").expect("request");
        assert!(request.url.contains("gemini-2.0-flash:generateContent"));
        assert!(request
            .headers
            .iter()
            .any(|(name, _)| name == "x-goog-api-key"));
    }

    #[test]
    fn responses_are_read_for_each_provider() {
        let openai = json!({ "choices": [{ "message": { "content": "  answer  " } }] });
        assert_eq!(parse_response("openai", &openai).expect("openai"), "answer");

        let anthropic = json!({ "content": [{ "text": "claude says" }] });
        assert_eq!(
            parse_response("anthropic", &anthropic).expect("anthropic"),
            "claude says"
        );

        let gemini = json!({ "candidates": [{ "content": { "parts": [{ "text": "gem says" }] } }] });
        assert_eq!(parse_response("gemini", &gemini).expect("gemini"), "gem says");

        assert!(parse_response("openai", &json!({})).is_err());
    }

    #[test]
    fn asking_with_a_transport_returns_the_answer() {
        let vault = vault_with_decisions();
        std::env::set_var("DAYDOC_LLM_KEY", "test-key");
        let transport = FakeTransport {
            answer: json!({ "choices": [{ "message": { "content": "Okta, DR-0001." } }] })
                .to_string(),
        };
        let answer = ask_with(&vault, "what did we decide about okta", 3, &transport)
            .expect("answer");
        assert_eq!(answer, "Okta, DR-0001.");
    }

    #[test]
    fn an_anthropic_key_is_never_offered_to_openai() {
        assert_eq!(env_names_for("anthropic"), &["ANTHROPIC_API_KEY"]);
        assert_eq!(env_names_for("openai"), &["OPENAI_API_KEY"]);
        assert_eq!(env_names_for("groq"), &["OPENAI_API_KEY"]);
        assert!(!env_names_for("openai").contains(&"ANTHROPIC_API_KEY"));
        assert!(!env_names_for("anthropic").contains(&"OPENAI_API_KEY"));
    }

    #[test]
    fn asking_with_the_llm_off_is_a_clear_refusal() {
        let vault = temp_vault();
        let transport = FakeTransport {
            answer: "{}".to_string(),
        };
        let err = ask_with(&vault, "anything", 3, &transport).expect_err("off");
        assert!(err.to_string().contains("LLM is off"));
    }
}
