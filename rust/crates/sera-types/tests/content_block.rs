use sera_types::content_block::*;
use sera_types::runtime::TokenUsage;

#[test]
fn content_block_type_tag_in_json() {
    let block = ContentBlock::Text { text: "hi".into() };
    let json = serde_json::to_string(&block).unwrap();
    assert!(
        json.contains("\"type\":\"text\""),
        "missing type tag: {json}"
    );
    assert!(
        json.contains("\"text\":\"hi\""),
        "missing text field: {json}"
    );
}

#[test]
fn conversation_message_cause_by_roundtrip() {
    let msg = ConversationMessage {
        role: ConversationRole::User,
        content: vec![ContentBlock::Text {
            text: "hello".into(),
        }],
        usage: None,
        cause_by: Some(ActionId::from("act-1")),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: ConversationMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.cause_by.as_ref().map(|a| a.0.as_str()),
        Some("act-1")
    );
}

#[test]
fn conversation_message_tool_use_tool_result_pairing() {
    let msg = ConversationMessage {
        role: ConversationRole::Assistant,
        content: vec![
            ContentBlock::ToolUse {
                id: "call-1".into(),
                name: "memory_read".into(),
                input: serde_json::json!({"path": "notes.md"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "call-1".into(),
                content: "# Notes".into(),
                is_error: false,
            },
        ],
        usage: Some(TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        cause_by: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: ConversationMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.len(), 2);
    assert!(
        matches!(&parsed.content[0], ContentBlock::ToolUse { name, .. } if name == "memory_read")
    );
    assert!(
        matches!(&parsed.content[1], ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call-1")
    );
}
