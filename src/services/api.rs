use crate::memory::types::ChatMessage;
use futures::StreamExt;

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

pub async fn call_chat_api_stream<F>(
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[ChatMessage],
    mut on_delta: F,
) -> Result<String, String>
where
    F: FnMut(String) + Send,
{
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    #[derive(serde::Serialize)]
    struct RequestBody {
        model: String,
        messages: Vec<serde_json::Value>,
        stream: bool,
    }

    let chat_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let request_body = RequestBody {
        model: model.to_string(),
        messages: chat_messages,
        stream: true,
    };

    let url = format!("{}/chat/completions", base_url);
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }

    let mut full_text = String::new();
    let mut raw_accum = String::new();
    let mut pending = String::new();
    let mut saw_stream_data = false;

    let mut stream = response.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| e.to_string())?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        raw_accum.push_str(&chunk_str);
        pending.push_str(&chunk_str);

        while let Some(newline) = pending.find('\n') {
            let mut line = pending[..newline].to_string();
            pending.drain(..newline + 1);
            if line.ends_with('\r') {
                line.pop();
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if !line.starts_with("data:") {
                continue;
            }

            saw_stream_data = true;
            let data = line.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                return Ok(full_text);
            }

            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };

            let delta = value
                .pointer("/choices/0/delta/content")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    value
                        .pointer("/choices/0/message/content")
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("");

            if !delta.is_empty() {
                full_text.push_str(delta);
                on_delta(delta.to_string());
            }
        }
    }

    if !pending.trim().is_empty() {
        raw_accum.push_str(&pending);
    }

    if !saw_stream_data {
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

        if let Ok(api_response) = serde_json::from_str::<ApiResponse>(&raw_accum) {
            let content = api_response
                .choices
                .first()
                .map(|c| c.message.content.clone())
                .unwrap_or_default();
            if !content.is_empty() {
                on_delta(content.clone());
            }
            return Ok(content);
        }
    }

    Ok(full_text)
}

pub async fn summarize_conversation_async(
    base_url: &str,
    api_key: &str,
    model: &str,
    conversation: &[ChatMessage],
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    #[derive(serde::Serialize)]
    struct RequestBody {
        model: String,
        messages: Vec<serde_json::Value>,
    }

    let summary_prompt = format!(
        "请用10个字以内总结以下对话内容，只返回总结文字，不要其他内容：\n{}\n总结：",
        conversation
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let chat_messages = vec![serde_json::json!({
        "role": "user",
        "content": summary_prompt
    })];

    let request_body = RequestBody {
        model: model.to_string(),
        messages: chat_messages,
    };

    let url = format!("{}/chat/completions", base_url);
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }

    let body_str = response.text().await.map_err(|e| e.to_string())?;

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
