use crate::memory::types::ChatMessage;

pub fn call_chat_api_sync(
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[ChatMessage],
) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    #[derive(serde::Serialize)]
    struct RequestBody {
        model: String,
        messages: Vec<serde_json::Value>,
    }

    let chat_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let request_body = RequestBody {
        model: model.to_string(),
        messages: chat_messages,
    };

    let url = format!("{}/chat/completions", base_url);
    let body_str = serde_json::to_string(&request_body).unwrap();

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .body(body_str)
        .send()
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }

    let body_str = response.text().map_err(|e| e.to_string())?;

    #[derive(serde::Deserialize)]
    struct ApiResponse {
        choices: Vec<Choice>,
    }

    #[derive(serde::Deserialize)]
    struct Choice {
        message: Message,
    }

    #[derive(serde::Deserialize)]
    struct Message {
        content: String,
    }

    let api_response: ApiResponse = serde_json::from_str(&body_str).map_err(|e| e.to_string())?;

    Ok(api_response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default())
}
