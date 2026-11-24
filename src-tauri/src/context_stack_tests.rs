use super::*;

fn asst(id: &str, ts: &str, ctx: u64, content: &str) -> String {
    let read = ctx - 2;
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","isSidechain":false,"message":{{"id":"{id}","model":"claude-fable-5-1","role":"assistant","content":{content},"usage":{{"input_tokens":2,"cache_read_input_tokens":{read},"cache_creation_input_tokens":0,"output_tokens":5}}}}}}"#
    )
}

fn user(ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{ts}","isSidechain":false,"message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn summary(ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{ts}","isSidechain":false,"isCompactSummary":true,"message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn result(ts: &str, tool_use_id: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{ts}","isSidechain":false,"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":"{text}"}}]}}}}"#
    )
}

fn attach(ts: &str, json: &str) -> String {
    format!(r#"{{"type":"attachment","timestamp":"{ts}","isSidechain":false,"attachment":{json}}}"#)
}

fn plain() -> Vec<String> {
    let a100 = "a".repeat(100);
    let m50 = "m".repeat(50);
    let x400 = "x".repeat(400);
    vec![
        user("2026-11-24T05:00:00.000Z", "hello"),
        attach(
            "2026-11-24T05:00:00.100Z",
            &format!(
                r#"{{"type":"instructions","files":[{{"path":"/p/CLAUDE.md","type":"Project","content":"{a100}"}},{{"path":"/home/u/MEMORY.md","type":"AutoMem","content":"{m50}"}}]}}"#
            ),
        ),
        attach(
            "2026-11-24T05:00:00.200Z",
            r#"{"type":"prompt_snapshot","systemPrompt":["abc"],"tools":[{"name":"Bash"}]}"#,
        ),
        asst(
            "msg_1",
            "2026-11-24T05:00:01.000Z",
            1000,
            r#"[{"type":"text","text":"ok"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]"#,
        ),
        result("2026-11-24T05:00:02.000Z", "t1", &x400),
        asst(
            "msg_2",
            "2026-11-24T05:00:03.000Z",
            1200,
            r#"[{"type":"text","text":"done"}]"#,
        ),
    ]
}

fn kinds(s: &Stack) -> Vec<BlockKind> {
    s.blocks.iter().map(|b| b.kind).collect()
}

#[test]
fn blocks_in_the_order_the_model_sees_them_with_the_harness_first() {
    let text = plain().join("\n");
    let s = build("claude", &text, "s1", Some(Path::new("/p")));
    assert!(s.supported);
    assert_eq!(s.session, "claude:s1");
    assert_eq!(s.model, "claude-fable-5-1");
    assert_eq!(
        kinds(&s),
        vec![
            BlockKind::Harness,
            BlockKind::User,
            BlockKind::Instructions,
            BlockKind::Memory,
            BlockKind::Agent,
            BlockKind::ToolCall,
            BlockKind::ToolResult,
            BlockKind::Agent,
        ]
    );
    let h = &s.blocks[0];
    assert_eq!(h.i, 0);
    assert_eq!(h.label, "Claude's own prompt and tools");
    assert!(
        h.text.starts_with("abc\n"),
        "the snapshot's prompt leads the text"
    );
    assert!(
        h.text.contains("\"name\": \"Bash\""),
        "the tool list follows"
    );
    assert_eq!(h.call, 0, "in the floor");

    let ins = &s.blocks[2];
    assert_eq!(ins.label, "CLAUDE.md", "relative to the project root");
    assert_eq!(ins.on_disk.as_deref(), Some("/p/CLAUDE.md"));
    assert_eq!(ins.bytes, 100);
    assert_eq!(ins.turn, 1);
    let mem = &s.blocks[3];
    assert_eq!(
        mem.label, "/home/u/MEMORY.md",
        "outside the root: the full path"
    );

    let call = &s.blocks[5];
    assert_eq!(call.label, "Bash: ls");
    assert_eq!(
        call.bytes,
        r#"{"command":"ls"}"#.len() as u64,
        "the compact wire form"
    );
    assert!(call.text.starts_with("Bash\n"));
    assert_eq!(call.call, 1, "produced by call 0, first sent on call 1");

    let res = &s.blocks[6];
    assert_eq!(res.label, "Bash result");
    assert_eq!(res.bytes, 400);
    assert!(!res.is_error);
    for (i, b) in s.blocks.iter().enumerate() {
        assert_eq!(b.i, i, "re-indexed over the live stack");
    }
}

#[test]
fn bytes_per_token_is_calibrated_on_the_measured_growth() {
    let text = plain().join("\n");
    let s = build("claude", &text, "s1", Some(Path::new("/p")));
    assert_eq!(s.measured.tokens, 1200);
    assert_eq!(s.measured.floor, 1000);
    assert_eq!(s.measured.base, 1000);
    assert_eq!(s.measured.calls, 2);
    assert_eq!(s.measured.compactions, 0);
    // Appended after the floor reading: "ok" (2) + {"command":"ls"} (16)
    // + 400 + "done" (4) = 422 bytes over 200 tokens of growth.
    assert!(s.calibrated);
    assert!(
        (s.bytes_per_token - 2.11).abs() < 1e-9,
        "{}",
        s.bytes_per_token
    );
    let res = &s.blocks[6];
    assert_eq!(res.tokens, 190, "400 / 2.11, rounded");
    assert!((res.share - 190.0 / 1200.0).abs() < 1e-9);
    let accounted: u64 = s.blocks.iter().map(|b| b.tokens).sum();
    assert_eq!(s.accounted_tokens, accounted);
    assert_eq!(s.unaccounted_tokens, 1200 - accounted as i64);
    let by: Vec<(BlockKind, usize)> = s.by_kind.iter().map(|k| (k.kind, k.blocks)).collect();
    assert_eq!(
        by,
        vec![
            (BlockKind::Harness, 1),
            (BlockKind::Instructions, 1),
            (BlockKind::Memory, 1),
            (BlockKind::User, 1),
            (BlockKind::Agent, 2),
            (BlockKind::ToolCall, 1),
            (BlockKind::ToolResult, 1),
        ]
    );
    assert_eq!(s.compacted.blocks, 0);
}

#[test]
fn a_compaction_keeps_the_summary_and_what_followed_and_counts_the_rest() {
    let mut lines = plain();
    lines.push(
        r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-11-24T05:00:04.000Z","isSidechain":false}"#
            .to_string(),
    );
    lines.push(summary("2026-11-24T05:00:04.100Z", "summary of before"));
    lines.push(asst(
        "msg_3",
        "2026-11-24T05:00:05.000Z",
        500,
        r#"[{"type":"text","text":"a"}]"#,
    ));
    lines.push(user("2026-11-24T05:00:06.000Z", "more"));
    lines.push(asst(
        "msg_4",
        "2026-11-24T05:00:07.000Z",
        600,
        r#"[{"type":"thinking","thinking":"hmm hmm"},{"type":"text","text":"bb"}]"#,
    ));
    let s = build("claude", &lines.join("\n"), "s1", Some(Path::new("/p")));
    assert_eq!(
        kinds(&s),
        vec![
            BlockKind::Harness,
            BlockKind::CompactionSummary,
            BlockKind::Agent,
            BlockKind::User,
            BlockKind::Thinking,
            BlockKind::Agent,
        ]
    );
    assert_eq!(
        s.compacted.blocks, 7,
        "user, two files, agent, call, result, agent"
    );
    assert_eq!(s.compacted.bytes, 5 + 100 + 50 + 2 + 16 + 400 + 4);
    assert_eq!(s.measured.compactions, 1);
    assert_eq!(s.measured.tokens, 600);
    assert_eq!(s.measured.base, 500, "the first reading after the summary");
    // Appended after that reading: "a" (1) + "more" (4) + "bb" (2); the
    // thinking is in the file but not re-sent.
    assert!(s.calibrated);
    assert!(
        (s.bytes_per_token - 0.07).abs() < 1e-9,
        "{}",
        s.bytes_per_token
    );
    let think = &s.blocks[4];
    assert!(!think.resent);
    assert_eq!(think.bytes, 7);
    assert_eq!(think.tokens, 0);
    assert_eq!(s.blocks[1].label, "Compaction summary");
    assert_eq!(
        s.blocks[3].turn, 2,
        "the summary is not a turn of the user's"
    );
}

#[test]
fn without_a_snapshot_or_growth_there_is_no_harness_block_and_the_fallback_ratio() {
    let text = [
        user("2026-11-24T05:00:00.000Z", "hello"),
        asst(
            "msg_1",
            "2026-11-24T05:00:01.000Z",
            300,
            r#"[{"type":"text","text":"ok"}]"#,
        ),
    ]
    .join("\n");
    let s = build("claude", &text, "s1", None);
    assert_eq!(kinds(&s), vec![BlockKind::User, BlockKind::Agent]);
    assert!(!s.calibrated);
    assert_eq!(s.bytes_per_token, FALLBACK_BYTES_PER_TOKEN);
    assert_eq!(s.accounted_tokens, 1 + 1, "5/4 and 2/4, rounded");
    assert_eq!(s.unaccounted_tokens, 298);
}

#[test]
fn a_streamed_message_counts_once_and_a_later_snapshot_replaces_the_earlier() {
    let text = [
        attach(
            "2026-11-24T05:00:00.000Z",
            r#"{"type":"prompt_snapshot","systemPrompt":["first"]}"#,
        ),
        user("2026-11-24T05:00:00.100Z", "hello"),
        asst(
            "msg_1",
            "2026-11-24T05:00:01.000Z",
            100,
            r#"[{"type":"text","text":"ok"}]"#,
        ),
        asst(
            "msg_1",
            "2026-11-24T05:00:01.500Z",
            100,
            r#"[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/x.rs"}}]"#,
        ),
        attach(
            "2026-11-24T05:00:02.000Z",
            r#"{"type":"prompt_snapshot","systemPrompt":["second, longer"]}"#,
        ),
        result("2026-11-24T05:00:02.500Z", "t1", "contents"),
        asst(
            "msg_2",
            "2026-11-24T05:00:03.000Z",
            150,
            r#"[{"type":"text","text":"done"}]"#,
        ),
    ]
    .join("\n");
    let s = build("claude", &text, "s1", Some(Path::new("/p")));
    assert_eq!(s.measured.calls, 2);
    let harness: Vec<&Block> = s
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Harness)
        .collect();
    assert_eq!(harness.len(), 1, "one live harness block");
    assert!(harness[0].text.starts_with("second, longer"));
    assert_eq!(
        s.blocks[0].kind,
        BlockKind::Harness,
        "first, wherever the snapshot fell"
    );
    let call = s
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::ToolCall)
        .unwrap();
    assert_eq!(call.label, "Read: /p/x.rs");
    let res = s
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::ToolResult)
        .unwrap();
    assert_eq!(
        res.label, "Read result",
        "named through the streamed tool_use"
    );
}

#[test]
fn attachments_become_labelled_blocks() {
    let text = [
        user("2026-11-24T05:00:00.000Z", "hello"),
        attach(
            "2026-11-24T05:00:00.100Z",
            r#"{"type":"file","filename":"/Users/u/.cache/bram/paste/1.png","content":"iVBOR"}"#,
        ),
        attach(
            "2026-11-24T05:00:00.200Z",
            r#"{"type":"hook_success","hookName":"guard","stdout":"ok\n","stderr":""}"#,
        ),
        attach(
            "2026-11-24T05:00:00.300Z",
            r#"{"type":"total_tokens_reminder","text":"<total_tokens>1</total_tokens>"}"#,
        ),
        attach(
            "2026-11-24T05:00:00.400Z",
            r#"{"type":"deferred_tools_delta","addedLines":["a","b"]}"#,
        ),
        attach(
            "2026-11-24T05:00:00.500Z",
            r#"{"type":"credential_org","organizationUuid":"x"}"#,
        ),
        asst(
            "msg_1",
            "2026-11-24T05:00:01.000Z",
            300,
            r#"[{"type":"text","text":"ok"}]"#,
        ),
    ]
    .join("\n");
    let s = build("claude", &text, "s1", None);
    let labels: Vec<&str> = s
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Attachment)
        .map(|b| b.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec![
            "Pasted image",
            "Hook output: guard",
            "Reminder",
            "Tool list change",
            "Credential org",
        ]
    );
    let img = &s.blocks[1];
    assert_eq!(
        img.on_disk.as_deref(),
        Some("/Users/u/.cache/bram/paste/1.png")
    );
    assert_eq!(img.images, 1);
    assert_eq!(img.bytes, 0, "base64 is not text");
    let delta = &s.blocks[4];
    assert_eq!(delta.text, "a\nb");
}

#[test]
fn labels_are_first_lines_cut_to_eighty_characters() {
    assert_eq!(first_line("  two\nlines "), "two");
    let long = "y".repeat(100);
    let cut = first_line(&long);
    assert_eq!(cut.chars().count(), 81);
    assert!(cut.ends_with('…'));
    assert_eq!(
        tool_call_label(
            "Bash",
            &serde_json::json!({"command": "ls -la", "description": "List files"})
        ),
        "Bash: List files",
        "the description beats the command"
    );
    assert_eq!(tool_call_label("Glob", &serde_json::json!({})), "Glob");
    assert_eq!(humanize("remote_session_change"), "Remote session change");
}

#[test]
fn a_provider_without_a_reader_gets_the_unsupported_stack_whatever_the_file_holds() {
    let text = plain().join("\n");
    for provider in ["codex", "opencode", "antigravity", "someday"] {
        assert_eq!(reader_for(provider), None, "{provider}");
        let s = build(provider, &text, "s1", None);
        assert!(!s.supported, "{provider}");
        assert_eq!(s.provider, provider);
        assert!(s.blocks.is_empty());
        assert_eq!(s.measured.calls, 0);
    }
    assert_eq!(reader_for("claude"), Some(Reader::ClaudeJsonl));
}

#[test]
fn empty_answers_for_other_providers_and_no_session() {
    let s = empty("codex", false);
    assert!(!s.supported);
    assert_eq!(s.provider, "codex");
    assert!(s.blocks.is_empty());
    let body = serde_json::to_value(&s).unwrap();
    assert_eq!(body["bytesPerToken"], 4.0);
    assert!(body.get("blocks").is_some());
    assert!(body["measured"]["tokens"].is_number());
}

#[test]
fn the_body_omits_block_text_and_names_fields_in_camel_case() {
    let text = plain().join("\n");
    let s = build("claude", &text, "s1", Some(Path::new("/p")));
    let body = serde_json::to_value(&s).unwrap();
    let b = &body["blocks"][6];
    assert!(
        b.get("text").is_none(),
        "text is the block route's, not the stack's"
    );
    assert_eq!(b["kind"], "tool-result");
    assert_eq!(b["onDisk"], serde_json::Value::Null);
    assert!(b["isError"].is_boolean());
    assert!(body["bytesPerToken"].is_number());
    assert!(body["unaccountedTokens"].is_number());
    assert_eq!(body["byKind"][0]["kind"], "harness");
}

/// A probe, not a test: prints a real session file's stack summary, the way
/// the reference doc's baseline was taken. Skips when the file is not named.
///
/// BRAM_STACK_FILE=<session.jsonl> BRAM_STACK_ROOT=<project root> \
///   cargo test --manifest-path src-tauri/Cargo.toml --lib \
///   context_stack::tests::probe_a_real_session_file -- --ignored --nocapture
#[test]
#[ignore]
fn probe_a_real_session_file() {
    let Ok(path) = std::env::var("BRAM_STACK_FILE") else {
        return;
    };
    let root = std::env::var("BRAM_STACK_ROOT").ok();
    let text = std::fs::read_to_string(&path).expect("readable session file");
    let s = build("claude", &text, "probe", root.as_deref().map(Path::new));
    println!(
        "blocks {} · measured {} · floor {} · base {} · calls {} · compactions {} · bytes/token {:.2} text, {:.2} opaque ({}, fit rows {}) · accounted {} · unaccounted {}",
        s.blocks.len(),
        s.measured.tokens,
        s.measured.floor,
        s.measured.base,
        s.measured.calls,
        s.measured.compactions,
        s.bytes_per_token,
        s.opaque_bytes_per_token,
        if s.calibrated { "measured" } else { "fallback" },
        s.wire.as_ref().map(|w| w.fit_rows).unwrap_or(0),
        s.accounted_tokens,
        s.unaccounted_tokens
    );
    println!(
        "compacted: {} blocks, {} bytes",
        s.compacted.blocks, s.compacted.bytes
    );
    for k in &s.by_kind {
        println!(
            "  {:<20} blocks {:>4}  bytes {:>9}  tokens {:>7}  share {:.3}",
            format!("{:?}", k.kind),
            k.blocks,
            k.bytes,
            k.tokens,
            k.share
        );
    }
    let mut top: Vec<&Block> = s.blocks.iter().collect();
    top.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    println!("largest blocks:");
    for b in top.iter().take(12) {
        println!(
            "  {:>7} tokens  {:<18} {}",
            b.tokens,
            format!("{:?}", b.kind),
            b.label
        );
    }
}

// --- the requests strategy ---------------------------------------------------

fn shape_record(hash: &str) -> String {
    format!(
        r#"{{"type":"api-request-shape","shapeHash":"{hash}","timestamp":"2026-11-24T05:00:00.000Z","shape":{{"system":[{{"type":"text","text":"You are Claude Code."}},{{"type":"text","text":"Be brief."}}],"tools":[{{"name":"Bash","input_schema":{{}}}},{{"name":"Read","input_schema":{{}}}}]}}}}"#
    )
}

fn blob_record(message: &serde_json::Value) -> (String, String) {
    let hash = message_hash(message);
    (
        hash.clone(),
        format!(
            r#"{{"type":"api-request-blob","hash":"{hash}","sessionId":"s1","message":{}}}"#,
            serde_json::to_string(message).unwrap()
        ),
    )
}

fn request_record(id: &str, base: Option<&str>, keep: usize, tail: &[String], ts: &str) -> String {
    let base = match base {
        Some(b) => format!("\"{b}\""),
        None => "null".to_string(),
    };
    format!(
        r#"{{"type":"api-request","id":"{id}","base":{base},"keep":{keep},"tail":{},"shapeHash":"sh1","timestamp":"{ts}","querySource":"repl_main_thread"}}"#,
        serde_json::to_string(tail).unwrap()
    )
}

fn user_record_json(ts: &str, content: &serde_json::Value) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{ts}","isSidechain":false,"message":{{"role":"user","content":{}}}}}"#,
        serde_json::to_string(content).unwrap()
    )
}

fn asst_record_json(id: &str, ts: &str, ctx: u64, content: &serde_json::Value) -> String {
    let read = ctx - 2;
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","isSidechain":false,"message":{{"id":"{id}","model":"claude-fable-5-1","role":"assistant","content":{},"usage":{{"input_tokens":2,"cache_read_input_tokens":{read},"cache_creation_input_tokens":0,"output_tokens":5}}}}}}"#,
        serde_json::to_string(content).unwrap()
    )
}

struct WireFixture {
    lines: Vec<String>,
    asst1_hash: String,
    merged_hash: String,
}

/// Two requests: the first carries a harness reminder (the CLAUDE.md
/// contents) and the user's hello; the second keeps both and appends the
/// assistant's turn and its two tool results merged into one message.
fn wire_fixture() -> WireFixture {
    use serde_json::json;
    let reminder = json!({"role": "system", "content": [{"type": "text", "text": "<system-reminder>\nCodebase instructions below.\n\nContents of /p/CLAUDE.md (project instructions):\n\n# Bram\nRules.\n\nContents of /home/u/.claude/memory/MEMORY.md (auto-memory):\n\n- a memory\n</system-reminder>"}]});
    let (reminder_hash, reminder_line) = blob_record(&reminder);
    let hello = json!("hello");
    let hello_hash = message_hash(&json!({"role": "user", "content": hello}));
    let asst1 = json!([
        {"type": "text", "text": "ok"},
        {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}},
        {"type": "tool_use", "id": "t2", "name": "Read", "input": {"file_path": "/p/x.rs"}}
    ]);
    let asst1_hash = message_hash(&json!({"role": "assistant", "content": asst1}));
    let tr1 = json!({"type": "tool_result", "tool_use_id": "t1", "content": "x".repeat(40), "is_error": false});
    let tr2 = json!({"type": "tool_result", "tool_use_id": "t2", "content": "y".repeat(20)});
    let merged_hash = message_hash(&json!({"role": "user", "content": [tr1, tr2]}));
    let lines = vec![
        shape_record("sh1"),
        user_record_json("2026-11-24T05:00:00.000Z", &hello),
        reminder_line,
        request_record(
            "r1",
            None,
            0,
            &[reminder_hash, hello_hash],
            "2026-11-24T05:00:00.500Z",
        ),
        asst_record_json("msg_1", "2026-11-24T05:00:01.000Z", 1000, &asst1),
        user_record_json("2026-11-24T05:00:02.000Z", &json!([tr1])),
        user_record_json("2026-11-24T05:00:02.100Z", &json!([tr2])),
        request_record(
            "r2",
            Some("r1"),
            2,
            &[asst1_hash.clone(), merged_hash.clone()],
            "2026-11-24T05:00:02.500Z",
        ),
        asst_record_json(
            "msg_2",
            "2026-11-24T05:00:03.000Z",
            1500,
            &json!([{"type": "text", "text": "done"}]),
        ),
    ];
    WireFixture {
        lines,
        asst1_hash,
        merged_hash,
    }
}

#[test]
fn the_request_chain_gives_the_exact_live_list_with_the_shape_first() {
    let fx = wire_fixture();
    let s = build("claude", &fx.lines.join("\n"), "s1", Some(Path::new("/p")));
    assert_eq!(s.reader, Strategy::Requests);
    let w = s.wire.as_ref().expect("wire summary");
    assert_eq!(w.requests, 2);
    assert_eq!(
        w.live_messages, 4,
        "reminder, hello, assistant, merged results"
    );
    assert_eq!(w.unresolved, 0);
    assert!(w.shape_known);
    assert_eq!(w.dropped_messages, 0);
    assert_eq!(
        kinds(&s),
        vec![
            BlockKind::Harness,
            BlockKind::Harness,
            BlockKind::Instructions,
            BlockKind::Memory,
            BlockKind::User,
            BlockKind::Agent,
            BlockKind::ToolCall,
            BlockKind::ToolCall,
            BlockKind::ToolResult,
            BlockKind::ToolResult,
        ]
    );
    assert_eq!(s.blocks[0].label, "Claude Code's system prompt");
    assert!(s.blocks[0].text.contains("Be brief."));
    assert_eq!(s.blocks[1].label, "Tool definitions (2)");
    let ins = &s.blocks[2];
    assert_eq!(ins.label, "CLAUDE.md");
    assert_eq!(ins.on_disk.as_deref(), Some("/p/CLAUDE.md"));
    assert!(
        ins.text.starts_with("<system-reminder>"),
        "the preamble rides with the first file"
    );
    assert!(ins.text.contains("# Bram"));
    let mem = &s.blocks[3];
    assert_eq!(
        mem.on_disk.as_deref(),
        Some("/home/u/.claude/memory/MEMORY.md")
    );
    assert!(mem.text.contains("- a memory"));
    assert_eq!(s.blocks[4].label, "hello");
    assert_eq!(s.blocks[4].turn, 1);
    assert_eq!(s.blocks[4].call, 0, "sent on the first request");
    assert_eq!(
        s.blocks[5].call, 1,
        "the assistant's turn joined on the second"
    );
    assert_eq!(s.blocks[8].label, "Bash result");
    assert_eq!(s.blocks[8].bytes, 40);
    assert_eq!(
        s.blocks[9].label, "Read result",
        "named through the merged message"
    );
    assert_eq!(s.measured.tokens, 1500);
    assert_eq!(s.measured.floor, 1000);
    assert_eq!(s.measured.calls, 2);
    // The ratio is the live request's bytes over its measured tokens, so the
    // blocks total the measured figure up to rounding.
    assert!(s.calibrated);
    let live_bytes: u64 = s.blocks.iter().map(|b| b.bytes).sum();
    assert_eq!(w.live_bytes, live_bytes);
    assert!((s.bytes_per_token - live_bytes as f64 / 1500.0).abs() < 1e-9);
    assert!(
        s.unaccounted_tokens.abs() <= s.blocks.len() as i64,
        "rounding only"
    );
    assert!(w.first_request_bytes < live_bytes);
    assert_eq!(fx.asst1_hash.len(), 64);
}

#[test]
fn messages_a_later_request_drops_are_counted_and_a_summary_is_named() {
    let fx = wire_fixture();
    let mut lines = fx.lines.clone();
    let summary = serde_json::json!("This session is being continued from a previous conversation that ran out of context. Summary: all of it.");
    let summary_hash = message_hash(&serde_json::json!({"role": "user", "content": summary}));
    lines.push(user_record_json("2026-11-24T05:00:04.000Z", &summary));
    lines.push(request_record(
        "r3",
        Some("r2"),
        2,
        &[summary_hash],
        "2026-11-24T05:00:04.500Z",
    ));
    lines.push(asst_record_json(
        "msg_3",
        "2026-11-24T05:00:05.000Z",
        400,
        &serde_json::json!([{"type": "text", "text": "going on"}]),
    ));
    let s = build("claude", &lines.join("\n"), "s1", Some(Path::new("/p")));
    let w = s.wire.as_ref().unwrap();
    assert_eq!(w.requests, 3);
    assert_eq!(w.live_messages, 3, "reminder, hello, summary");
    assert_eq!(
        w.dropped_messages, 2,
        "the assistant turn and the merged results"
    );
    assert_eq!(s.compacted.blocks, 5, "agent, two calls, two results");
    assert_eq!(s.compacted.bytes, 2 + 16 + 23 + 40 + 20);
    assert_eq!(
        kinds(&s),
        vec![
            BlockKind::Harness,
            BlockKind::Harness,
            BlockKind::Instructions,
            BlockKind::Memory,
            BlockKind::User,
            BlockKind::CompactionSummary,
        ]
    );
    assert_eq!(s.blocks[5].label, "Compaction summary");
    assert_eq!(s.measured.tokens, 400);
    assert_eq!(fx.merged_hash.len(), 64);
}

#[test]
fn an_unresolved_hash_is_counted_and_shown_not_dropped() {
    let fx = wire_fixture();
    let mut lines = fx.lines.clone();
    lines.push(request_record(
        "r3",
        Some("r2"),
        4,
        &["0".repeat(64)],
        "2026-11-24T05:00:04.500Z",
    ));
    lines.push(asst_record_json(
        "msg_3",
        "2026-11-24T05:00:05.000Z",
        1600,
        &serde_json::json!([{"type": "text", "text": "x"}]),
    ));
    let s = build("claude", &lines.join("\n"), "s1", None);
    let w = s.wire.as_ref().unwrap();
    assert_eq!(w.unresolved, 1);
    assert_eq!(s.blocks.last().unwrap().label, "Unresolved message");
    assert_eq!(s.blocks.last().unwrap().bytes, 0);
}

#[test]
fn images_cost_a_fixed_estimate_not_their_base64() {
    use serde_json::json;
    let hello = json!("look");
    let hello_hash = message_hash(&json!({"role": "user", "content": hello}));
    let asst1 = json!([{"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/p/a.png"}}]);
    let asst1_hash = message_hash(&json!({"role": "assistant", "content": asst1}));
    let tr = json!([{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "A".repeat(50000)}}]}]);
    let tr_hash = message_hash(&json!({"role": "user", "content": tr}));
    let lines = vec![
        shape_record("sh1"),
        user_record_json("2026-11-24T05:00:00.000Z", &hello),
        request_record("r1", None, 0, &[hello_hash], "2026-11-24T05:00:00.500Z"),
        asst_record_json("msg_1", "2026-11-24T05:00:01.000Z", 1000, &asst1),
        user_record_json("2026-11-24T05:00:02.000Z", &tr),
        request_record(
            "r2",
            Some("r1"),
            1,
            &[asst1_hash, tr_hash],
            "2026-11-24T05:00:02.500Z",
        ),
        asst_record_json(
            "msg_2",
            "2026-11-24T05:00:03.000Z",
            2700,
            &json!([{"type": "text", "text": "seen"}]),
        ),
    ];
    let s = build("claude", &lines.join("\n"), "s1", None);
    let res = s
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::ToolResult)
        .unwrap();
    assert_eq!(res.images, 1);
    assert_eq!(res.bytes, 0, "base64 is not text");
    assert_eq!(res.label, "Read result (+1 image)");
    assert_eq!(res.tokens, IMAGE_TOKENS);
    let w = s.wire.as_ref().unwrap();
    assert_eq!(w.images, 1);
    // The ratio leaves the image's tokens out of the pool it divides.
    assert!((s.bytes_per_token - w.live_bytes as f64 / (2700 - IMAGE_TOKENS) as f64).abs() < 1e-9);
}

#[test]
fn a_file_without_request_records_falls_back_to_the_records_strategy() {
    let text = plain().join("\n");
    let s = build("claude", &text, "s1", Some(Path::new("/p")));
    assert_eq!(s.reader, Strategy::Records);
    assert!(s.wire.is_none());
}

#[test]
fn the_hash_is_the_sha256_of_canonical_json() {
    // Python: hashlib.sha256(json.dumps({"role":"user","content":"hello"}, sort_keys=True, separators=(",",":")).encode()).hexdigest()
    let h = message_hash(&serde_json::json!({"role": "user", "content": "hello"}));
    assert_eq!(
        h,
        "f4f7e767b9a1966921d93f1818bb0238c1633ed715f29a789b4e3a624ab16512"
    );
    // Key order must not matter, and nesting is canonicalized too.
    let a = message_hash(
        &serde_json::json!({"role": "user", "content": [{"type": "text", "text": "x"}]}),
    );
    let b = message_hash(
        &serde_json::json!({"content": [{"text": "x", "type": "text"}], "role": "user"}),
    );
    assert_eq!(a, b);
    assert_ne!(
        a,
        message_hash(&serde_json::json!({"role": "user", "content": "x"}))
    );
}

#[test]
fn fit_rates_recovers_two_rates_and_refuses_thin_or_implausible_data() {
    // delta = text/2.5 + opaque/3.75, exactly, over eight rows.
    let rows: Vec<(u64, u64, u64)> = (1..=8u64)
        .map(|i| {
            let text = 1000 * i;
            let opaque = 300 * (9 - i);
            (
                (text as f64 / 2.5 + opaque as f64 / 3.75).round() as u64,
                text,
                opaque,
            )
        })
        .collect();
    let (t, o) = fit_rates(&rows).expect("a fit");
    assert!((t - 2.5).abs() < 0.05, "{t}");
    assert!((o - 3.75).abs() < 0.1, "{o}");
    assert_eq!(fit_rates(&rows[..5]), Err("too-few-rows"));
    let no_opaque: Vec<(u64, u64, u64)> = rows.iter().map(|&(d, t, _)| (d, t, 0)).collect();
    assert_eq!(fit_rates(&no_opaque), Err("no-opaque-bytes"));
    let absurd: Vec<(u64, u64, u64)> = rows.iter().map(|&(_, t, o)| (1, t, o)).collect();
    assert!(
        fit_rates(&absurd).is_err(),
        "a rate outside 1 to 12 bytes a token"
    );
}

#[test]
fn the_thinking_signature_is_the_opaque_part_of_its_block() {
    use serde_json::json;
    let hello = json!("hi");
    let hello_hash = message_hash(&json!({"role": "user", "content": hello}));
    let asst1 = json!([
        {"type": "thinking", "thinking": "", "signature": "S".repeat(1200)},
        {"type": "text", "text": "ok"}
    ]);
    let asst1_hash = message_hash(&json!({"role": "assistant", "content": asst1}));
    let more = json!("more");
    let more_hash = message_hash(&json!({"role": "user", "content": more}));
    let lines = vec![
        shape_record("sh1"),
        user_record_json("2026-11-24T05:00:00.000Z", &hello),
        request_record("r1", None, 0, &[hello_hash], "2026-11-24T05:00:00.500Z"),
        asst_record_json("msg_1", "2026-11-24T05:00:01.000Z", 1000, &asst1),
        user_record_json("2026-11-24T05:00:02.000Z", &more),
        request_record(
            "r2",
            Some("r1"),
            1,
            &[asst1_hash, more_hash],
            "2026-11-24T05:00:02.500Z",
        ),
        asst_record_json(
            "msg_2",
            "2026-11-24T05:00:03.000Z",
            1400,
            &json!([{"type": "text", "text": "seen"}]),
        ),
    ];
    let s = build("claude", &lines.join("\n"), "s1", None);
    let think = s
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Thinking)
        .unwrap();
    assert_eq!(think.label, "Thinking (encrypted)");
    assert_eq!(think.bytes, 1200);
    assert_eq!(think.opaque_bytes, 1200);
    assert!(think.resent, "in the live request, so re-sent");
    assert!(think.tokens > 0);
    let w = s.wire.as_ref().unwrap();
    assert_eq!(w.fit_rows, 1);
    assert_eq!(
        w.fit, "too-few-rows",
        "one usable row is no fit; one rate for both"
    );
    assert_eq!(s.opaque_bytes_per_token, s.bytes_per_token);
    let body = serde_json::to_value(&s).unwrap();
    assert!(body["opaqueBytesPerToken"].is_number());
    assert_eq!(
        body["blocks"][2]["opaqueBytes"], 0,
        "the system prompt has none"
    );
}

/// Eight answered turns whose readings grow by text/2.5 + signature/3.75,
/// then a ninth request still waiting for its response, as a live file
/// looks mid-turn.
#[test]
fn the_fit_runs_on_a_live_file_whose_last_request_has_no_response_yet() {
    use serde_json::json;
    let mut lines = vec![shape_record("sh1")];
    let hello = json!("go");
    let hello_hash = message_hash(&json!({"role": "user", "content": hello}));
    lines.push(user_record_json("2026-11-24T05:00:00.000Z", &hello));
    lines.push(request_record(
        "r0",
        None,
        0,
        &[hello_hash],
        "2026-11-24T05:00:00.500Z",
    ));
    let mut ctx: u64 = 5000;
    let mut prev_id = "r0".to_string();
    let mut total = 1usize;
    for i in 1..=8u64 {
        let ts = format!("2026-11-24T05:00:{:02}.000Z", i * 3);
        let text = "t".repeat((500 * i) as usize);
        let sig = "S".repeat((2000 * (9 - i)) as usize);
        let asst = json!([
            {"type": "thinking", "thinking": "", "signature": sig},
            {"type": "text", "text": text}
        ]);
        let asst_hash = message_hash(&json!({"role": "assistant", "content": asst}));
        lines.push(asst_record_json(&format!("msg_{i}"), &ts, ctx, &asst));
        let user = json!(format!("next {i}"));
        let user_hash = message_hash(&json!({"role": "user", "content": user}));
        lines.push(user_record_json(&ts, &user));
        let rid = format!("r{i}");
        lines.push(request_record(
            &rid,
            Some(&prev_id),
            total,
            &[asst_hash, user_hash],
            &ts,
        ));
        // The reading this request will get: the appended bytes at the two rates.
        let appended_text = (500 * i) as f64 + format!("next {i}").len() as f64 + 2.0;
        ctx += (appended_text / 2.5 + (2000 * (9 - i)) as f64 / 3.75).round() as u64;
        prev_id = rid;
        total += 2;
    }
    // Eight responses answered requests r0..r7; r8 is still waiting.
    let s = build("claude", &lines.join("\n"), "s1", None);
    let w = s.wire.as_ref().unwrap();
    assert_eq!(w.requests, 9);
    assert_eq!(s.measured.calls, 8);
    assert_eq!(w.fit_rows, 7, "r1..r7 answered and grown");
    assert_eq!(w.fit, "ok");
    assert!(
        s.opaque_bytes_per_token > s.bytes_per_token,
        "signatures priced dearer than text"
    );
    assert!(
        (s.opaque_bytes_per_token / s.bytes_per_token - 1.5).abs() < 0.1,
        "{} / {}",
        s.opaque_bytes_per_token,
        s.bytes_per_token
    );
}
