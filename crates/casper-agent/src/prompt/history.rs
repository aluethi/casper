//! Conversation history loading for prompt assembly.
//!
//! Loads messages from a conversation, keeping the most recent turns
//! within a token budget. Tool use / tool result pairs are never split.

use sqlx::PgPool;
use uuid::Uuid;

use super::types::{HistoryMessage, estimate_tokens_json};

/// A raw message row from the database.
#[derive(Debug, sqlx::FromRow)]
struct MessageRow {
    role: String,
    content: serde_json::Value,
    token_count: Option<i32>,
}

/// Load conversation history for prompt assembly.
///
/// Returns messages in chronological order (oldest first), selecting from the
/// most recent messages that fit within `budget_tokens`.
///
/// **Pair integrity:** `tool_use` (role=assistant with tool_use blocks) and
/// `tool_result` messages are always kept as pairs. If including a tool_result
/// would exceed the budget, both the tool_result and its preceding tool_use
/// are excluded.
pub async fn load_conversation_history(
    pool: &PgPool,
    tenant_id: Uuid,
    conversation_id: Uuid,
    budget_tokens: i32,
) -> Result<Vec<HistoryMessage>, String> {
    // Fetch all messages ordered by created_at DESC (newest first).
    // We process newest-first, then reverse at the end for chronological order.
    let rows: Vec<MessageRow> = sqlx::query_as(
        "SELECT role, content, token_count
         FROM messages
         WHERE tenant_id = $1 AND conversation_id = $2
         ORDER BY created_at DESC",
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error loading history: {e}"))?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // Group messages into "turns" that must be kept or dropped together.
    // A turn is either:
    //   - A single user/system message
    //   - An assistant message followed by its tool_result messages (a tool-call group)
    let turns = group_into_turns(rows);

    // Select turns newest-first within budget
    let mut selected: Vec<Turn> = Vec::new();
    let mut used_tokens = 0;

    for turn in turns {
        let turn_tokens = turn.total_tokens();
        if used_tokens + turn_tokens > budget_tokens {
            break;
        }
        used_tokens += turn_tokens;
        selected.push(turn);
    }

    // Reverse to chronological order and flatten
    selected.reverse();

    let messages: Vec<HistoryMessage> = selected
        .into_iter()
        .flat_map(|turn| turn.messages)
        .collect();

    Ok(messages)
}

/// A "turn" groups messages that must be kept or dropped together.
#[derive(Debug)]
struct Turn {
    messages: Vec<HistoryMessage>,
}

impl Turn {
    fn total_tokens(&self) -> i32 {
        self.messages.iter().map(|m| m.token_count).sum()
    }
}

/// Returns true if a content value contains tool_use blocks.
fn has_tool_use(content: &serde_json::Value) -> bool {
    if let Some(arr) = content.as_array() {
        arr.iter()
            .any(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
    } else {
        false
    }
}

/// Group messages into turns. Input is in reverse chronological order (newest first).
///
/// Processing newest-first: when we encounter a tool_result, we accumulate it
/// along with subsequent tool_results, then attach them to the next assistant
/// message that contains tool_use blocks. This forms a single turn.
///
/// Regular messages (user, system, assistant without tool_use) are each their own turn.
fn group_into_turns(rows: Vec<MessageRow>) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    let mut pending_tool_results: Vec<HistoryMessage> = Vec::new();

    for row in rows {
        let token_count = row
            .token_count
            .unwrap_or_else(|| estimate_tokens_json(&row.content));

        let msg = HistoryMessage {
            role: row.role.clone(),
            content: row.content,
            token_count,
        };

        if row.role == "tool" {
            // Accumulate tool results (we're going newest-first,
            // so tool_results come before their assistant message)
            pending_tool_results.push(msg);
        } else if row.role == "assistant" && has_tool_use(&msg.content) {
            // This assistant message has tool_use blocks -- pair it with
            // any pending tool_results to form one turn.
            // In the final output, the order should be: assistant, then tool_results.
            // Since we're processing newest-first, the tool_results we accumulated
            // came *after* this assistant message chronologically.
            let mut turn_messages = vec![msg];
            // tool_results are in reverse chronological order, reverse them
            pending_tool_results.reverse();
            turn_messages.append(&mut pending_tool_results);
            turns.push(Turn {
                messages: turn_messages,
            });
        } else {
            // If there are orphaned tool_results (shouldn't happen normally),
            // flush them as their own turn to avoid losing data.
            if !pending_tool_results.is_empty() {
                pending_tool_results.reverse();
                turns.push(Turn {
                    messages: std::mem::take(&mut pending_tool_results),
                });
            }
            turns.push(Turn {
                messages: vec![msg],
            });
        }
    }

    // Flush any remaining tool_results
    if !pending_tool_results.is_empty() {
        pending_tool_results.reverse();
        turns.push(Turn {
            messages: pending_tool_results,
        });
    }

    turns
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_msg(role: &str, content: serde_json::Value, tokens: i32) -> MessageRow {
        MessageRow {
            role: role.to_string(),
            content,
            token_count: Some(tokens),
        }
    }

    #[test]
    fn group_simple_messages() {
        // Newest first: msg3, msg2, msg1
        let rows = vec![
            make_msg("assistant", json!("Response 2"), 10),
            make_msg("user", json!("Question 2"), 5),
            make_msg("assistant", json!("Response 1"), 10),
            make_msg("user", json!("Question 1"), 5),
        ];

        let turns = group_into_turns(rows);
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].total_tokens(), 10);
        assert_eq!(turns[1].total_tokens(), 5);
    }

    #[test]
    fn group_tool_use_pairs() {
        // Newest first: tool_result, assistant(tool_use), user
        let rows = vec![
            make_msg(
                "tool",
                json!({"type": "tool_result", "content": "result"}),
                20,
            ),
            make_msg(
                "assistant",
                json!([{"type": "tool_use", "name": "search", "input": {}}]),
                15,
            ),
            make_msg("user", json!("Search for something"), 10),
        ];

        let turns = group_into_turns(rows);
        assert_eq!(turns.len(), 2); // (assistant+tool_result) + user
        assert_eq!(turns[0].total_tokens(), 35); // 15 + 20
        assert_eq!(turns[0].messages.len(), 2);
        assert_eq!(turns[0].messages[0].role, "assistant");
        assert_eq!(turns[0].messages[1].role, "tool");
        assert_eq!(turns[1].total_tokens(), 10);
    }

    #[test]
    fn group_multiple_tool_results() {
        // assistant called two tools -> two tool_result messages
        // Newest first: tool_result_2, tool_result_1, assistant(tool_use), user
        let rows = vec![
            make_msg("tool", json!({"content": "result2"}), 10),
            make_msg("tool", json!({"content": "result1"}), 10),
            make_msg(
                "assistant",
                json!([
                    {"type": "tool_use", "name": "tool1", "input": {}},
                    {"type": "tool_use", "name": "tool2", "input": {}}
                ]),
                20,
            ),
            make_msg("user", json!("Do two things"), 5),
        ];

        let turns = group_into_turns(rows);
        assert_eq!(turns.len(), 2);
        // The tool-use turn should have 3 messages: assistant + 2 tool results
        assert_eq!(turns[0].messages.len(), 3);
        assert_eq!(turns[0].total_tokens(), 40); // 20 + 10 + 10
    }

    #[test]
    fn budget_truncation_keeps_pairs_intact() {
        // 20 messages, budget for only some.
        // Newest first: tool_result(20 tokens), assistant_tool_use(15), user(5), assistant(10), user(5)
        let rows = vec![
            make_msg("tool", json!({"content": "result"}), 20),
            make_msg(
                "assistant",
                json!([{"type": "tool_use", "name": "t", "input": {}}]),
                15,
            ),
            make_msg("user", json!("question 2"), 5),
            make_msg("assistant", json!("answer 1"), 10),
            make_msg("user", json!("question 1"), 5),
        ];

        let turns = group_into_turns(rows);
        // turns: [assistant+tool(35), user(5), assistant(10), user(5)]

        // Budget = 45: should fit the tool turn (35) + user (5) + maybe not more
        let budget = 45;
        let mut selected: Vec<Turn> = Vec::new();
        let mut used = 0;
        for turn in turns {
            let t = turn.total_tokens();
            if used + t > budget {
                break;
            }
            used += t;
            selected.push(turn);
        }

        assert_eq!(selected.len(), 2); // tool turn (35) + user (5)
        assert_eq!(used, 40);
    }

    #[test]
    fn budget_too_small_for_tool_pair_skips_it() {
        // Budget is 10, but the tool pair costs 35
        let rows = vec![
            make_msg("tool", json!({"content": "result"}), 20),
            make_msg(
                "assistant",
                json!([{"type": "tool_use", "name": "t", "input": {}}]),
                15,
            ),
            make_msg("user", json!("question"), 5),
        ];

        let turns = group_into_turns(rows);
        let budget = 10;
        let mut selected: Vec<Turn> = Vec::new();
        let mut used = 0;
        for turn in turns {
            let t = turn.total_tokens();
            if used + t > budget {
                break;
            }
            used += t;
            selected.push(turn);
        }

        // The first turn (35 tokens) exceeds budget, so nothing is selected
        assert_eq!(selected.len(), 0);
    }

    #[test]
    fn twenty_messages_budget_for_ten() {
        // Create 20 messages: alternating user(5 tokens) and assistant(5 tokens)
        // Total = 100 tokens. Budget = 50 => should get the most recent 10 messages.
        let mut rows = Vec::new();
        for i in (0..20).rev() {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            rows.push(make_msg(role, json!(format!("message {i}")), 5));
        }

        let turns = group_into_turns(rows);
        assert_eq!(turns.len(), 20);

        let budget = 50;
        let mut selected: Vec<Turn> = Vec::new();
        let mut used = 0;
        for turn in turns {
            let t = turn.total_tokens();
            if used + t > budget {
                break;
            }
            used += t;
            selected.push(turn);
        }

        assert_eq!(selected.len(), 10);
        assert_eq!(used, 50);

        // Reverse to chronological
        selected.reverse();
        let messages: Vec<HistoryMessage> = selected.into_iter().flat_map(|t| t.messages).collect();

        assert_eq!(messages.len(), 10);
        // The most recent messages should be included (messages 10-19 in the original 0-19 sequence).
        // In newest-first ordering, index 0 was msg 19, index 1 was msg 18, etc.
        // We selected the first 10 (newest 10): msgs 19,18,17,...,10
        // After reversing: 10,11,12,...,19
    }
}
