//! 七牛等网关要求每条 message 都有 `content`。
//! rig OpenAI Completions 在「只有 tool_calls、没有正文」时会 omit 该字段。

use rig_core::completion::message::AssistantContent;
use rig_core::completion::{
    CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Message,
    ProviderCapabilities,
};
use rig_core::streaming::StreamingCompletionResponse;
use serde_json::{json, Value};

/// 扫 `messages`（或整段 body）里缺 / 为 null 的 `content`，补成 `""`。
pub fn fill_missing_message_content(body: &mut Value) {
    let Some(msgs) = (if body.is_array() {
        body.as_array_mut()
    } else {
        body.get_mut("messages").and_then(|v| v.as_array_mut())
    }) else {
        return;
    };
    for m in msgs {
        let Some(obj) = m.as_object_mut() else { continue };
        if obj.get("content").map_or(true, Value::is_null) {
            obj.insert("content".into(), json!(""));
        }
    }
}

/// 给只有 tool_calls 的 assistant 补一条空正文，序列化时才会带上 `content`。
pub fn ensure_assistant_tool_content(history: &mut [Message]) {
    for msg in history {
        let Message::Assistant { content, .. } = msg else {
            continue;
        };
        let has_tool = content
            .iter()
            .any(|c| matches!(c, AssistantContent::ToolCall(_)));
        let has_text = content.iter().any(|c| matches!(c, AssistantContent::Text(_)));
        if has_tool && !has_text {
            content.insert(0, AssistantContent::text(""));
        }
    }
}

/// Chat 路径：发出去之前补 `content`。
pub struct EnsureMessageContent<M>(pub M);

impl<M: CompletionModel> CompletionModel for EnsureMessageContent<M> {
    fn completion(
        &self,
        mut request: CompletionRequest,
    ) -> impl std::future::Future<Output = Result<CompletionResponse, CompletionError>>
    + rig_core::wasm_compat::WasmCompatSend {
        ensure_assistant_tool_content(&mut request.chat_history);
        self.0.completion(request)
    }

    fn stream(
        &self,
        mut request: CompletionRequest,
    ) -> impl std::future::Future<Output = Result<StreamingCompletionResponse, CompletionError>>
    + rig_core::wasm_compat::WasmCompatSend {
        ensure_assistant_tool_content(&mut request.chat_history);
        self.0.stream(request)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.0.capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fill_adds_content_on_assistant_tool_calls() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "精修第 21 章正文"},
                {"role": "assistant", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "get_chapter_content", "arguments": "{}"}}]}
            ]
        });
        fill_missing_message_content(&mut body);
        assert_eq!(body["messages"][1]["content"], json!(""));
        assert!(body["messages"][1].get("tool_calls").is_some());
    }

    #[test]
    fn fill_replaces_null_content() {
        let mut msgs = json!([{"role": "assistant", "content": null, "tool_calls": []}]);
        fill_missing_message_content(&mut msgs);
        assert_eq!(msgs[0]["content"], json!(""));
    }

    #[test]
    fn ensure_adds_empty_text_beside_tool_call() {
        let mut history = vec![Message::Assistant {
            id: None,
            content: vec![AssistantContent::tool_call(
                "c1",
                "get_chapter_content",
                json!({}),
            )],
        }];
        ensure_assistant_tool_content(&mut history);
        let Message::Assistant { content, .. } = &history[0] else {
            panic!("assistant");
        };
        assert!(matches!(content[0], AssistantContent::Text(_)));
        assert!(matches!(content[1], AssistantContent::ToolCall(_)));
    }

    #[test]
    fn ensure_skips_when_text_already_present() {
        let mut history = vec![Message::Assistant {
            id: None,
            content: vec![
                AssistantContent::text("ok"),
                AssistantContent::tool_call("c1", "ping", json!({})),
            ],
        }];
        ensure_assistant_tool_content(&mut history);
        let Message::Assistant { content, .. } = &history[0] else {
            panic!("assistant");
        };
        assert_eq!(content.len(), 2);
    }
}
