//! Intent Agent - LLM-based intent classification with streaming

use std::sync::mpsc;

use crate::agents::types::RoutingDecision;
use crate::memory::types::ChatMessage;
use crate::services::api::call_chat_api_stream;

#[derive(Debug, Clone)]
pub enum IntentState {
    Idle,
    Understanding,
    Completed(RoutingDecision),
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum IntentEvent {
    Thinking(String),
    Decision(RoutingDecision),
    Error(String),
}

pub struct IntentAgent;

impl IntentAgent {
    pub fn new() -> Self {
        Self
    }

    pub async fn classify(
        message: String,
        base_url: String,
        api_key: String,
        model: String,
        sender: mpsc::Sender<IntentEvent>,
    ) {
        let prompt = format!(
            r#"你是一个意图分类助手，负责分析用户消息的意图并决定路由到哪个处理器。

用户消息: "{}"

请先流式地思考你的分类理由，然后返回JSON格式的分类结果。

分类选项：
- system: 涉及系统信息查询（进程、内存、磁盘空间、文件操作、打开应用等）
- coding: 涉及代码开发、编程、写代码、创建页面、做UI、做前端组件、做登录页、做界面等任何软件开发相关任务
- general: 普通对话、闲聊、知识问答等

请按以下JSON格式返回（只返回JSON，不要有其他文字）：
{{"intent": "system|coding|general", "task": "用户的原始问题", "reasoning": "分类理由"}}"#,
            message
        );

        let system_msg = ChatMessage::new(
            "system",
            "你是一个意图分类助手，负责分析用户意图并决定路由。",
        );

        let user_msg = ChatMessage::new(
            "user",
            &prompt,
        );

        let messages = vec![system_msg, user_msg];
        let sender_for_stream = sender.clone();

        let result = call_chat_api_stream(&base_url, &api_key, &model, &messages, None, move |delta| {
            let trimmed = delta.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('{') {
                let _ = sender_for_stream.send(IntentEvent::Thinking(trimmed.to_string()));
            }
        })
        .await;

        match result {
            Ok(response_val) => {
                let full_text = response_val["content"].as_str().unwrap_or_default();
                if let Some(json_start) = full_text.find('{') {
                    let json_str = &full_text[json_start..];
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let intent = parsed["intent"].as_str().unwrap_or("general");
                        let task = parsed["task"].as_str().unwrap_or(&message);

                        let decision = match intent {
                            "system" => RoutingDecision::SystemTools {
                                task: task.to_string(),
                            },
                            "coding" => RoutingDecision::ClaudeCode {
                                instruction: task.to_string(),
                                session_id: None,
                            },
                            _ => RoutingDecision::GeneralAI {
                                messages: vec![ChatMessage::new("user", task)],
                            },
                        };

                        let _ = sender.send(IntentEvent::Decision(decision));
                        return;
                    }
                }
                let _ = sender.send(IntentEvent::Decision(RoutingDecision::GeneralAI {
                    messages: vec![ChatMessage::new("user", &message)],
                }));
            }
            Err(error) => {
                let _ = sender.send(IntentEvent::Error(error));
                let _ = sender.send(IntentEvent::Decision(RoutingDecision::GeneralAI {
                    messages: vec![ChatMessage::new("user", &message)],
                }));
            }
        }
    }
}
