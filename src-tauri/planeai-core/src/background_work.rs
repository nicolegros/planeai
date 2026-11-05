//! Background subagents still running when a Claude Code turn ends, read from its transcript.
//!
//! Claude Code fires `Stop` at the end of every turn, even while subagents it launched in the
//! background keep working; it resumes on its own when they report. Background shell commands
//! and monitors do not hold a session: they are as often a dev server as a wait.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A cheap filter before parsing: every record the parser reads contains one of these.
const RELEVANT: [&str; 6] = [
    "\"assistant\"",
    "turn_duration",
    "async_launched",
    "task-notification",
    "\"human\"",
    "\"handback\"",
];

/// Rereads a transcript only when it changed since the last read: a turn held by background
/// subagents is rechecked every few seconds and a long session's transcript runs to megabytes.
#[derive(Default)]
pub struct TranscriptCache {
    read: HashMap<PathBuf, (SystemTime, u64, Option<usize>)>,
}

impl TranscriptCache {
    /// [`pending_background_subagents`] for the transcript at `path`; `None` when unreadable.
    pub fn pending_background_subagents(&mut self, path: &Path) -> Option<usize> {
        let metadata = std::fs::metadata(path).ok()?;
        // Transcripts only grow, so their length changes even when the clock is coarse.
        let version = (metadata.modified().ok()?, metadata.len());
        if let Some(&(modified, len, pending)) = self.read.get(path) {
            if (modified, len) == version {
                return pending;
            }
        }
        let file = std::fs::File::open(path).ok()?;
        let pending = pending_background_subagents(std::io::BufReader::new(file));
        self.read
            .insert(path.to_path_buf(), (version.0, version.1, pending));
        pending
    }

    pub fn forget(&mut self, path: &Path) {
        self.read.remove(path);
    }
}

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
/// notice holds the turn until it hands back or reports again, and its final notice does not
/// count down. A subagent stopped by hand never reports, so a prompt the user types drops those
/// notices.
pub fn pending_background_subagents(transcript: impl BufRead) -> Option<usize> {
    let mut turn_closed = false;
    let mut alive_at_turn_end = 0;
    let mut finished_since_turn_end = 0;
    let mut subagents = std::collections::BTreeSet::new();
    let mut waiting_on_own_work = std::collections::BTreeSet::new();
    let mut waiting_at_turn_end = std::collections::BTreeSet::new();
    for line in transcript.lines().map_while(Result::ok) {
        if !RELEVANT.iter().any(|needle| line.contains(needle)) {
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
                waiting_at_turn_end = waiting_on_own_work.clone();
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
                    if turn_closed && !waiting_at_turn_end.contains(task_id) {
                        finished_since_turn_end += 1;
                    }
                }
            }
            Some("user") if is_typed_by_the_user(&record) => waiting_on_own_work.clear(),
            // A hand-back means the subagent resumed, so Claude Code counts it again.
            Some("user")
                if record.pointer("/origin/handback") == Some(&serde_json::json!(true)) =>
            {
                if let Some(agent_id) = record.pointer("/origin/from").and_then(|id| id.as_str()) {
                    waiting_on_own_work.remove(agent_id);
                }
            }
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

        records.extend([hand_back("a361"), assistant(), turn_end(Some(1))]);
        assert_eq!(
            pending(&records),
            Some(1),
            "its hand-back turn counts it once"
        );

        records.push(notification("a361", FINISHED));
        assert_eq!(
            pending(&records),
            Some(0),
            "the final notice after the turn ends it"
        );
    }

    fn hand_back(agent_id: &str) -> serde_json::Value {
        json!({
            "type": "user",
            "isMeta": true,
            "origin": { "kind": "peer", "from": agent_id, "handback": true },
            "message": { "content": "Another Claude session sent a message" },
        })
    }

    /// Claude Code leaves a waiting subagent out of its count, so its final notice must not
    /// count down another subagent that still runs.
    #[test]
    fn a_waiting_subagents_final_notice_leaves_the_others_counted() {
        let records = [
            launch("a"),
            launch("b"),
            assistant(),
            turn_end(Some(2)),
            notification("a", WAITING),
            assistant(),
            turn_end(Some(1)),
            notification("a", FINISHED),
        ];
        assert_eq!(pending(&records), Some(1), "b still runs");
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
    fn every_record_the_parser_reads_passes_the_filter() {
        for record in [
            assistant(),
            turn_end(None),
            launch("a1"),
            notification("a1", WAITING),
            typed_prompt(),
            hand_back("a1"),
        ] {
            let line = record.to_string();
            assert!(
                RELEVANT.iter().any(|needle| line.contains(needle)),
                "{line}"
            );
        }
    }

    #[test]
    fn the_cache_rereads_a_transcript_only_once_it_grew() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let lines = |records: &[serde_json::Value]| {
            records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>()
        };
        let mut cache = TranscriptCache::default();
        assert_eq!(
            cache.pending_background_subagents(&path),
            None,
            "missing file"
        );

        std::fs::write(
            &path,
            lines(&[launch("a1"), assistant(), turn_end(Some(1))]),
        )
        .unwrap();
        assert_eq!(cache.pending_background_subagents(&path), Some(1));

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, lines(&[notification("a1", FINISHED)]).as_bytes())
            .unwrap();
        assert_eq!(
            cache.pending_background_subagents(&path),
            Some(0),
            "an append within the same clock tick still counts"
        );
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
