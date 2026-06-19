use anyhow::Result;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::{AgentResponse, AgentRunContext, AgentTrait, ToolCall};
use crate::agents::core::orchestrator::OrchestratorEvent;
use crate::agents::core::tool_dispatcher::ToolDispatcher;
use crate::memory::types::ChatMessage;

pub struct AgentRuntime {
    agent: Arc<dyn AgentTrait>,
    tool_dispatcher: ToolDispatcher,
}

impl AgentRuntime {
    pub fn new(agent: Arc<dyn AgentTrait>, tool_dispatcher: ToolDispatcher) -> Self {
        Self {
            agent,
            tool_dispatcher,
        }
    }

    pub async fn run<F>(&self, mut context: AgentRunContext, mut on_event: F) -> Result<String>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let mut max_steps = 15;
        while max_steps > 0 {
            max_steps -= 1;

            if context
                .cancel_flag
                .as_ref()
                .map(|f| f.load(Ordering::SeqCst))
                .unwrap_or(false)
            {
                return Ok("任务已被用户取消。".to_string());
            }

            let response = self.step_once(&context, &mut on_event).await?;

            match response {
                AgentResponse::Answer(answer) => {
                    context.add_message(ChatMessage {
                        role: "assistant".to_string(),
                        content: answer.clone(),
                        tool_calls: None,
                        tool_call_id: None,
                    });

                    if let Some(ref mut input_rx) = context.user_input_rx {
                        on_event(OrchestratorEvent::AwaitingUserInput {
                            reply: answer.clone(),
                        });
                        match input_rx.recv().await {
                            Some(user_msg) => {
                                context.add_message(ChatMessage::new("user", &user_msg));
                                continue;
                            }
                            None => {
                                return Ok(String::new());
                            }
                        }
                    } else {
                        on_event(OrchestratorEvent::StepFinished {
                            result: answer.clone(),
                        });
                        return Ok(answer);
                    }
                }
                AgentResponse::ToolCalls(calls, thinking) => {
                    if !thinking.is_empty() {
                        on_event(OrchestratorEvent::Plan { plan: thinking });
                    }
                    self.execute_tool_calls(&mut context, &calls, &mut on_event)
                        .await?;
                }
            }
        }

        Ok("Reached maximum execution steps.".to_string())
    }

    async fn step_once<F>(
        &self,
        context: &AgentRunContext,
        on_event: &mut F,
    ) -> Result<AgentResponse>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let mut context_inner = AgentRunContext::new(context.session_id.clone());
        context_inner.history = context.history.clone();
        context_inner.metadata = context.metadata.clone();
        context_inner.cancel_flag = context.cancel_flag.clone();
        self.refresh_agent_tools(&mut context_inner);

        let mut step_fut = Box::pin(self.agent.step_stream(
            &mut context_inner,
            Box::new(move |delta| {
                let _ = delta_tx.send(delta);
            }),
        ));
        let response = loop {
            tokio::select! {
                res = &mut step_fut => break res,
                Some(delta) = delta_rx.recv() => {
                    on_event(OrchestratorEvent::AssistantDelta(delta));
                }
            }
        };
        while let Ok(delta) = delta_rx.try_recv() {
            on_event(OrchestratorEvent::AssistantDelta(delta));
        }
        response
    }

    async fn execute_tool_calls<F>(
        &self,
        context: &mut AgentRunContext,
        calls: &[ToolCall],
        on_event: &mut F,
    ) -> Result<()>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let tool_calls_json: Vec<Value> = calls
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })
            })
            .collect();

        context.add_message(ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some(tool_calls_json),
            tool_call_id: None,
        });

        for call in calls {
            on_event(OrchestratorEvent::ToolCall {
                name: call.name.clone(),
                args: call.arguments.clone(),
            });

            let result = self.tool_dispatcher.dispatch(call, on_event).await;

            on_event(OrchestratorEvent::ToolResult {
                name: call.name.clone(),
                result: result.clone(),
            });

            context.add_message(ChatMessage {
                role: "tool".to_string(),
                content: result,
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
            });
        }

        Ok(())
    }

    fn refresh_agent_tools(&self, context: &mut AgentRunContext) {
        let filter = self.agent.tool_filter();
        if let Ok(registry) = crate::agents::core::tool_registry::tool_registry().lock() {
            context.tool_sources = registry.tool_sources(filter.as_deref());
            context.tool_definitions = registry.tool_definitions(filter.as_deref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct AnswerAgent {
        answer: String,
    }

    #[async_trait]
    impl AgentTrait for AnswerAgent {
        fn id(&self) -> &str {
            "answer_agent"
        }

        fn name(&self) -> &str {
            "Answer Agent"
        }

        fn soul_prompt(&self) -> &str {
            ""
        }

        fn model(&self) -> &str {
            "test"
        }

        fn api_base(&self) -> &str {
            ""
        }

        fn api_key(&self) -> &str {
            ""
        }

        async fn step_stream(
            &self,
            _context: &mut AgentRunContext,
            on_delta: Box<dyn FnMut(String) + Send>,
        ) -> Result<AgentResponse> {
            let mut on_delta = on_delta;
            on_delta("hello".to_string());
            Ok(AgentResponse::Answer(self.answer.clone()))
        }
    }

    #[tokio::test]
    async fn runtime_returns_answer_and_emits_delta() {
        let runtime = AgentRuntime::new(
            Arc::new(AnswerAgent {
                answer: "done".to_string(),
            }),
            ToolDispatcher::new(None),
        );
        let mut context = AgentRunContext::new("session".to_string());
        context.add_message(ChatMessage::new("user", "go"));
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_callback = events.clone();

        let result = runtime
            .run(context, move |event| {
                events_for_callback.lock().unwrap().push(event);
            })
            .await
            .unwrap();

        assert_eq!(result, "done");
        let events = events.lock().unwrap();
        assert!(matches!(
            events.as_slice(),
            [
                OrchestratorEvent::AssistantDelta(delta),
                OrchestratorEvent::StepFinished { result }
            ] if delta == "hello" && result == "done"
        ));
    }
}
