//! Background subagents still running when a Claude Code turn ends, read from its transcript.
//!
//! Claude Code fires `Stop` at the end of every turn, even while subagents it launched in the
//! background keep working; it resumes on its own when they report. Background shell commands
//! and monitors do not hold a session: they are as often a dev server as a wait.

use std::io::BufRead;

/// The summary Claude Code gives a subagent that stopped while its own background work runs.
const WAITING_ON_OWN_WORK: &str = "waiting on its own background work";

/// The background subagents still running after the transcript's last turn, or `None` while
/// that turn has not closed yet (Claude Code writes its `turn_duration` after the Stop hooks).
///
/// A turn's `pendingBackgroundAgentCount` still counts a subagent whose report started that
/// turn, and the subagent's final notice may land after it without starting another, so
/// notices after the last turn count down from it.
///
/// The count leaves out a subagent that stopped while its own background work runs, so its
/// notice holds the turn until it reports again. A subagent stopped by hand never does, so a
/// prompt the user types drops those notices.
pub fn pending_background_subagents(transcript: impl BufRead) -> Option<usize> {
    let mut turn_closed = false;
    let mut alive_at_turn_end = 0;
    let mut finished_since_turn_end = 0;
    let mut subagents = std::collections::BTreeSet::new();
    let mut waiting_on_own_work = std::collections::BTreeSet::new();
    for line in transcript.lines().map_while(Result::ok) {
        let relevant = line.contains("\"assistant\"")
            || line.contains("turn_duration")
            || line.contains("async_launched")
            || line.contains("task-notification")
            || line.contains("\"human\"");
        if !relevant {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match record.get("type").and_then(|kind| kind.as_str()) {
            Some("assistant") => turn_closed = false,
            Some("system")
                if record.get("subtype").and_then(|s| s.as_str()) == Some("turn_duration") =>
            {
                turn_closed = true;
                finished_since_turn_end = 0;
                alive_at_turn_end = record
                    .get("pendingBackgroundAgentCount")
                    .and_then(|count| count.as_u64())
                    .unwrap_or(0) as usize;
            }
            Some("user") if is_task_notification(&record) => {
                let text = message_text(&record);
                let Some(task_id) = tag(&text, "task-id") else {
                    continue;
                };
                if !subagents.contains(task_id) {
                    continue;
                }
                if tag(&text, "summary")
                    .is_some_and(|summary| summary.contains(WAITING_ON_OWN_WORK))
                {
                    waiting_on_own_work.insert(task_id.to_string());
                } else {
                    waiting_on_own_work.remove(task_id);
                    if turn_closed {
                        finished_since_turn_end += 1;
                    }
                }
            }
            Some("user") if is_typed_by_the_user(&record) => waiting_on_own_work.clear(),
            Some("user") => {
                if let Some(agent_id) = launched_subagent(&record) {
                    subagents.insert(agent_id.to_string());
                }
            }
            _ => {}
        }
    }
    turn_closed.then(|| {
        alive_at_turn_end.saturating_sub(finished_since_turn_end) + waiting_on_own_work.len()
    })
}

/// The id of a subagent a tool result launched in the background.
fn launched_subagent(record: &serde_json::Value) -> Option<&str> {
    let result = record.get("toolUseResult")?;
    let launched = result.get("isAsync").and_then(|v| v.as_bool()) == Some(true)
        && result.get("status").and_then(|v| v.as_str()) == Some("async_launched");
    launched
        .then(|| result.get("agentId").and_then(|id| id.as_str()))
        .flatten()
}

fn is_typed_by_the_user(record: &serde_json::Value) -> bool {
    record
        .pointer("/origin/kind")
        .and_then(|kind| kind.as_str())
        == Some("human")
}

fn is_task_notification(record: &serde_json::Value) -> bool {
    record
        .pointer("/origin/kind")
        .and_then(|kind| kind.as_str())
        == Some("task-notification")
}

fn message_text(record: &serde_json::Value) -> String {
    match record.pointer("/message/content") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
            .collect(),
        _ => String::new(),
    }
}

fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("<{name}>"))? + name.len() + 2;
    let end = start + text[start..].find(&format!("</{name}>"))?;
    Some(text[start..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pending(records: &[serde_json::Value]) -> Option<usize> {
        pending_background_subagents(std::io::Cursor::new(
            records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>(),
        ))
    }

    fn assistant() -> serde_json::Value {
        json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "started" }] } })
    }

    fn turn_end(pending: Option<u64>) -> serde_json::Value {
        let mut record = json!({ "type": "system", "subtype": "turn_duration", "durationMs": 10 });
        if let Some(count) = pending {
            record["pendingBackgroundAgentCount"] = json!(count);
        }
        record
    }

    fn launch(agent_id: &str) -> serde_json::Value {
        json!({
            "type": "user",
            "toolUseResult": { "isAsync": true, "status": "async_launched", "agentId": agent_id },
        })
    }

    fn notification(task_id: &str, summary: &str) -> serde_json::Value {
        json!({
            "type": "user",
            "origin": { "kind": "task-notification" },
            "message": { "content": format!(
                "<task-notification>\n<task-id>{task_id}</task-id>\n<status>completed</status>\n<summary>{summary}</summary>\n</task-notification>"
            ) },
        })
    }

    const WAITING: &str = "Agent \"Sleep 90s then reply\" is waiting on its own background work";
    const FINISHED: &str = "Agent \"Sleep 90s then reply\" finished";

    #[test]
    fn a_turn_still_closing_is_unknown() {
        assert_eq!(pending(&[]), None);
        assert_eq!(
            pending(&[turn_end(Some(1)), assistant()]),
            None,
            "the last turn's count is stale once the next turn starts"
        );
    }

    #[test]
    fn claude_codes_count_of_live_background_agents_holds_the_turn() {
        assert_eq!(pending(&[assistant(), turn_end(Some(2))]), Some(2));
        assert_eq!(pending(&[assistant(), turn_end(None)]), Some(0));
    }

    /// The sequence of a real session: a background subagent stops while its own background
    /// command runs, then hands back, then sends its final notice without starting a turn.
    #[test]
    fn a_subagent_holds_until_its_final_notice_even_one_that_starts_no_turn() {
        let mut records = vec![launch("a361"), assistant(), turn_end(Some(1))];
        assert_eq!(pending(&records), Some(1));

        records.extend([notification("a361", WAITING), assistant(), turn_end(None)]);
        assert_eq!(pending(&records), Some(1), "waiting on its own work");

        records.extend([assistant(), turn_end(Some(1))]);
        assert_eq!(
            pending(&records),
            Some(2),
            "its hand-back turn still counts it"
        );

        records.push(notification("a361", FINISHED));
        assert_eq!(
            pending(&records),
            Some(0),
            "the final notice after the turn ends it"
        );
    }

    fn typed_prompt() -> serde_json::Value {
        json!({ "type": "user", "origin": { "kind": "human" }, "message": { "content": "next" } })
    }

    /// A subagent stopped by hand while it waited on its own work never reports again.
    #[test]
    fn a_prompt_the_user_types_drops_waiting_notices_that_never_resolved() {
        let mut records = vec![
            launch("a1"),
            assistant(),
            turn_end(Some(1)),
            notification("a1", WAITING),
            assistant(),
            turn_end(None),
        ];
        assert_eq!(pending(&records), Some(1));
        records.extend([typed_prompt(), assistant(), turn_end(None)]);
        assert_eq!(pending(&records), Some(0));
    }

    #[test]
    fn background_commands_and_unknown_tasks_never_hold() {
        assert_eq!(
            pending(&[
                launch("a1"),
                assistant(),
                turn_end(None),
                notification(
                    "btymzer5b",
                    "Background command \"Sleep\" completed (exit code 0)"
                ),
                notification("other", WAITING),
            ]),
            Some(0)
        );
    }

    #[test]
    fn notification_content_may_be_text_blocks_and_noise_is_skipped() {
        let mut waiting = notification("a1", WAITING);
        let text = waiting["message"]["content"].as_str().unwrap().to_string();
        waiting["message"]["content"] = json!([{ "type": "text", "text": text }]);
        let input = format!(
            "not json\n{}\n{}\n{}\n{}\n",
            launch("a1"),
            waiting,
            assistant(),
            turn_end(None)
        );
        assert_eq!(
            pending_background_subagents(std::io::Cursor::new(input)),
            Some(1)
        );
    }
}
