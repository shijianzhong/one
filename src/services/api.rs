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
    tools: Option<&[serde_json::Value]>,
    mut on_delta: F,
) -> Result<serde_json::Value, String>
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
        #[serde(skip_serializing_if = "Option::is_none")]
        tools: Option<Vec<serde_json::Value>>,
        stream: bool,
    }

    let chat_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let mut val = serde_json::json!({ "role": m.role, "content": m.content });
            if let Some(tool_calls) = &m.tool_calls {
                val["tool_calls"] = serde_json::to_value(tool_calls).unwrap();
            }
            if let Some(tool_call_id) = &m.tool_call_id {
                val["tool_call_id"] = serde_json::json!(tool_call_id);
            }
            val
        })
        .collect();

    let request_body = RequestBody {
        model: model.to_string(),
        messages: chat_messages,
        tools: tools.map(|t| t.to_vec()),
        stream: true,
    };

    let url = format!("{}/chat/completions", base_url);
    eprintln!("\n========== LLM REQUEST ==========");
    eprintln!("Model: {}", model);
    eprintln!("Messages ({}):", request_body.messages.len());
    for (i, msg) in request_body.messages.iter().enumerate() {
        let role = msg["role"].as_str().unwrap_or("");
        let content_str = msg["content"].as_str().unwrap_or("");
        let content_preview = content_str;
        let has_tools = msg.get("tool_calls").is_some();
        eprintln!(
            "  [{}] role={} content={}{}",
            i,
            role,
            content_preview,
            if has_tools { " [has tool_calls]" } else { "" }
        );
    }
    eprintln!(
        "Tools: {}",
        tools
            .map(|t| serde_json::to_string(t).unwrap_or_default())
            .unwrap_or_else(|| "none".to_string())
    );
    eprintln!("================================\n");
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        return Err(format!("API error: {} - {}", status, err_body));
    }

    let mut full_text = String::new();
    let mut tool_calls_map: std::collections::HashMap<i32, serde_json::Value> =
        std::collections::HashMap::new();

    let mut stream = response.bytes_stream();
    let mut pending = String::new();

    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| e.to_string())?;
        let chunk_str = String::from_utf8_lossy(&chunk);
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

            let data = line.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                break;
            }

            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };

            if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
                if let Some(choice) = choices.get(0) {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                            full_text.push_str(content);
                            on_delta(content.to_string());
                        }

                        if let Some(tool_calls) =
                            delta.get("tool_calls").and_then(|tc| tc.as_array())
                        {
                            for tc in tool_calls {
                                let index =
                                    tc.get("index").and_then(|i| i.as_i64()).unwrap_or(0) as i32;
                                let entry =
                                    tool_calls_map.entry(index).or_insert(serde_json::json!({
                                        "id": "",
                                        "type": "function",
                                        "function": { "name": "", "arguments": "" }
                                    }));

                                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                    entry["id"] = serde_json::json!(id);
                                }
                                if let Some(func) = tc.get("function") {
                                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                                        entry["function"]["name"] = serde_json::json!(name);
                                    }
                                    if let Some(args) =
                                        func.get("arguments").and_then(|v| v.as_str())
                                    {
                                        let current_args =
                                            entry["function"]["arguments"].as_str().unwrap_or("");
                                        entry["function"]["arguments"] =
                                            serde_json::json!(format!("{}{}", current_args, args));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !tool_calls_map.is_empty() {
        let tool_calls: Vec<serde_json::Value> =
            tool_calls_map.into_iter().map(|(_, v)| v).collect();
        eprintln!("\n========== LLM RESPONSE (with tool calls) ==========");
        let preview_len = std::cmp::min(500, full_text.len());
        let preview_end = (0..=preview_len)
            .rev()
            .find(|&i| full_text.is_char_boundary(i))
            .unwrap_or(0);
        eprintln!("Content (first 500): {}", &full_text[..preview_end]);
        eprintln!(
            "Tool calls: {}",
            serde_json::to_string_pretty(&tool_calls).unwrap_or_default()
        );
        eprintln!("===================================================\n");
        return Ok(serde_json::json!({
            "role": "assistant",
            "content": full_text,
            "tool_calls": tool_calls
        }));
    }

    eprintln!("\n========== LLM RESPONSE ==========");
    let preview_len = std::cmp::min(1000, full_text.len());
    let preview_end = (0..=preview_len)
        .rev()
        .find(|&i| full_text.is_char_boundary(i))
        .unwrap_or(0);
    eprintln!("{}", &full_text[..preview_end]);
    eprintln!("==================================\n");

    Ok(serde_json::json!({
        "role": "assistant",
        "content": full_text
    }))
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
