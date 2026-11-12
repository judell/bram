use super::*;

fn assistant(id: &str, ts: &str, input: u64, read: u64, create: u64, out: u64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","isSidechain":false,"message":{{"id":"{id}","model":"claude-opus-5-5","role":"assistant","content":[{{"type":"text","text":"x"}}],"usage":{{"input_tokens":{input},"cache_read_input_tokens":{read},"cache_creation_input_tokens":{create},"output_tokens":{out},"server_tool_use":{{"web_search_requests":0}}}}}}}}"#
    )
}

fn t(iso: &str) -> i64 {
    crate::parse_iso_to_epoch_ms(iso).unwrap()
}

#[test]
fn claude_streamed_record_counts_once_with_the_last_usage() {
    let text = [
        assistant("msg_1", "2026-11-01T10:00:00.000Z", 2, 100, 50, 10),
        // the same message streamed again, one record per content block
        assistant("msg_1", "2026-11-01T10:00:01.000Z", 2, 100, 50, 430),
        assistant("msg_2", "2026-11-01T10:00:09.000Z", 3, 200, 0, 7),
        r#"{"type":"assistant","timestamp":"2026-11-01T10:00:10.000Z","message":{"id":"msg_3","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0},"content":[]}}"#
            .to_string(),
    ]
    .join("\n");
    let pf = parse_claude_text(&text, "s1", Thread::Main, None);
    assert_eq!(
        pf.calls.len(),
        2,
        "synthetic skipped, streamed counted once"
    );
    let c = &pf.calls[0];
    assert_eq!(c.output, 430, "the last record's usage");
    assert_eq!(
        c.at_ms,
        t("2026-11-01T10:00:00.000Z"),
        "the first record's time"
    );
    assert_eq!(c.context, 2 + 100 + 50);
    assert_eq!(c.input, 2);
    assert_eq!(pf.calls[1].context, 203);
    assert_eq!(pf.unreadable, 0);
}

#[test]
fn claude_subagent_file_keeps_its_agent_and_is_not_main() {
    let text = assistant("msg_a", "2026-11-01T10:00:00.000Z", 1, 10, 0, 5);
    let pf = parse_claude_text(&text, "s1", Thread::Subagent, Some("a1d139ff".into()));
    assert_eq!(pf.thread, Some(Thread::Subagent));
    assert_eq!(pf.agent_id.as_deref(), Some("a1d139ff"));
    assert_eq!(pf.session_id, "s1");
    assert_eq!(pf.calls.len(), 1);
}

#[test]
fn an_unparseable_line_is_counted_not_dropped_silently() {
    let text = format!(
        "{}\nthis is not json\n{{\"type\":\"assistant\"\n",
        assistant("msg_1", "2026-11-01T10:00:00.000Z", 1, 1, 1, 1)
    );
    let pf = parse_claude_text(&text, "s1", Thread::Main, None);
    assert_eq!(pf.calls.len(), 1);
    assert_eq!(pf.unreadable, 2);
}

fn codex_count(ts: &str, ordinal: u64, total: u64, input: u64, cached: u64, out: u64) -> String {
    format!(
        r#"{{"timestamp":"{ts}","ordinal":{ordinal},"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"cache_write_input_tokens":0,"output_tokens":{out},"reasoning_output_tokens":3,"total_tokens":{total}}}}},"rate_limits":{{"primary":{{"used_percent":7.0,"window_minutes":300,"resets_at":1}},"secondary":{{"used_percent":43.0,"window_minutes":10080,"resets_at":2}}}}}}}}"#
    )
}

const CODEX_META: &str = r#"{"timestamp":"2026-11-09T15:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"main-1","cwd":"/r"}}"#;
const CODEX_TURN: &str = r#"{"timestamp":"2026-11-09T15:00:01.000Z","ordinal":1,"type":"turn_context","payload":{"model":"gpt-6-astra","cwd":"/r"}}"#;

#[test]
fn codex_pair_with_a_repeat_counts_two_calls_and_reads_context_as_input() {
    let text = [
        CODEX_META.to_string(),
        CODEX_TURN.to_string(),
        codex_count("2026-11-09T15:00:05.000Z", 2, 1000, 900, 800, 100),
        // the same event again: an unchanged running total is a repeat
        codex_count("2026-11-09T15:00:05.500Z", 3, 1000, 900, 800, 100),
        codex_count("2026-11-09T15:00:09.000Z", 4, 2400, 1300, 1200, 100),
        r#"{"timestamp":"2026-11-09T15:00:10.000Z","ordinal":5,"type":"event_msg","payload":{"type":"token_count","info":null}}"#
            .to_string(),
    ]
    .join("\n");
    let pf = parse_codex_text(&text, "fallback");
    assert_eq!(pf.calls.len(), 2);
    let c = &pf.calls[0];
    assert_eq!(c.model, "gpt-6-astra");
    assert_eq!(
        c.context, 900,
        "input_tokens already includes the cached part"
    );
    assert_eq!(c.cache_read, 800);
    assert_eq!(c.input, 100, "the remainder after cache");
    assert_eq!(c.reasoning, Some(3));
    assert_eq!(pf.thread, Some(Thread::Main));
    assert_eq!(pf.session_id, "main-1");
    let q = pf.quota.unwrap();
    assert_eq!(q.primary.unwrap().used_percent, Some(7.0));
    assert_eq!(q.secondary.unwrap().window_minutes, Some(10080));
}

#[test]
fn codex_subagent_fork_skips_the_copied_history_and_reports_its_parent() {
    let meta = r#"{"timestamp":"2026-11-09T15:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"child-1","parent_thread_id":"parent-1","thread_source":"subagent","agent_nickname":"Parfit","subagent_history_start_ordinal":10,"cwd":"/r"}}"#;
    let text = [
        meta.to_string(),
        // copied from the parent: before the start ordinal
        codex_count("2026-11-09T14:00:00.000Z", 5, 9000, 8000, 7000, 50),
        codex_count("2026-11-09T15:00:05.000Z", 12, 500, 400, 0, 20),
    ]
    .join("\n");
    let pf = parse_codex_text(&text, "x");
    assert_eq!(pf.calls.len(), 1, "only the child's own call");
    assert_eq!(pf.calls[0].context, 400);
    assert_eq!(pf.thread, Some(Thread::Subagent));
    assert_eq!(pf.session_id, "parent-1");
    assert_eq!(pf.agent_id.as_deref(), Some("child-1"));
    assert_eq!(pf.label.as_deref(), Some("Parfit"));
}

#[test]
fn opencode_input_excludes_cache_so_context_adds_it_back() {
    let row = r#"{"time":{"created":1794441583052,"completed":1794441586756},"agent":"build","model":{"id":"claude-fable-5-2","providerID":"anthropic"},"tokens":{"input":4,"output":117,"reasoning":30,"cache":{"read":30633,"write":12}},"content":[]}"#;
    let c = opencode_call(row, 0).unwrap().unwrap();
    assert_eq!(c.context, 4 + 30633 + 12);
    assert_eq!(c.cache_read, 30633);
    assert_eq!(c.cache_creation, 12);
    assert_eq!(c.output, 117);
    assert_eq!(c.reasoning, Some(30));
    assert_eq!(c.model, "claude-fable-5-2");
    assert_eq!(c.at_ms, 1794441583052);
    assert_eq!(opencode_call(r#"{"content":[]}"#, 0).unwrap(), None);
    assert!(opencode_call("nope", 0).is_err());
}

const SRC: &str = r##"fn a() {
    let s = "}{ not code";
    let c = '{';
    let lt: &'static str = "x";
}
#[cfg(test)]
mod tests {
    use super::*;
    const RAW: &str = r#"}}}"#;
    #[test]
    fn t() {
        assert_eq!(format!("{}", 1), "1");
    }
}
fn after() {}
#[cfg(test)]
#[path = "x.rs"]
mod other;
#[cfg(test)]
mod second {
    // } in a comment
    fn q() {}
}
"##;

#[test]
fn test_spans_skip_braces_in_strings_chars_and_comments() {
    let spans = test_spans(SRC);
    assert_eq!(spans, vec![(6, 14), (19, 23)], "{spans:?}");
}

#[test]
fn read_result_lines_inside_and_outside_a_test_span() {
    // A Read result: spaces, line number, tab, text.
    let result = "     5\tfn a() {}\n     6\t#[cfg(test)]\n     7\tmod tests {\n    14\t}\n    15\tfn after() {}";
    let lines = read_result_lines(result);
    assert_eq!(
        lines.iter().map(|l| l.0).collect::<Vec<_>>(),
        vec![5, 6, 7, 14, 15]
    );
    let spans = test_spans(SRC);
    let (n, bytes) = test_share(&lines, &spans);
    assert_eq!(n, 3, "lines 6, 7 and 14 are inside (6, 14)");
    let want: u32 = lines
        .iter()
        .filter(|(l, _)| (6..=14).contains(l))
        .map(|(_, b)| *b)
        .sum();
    assert_eq!(bytes, want as u64);
}

#[test]
fn grep_content_mode_splits_two_files_and_context_lines() {
    let result = "src-tauri/src/lib.rs:120:fn a() {\nsrc-tauri/src/lib.rs-121-    let x = 1;\n--\napp/__shell/helpers.js:7:window.a = 1;\n[Showing results with pagination = limit: 250]";
    let by = grep_content_by_file(result, None);
    assert_eq!(by.len(), 2);
    assert_eq!(by[0].0, "src-tauri/src/lib.rs");
    assert_eq!(
        by[0].2.iter().map(|l| l.0).collect::<Vec<_>>(),
        vec![120, 121]
    );
    assert_eq!(by[1].0, "app/__shell/helpers.js");
    assert!(by[1].2.is_empty(), "line numbers kept only for lib.rs");
    assert_eq!(
        by[0].1,
        ("src-tauri/src/lib.rs:120:fn a() {".len()
            + 1
            + "src-tauri/src/lib.rs-121-    let x = 1;".len()
            + 1) as u64
    );
    // One file: the path is omitted from the lines.
    let one = grep_content_by_file("12:foo\n13-bar", Some("/r/src-tauri/src/lib.rs"));
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].0, "/r/src-tauri/src/lib.rs");
}

#[test]
fn shell_reads_name_their_files_and_other_commands_name_none() {
    assert_eq!(
        shell_named_files("sed -n '100,200p' src-tauri/src/lib.rs"),
        vec!["src-tauri/src/lib.rs"]
    );
    assert_eq!(
        shell_named_files("grep -n \"foo\" app/__shell/helpers.js | head -80"),
        vec!["app/__shell/helpers.js"]
    );
    assert_eq!(shell_named_files("cat a.md b.md"), vec!["a.md", "b.md"]);
    assert!(shell_named_files("cargo build --manifest-path src-tauri/Cargo.toml").is_empty());
}

#[test]
fn tool_results_pair_with_their_calls_through_the_parser() {
    let lines = [
        r#"{"type":"user","timestamp":"2026-11-01T10:00:00.000Z","message":{"role":"user","content":"hello"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-01T10:00:01.000Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"tool_use","id":"tu1","name":"Read","input":{"file_path":"/r/src-tauri/src/lib.rs","offset":6,"limit":3}},{"type":"tool_use","id":"tu2","name":"Bash","input":{"command":"sed -n '1,5p' /r/app/main.js"}}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        r#"{"type":"user","timestamp":"2026-11-01T10:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"     6\t#[cfg(test)]\n     7\tmod tests {\n    14\t}"},{"type":"tool_result","tool_use_id":"tu2","content":[{"type":"text","text":"abcde"}]}]}}"#,
    ]
    .join("\n");
    let pf = parse_claude_text(&lines, "s", Thread::Main, None);
    assert_eq!(pf.reads.len(), 2);
    assert_eq!(pf.reads[0].path, "/r/src-tauri/src/lib.rs");
    assert!(!pf.reads[0].via_shell);
    assert_eq!(pf.reads[0].lines.len(), 3);
    assert_eq!(pf.reads[1].path, "/r/app/main.js");
    assert!(pf.reads[1].via_shell);
    assert_eq!(pf.reads[1].bytes, 5);
}

#[test]
fn a_real_user_message_starts_the_turn() {
    let lines = [
        r#"{"type":"user","timestamp":"2026-11-01T10:00:00.000Z","message":{"role":"user","content":"one"}}"#.to_string(),
        assistant("m1", "2026-11-01T10:00:01.000Z", 1, 1, 1, 1),
        r#"{"type":"user","timestamp":"2026-11-01T10:00:05.000Z","message":{"role":"user","content":"two"}}"#.to_string(),
        assistant("m2", "2026-11-01T10:00:06.000Z", 1, 1, 1, 1),
        assistant("m3", "2026-11-01T10:00:07.000Z", 1, 1, 1, 1),
    ]
    .join("\n");
    let mut p = ClaudeParser::new(false);
    p.feed(&lines);
    assert_eq!(p.turn_start.0, 1);
    assert_eq!(p.calls.len() - p.turn_start.0, 2);
}

fn raw(at: i64, model: &str, context: u64, cache_read: u64, output: u64) -> RawCall {
    RawCall {
        at_ms: at,
        model: model.into(),
        context,
        input: context - cache_read,
        cache_read,
        cache_creation: 0,
        output,
        reasoning: None,
    }
}

struct NoSpans;
impl SpanSource for NoSpans {
    fn spans_at(&mut self, _: i64) -> Option<Arc<Vec<(u32, u32)>>> {
        None
    }
}

fn session(id: &str, thread: Thread, calls: Vec<RawCall>) -> Arc<ParsedFile> {
    Arc::new(ParsedFile {
        provider: "claude",
        session_id: id.into(),
        thread: Some(thread),
        calls,
        ..Default::default()
    })
}

#[test]
fn nearest_rank_percentiles_on_a_small_fixed_set() {
    let v: Vec<u64> = (1..=10).map(|n| n * 10).collect();
    assert_eq!(nearest_rank(&v, 50.0), 50);
    assert_eq!(nearest_rank(&v, 90.0), 90);
    assert_eq!(nearest_rank(&v, 99.0), 100);
    assert_eq!(nearest_rank(&v, 1.0), 10);
    assert_eq!(nearest_rank(&[], 50.0), 0);
}

#[test]
fn attribution_splits_a_window_across_its_items_and_leaves_the_rest_to_chat() {
    let windows = vec![
        ClaimWindow {
            from_ms: 100,
            to_ms: 200,
            kind: "approved".into(),
            ids: vec!["a".into(), "b".into()],
        },
        ClaimWindow {
            from_ms: 300,
            to_ms: 301,
            kind: "approved".into(),
            ids: vec![],
        },
    ];
    assert_eq!(
        attribute(&windows, 150),
        Attribution::Items(vec!["a".into(), "b".into()])
    );
    assert_eq!(attribute(&windows, 250), Attribution::Chat);
    assert_eq!(
        attribute(&windows, 300),
        Attribution::Chat,
        "empty ids is no item"
    );
    assert_eq!(attribute(&windows, 50), Attribution::Chat);

    let main = session(
        "s1",
        Thread::Main,
        vec![
            raw(110, "m", 1000, 800, 10),
            raw(150, "m", 2000, 1800, 20),
            raw(250, "m", 3000, 2800, 30),
        ],
    );
    let report = build_report(
        ReportInput {
            root: "/r",
            files: vec![main],
            windows: Some(windows),
            since_ms: None,
            session_filter: None,
            include_calls: true,
            instruction_bytes: vec![],
        },
        &mut NoSpans,
        None,
    );
    let s = &report.sessions[0];
    assert_eq!(s.calls, 3);
    assert_eq!(s.chat.calls, 1.0);
    assert_eq!(s.items.len(), 2);
    assert_eq!(s.items[0].calls, 1.0, "two calls split across two items");
    assert_eq!(s.items[0].context, 1500);
    assert_eq!(s.items[0].peak_context, 2000);
    assert_eq!(report.attribution.source, "state-mirror");
    assert_eq!(report.calls.unwrap().len(), 3);
}

#[test]
fn window_fit_floor_and_uncached_math_on_fixed_calls() {
    // Two sessions on one model. s1: 20k, 70k, 150k. s2: 30k, 40k.
    let s1 = session(
        "s1",
        Thread::Main,
        vec![
            raw(0, "m", 20_000, 0, 10),
            raw(60_000, "m", 70_000, 50_000, 20),
            raw(120_000, "m", 150_000, 100_000, 30),
        ],
    );
    let s2 = session(
        "s2",
        Thread::Main,
        vec![raw(0, "m", 30_000, 0, 5), raw(600_000, "m", 40_000, 0, 5)],
    );
    // A subagent's calls never count toward the main-thread model table.
    let sub = session("s1", Thread::Subagent, vec![raw(5, "m", 999_999, 0, 1)]);
    let report = build_report(
        ReportInput {
            root: "/r",
            files: vec![s1, s2, sub],
            windows: None,
            since_ms: None,
            session_filter: None,
            include_calls: false,
            instruction_bytes: vec![InstructionFile {
                path: "CLAUDE.md".into(),
                bytes: 9,
            }],
        },
        &mut NoSpans,
        None,
    );
    assert_eq!(report.attribution.source, "unavailable");
    let m = &report.constrained.by_model[0];
    assert_eq!(m.calls, 5);
    assert_eq!(m.sessions, 2);
    assert_eq!(m.context.peak, 150_000);
    assert_eq!(
        m.context.p50, 40_000,
        "rank ceil(0.5*5)=3 of 20,30,40,70,150"
    );
    assert_eq!(m.context.p90, 150_000);
    assert_eq!(m.fits["32k"], 0.4, "20k and 30k of five calls");
    assert_eq!(m.fits["64k"], 0.6);
    assert_eq!(m.fits["128k"], 0.8);
    assert_eq!(m.fits["200k"], 1.0);
    assert_eq!(m.sessions_fit["64k"], 0.5, "s2 peaks at 40k, s1 at 150k");
    assert_eq!(m.sessions_fit["200k"], 1.0);
    assert_eq!(m.floor.min, 20_000);
    assert_eq!(m.floor.max, 30_000);
    assert_eq!(m.floor.p50, 20_000);
    // uncached per call: 20k, 20k, 50k, 30k, 40k
    assert_eq!(m.uncached_per_call.p50, 30_000);
    // cache hit: (0+50k+100k+0+0) / (20+70+150+30+40)k
    assert_eq!(m.cache_hit_share, 0.4839);
    // gaps within a turn: 60s, 60s; the 10-minute gap is a new turn
    assert_eq!(m.gap_ms.p50, 60_000);
    assert_eq!(m.output_per_call.p50, 10);
    assert_eq!(report.constrained.instruction_bytes[0].bytes, 9);
    // The subagent still has its own session row.
    assert!(report.sessions.iter().any(|s| s.thread == Thread::Subagent));
}

#[test]
fn since_and_session_filters_limit_the_report() {
    let a = session(
        "a",
        Thread::Main,
        vec![raw(100, "m", 1, 0, 1), raw(300, "m", 1, 0, 1)],
    );
    let b = session("b", Thread::Main, vec![raw(300, "m", 1, 0, 1)]);
    let mk = |since, filter| {
        build_report(
            ReportInput {
                root: "/r",
                files: vec![a.clone(), b.clone()],
                windows: None,
                since_ms: since,
                session_filter: filter,
                include_calls: false,
                instruction_bytes: vec![],
            },
            &mut NoSpans,
            None,
        )
    };
    assert_eq!(
        mk(Some(200), None)
            .sessions
            .iter()
            .map(|s| s.calls)
            .sum::<usize>(),
        2
    );
    assert_eq!(
        mk(None, Some(("claude".into(), "b".into()))).sessions.len(),
        1
    );
}

struct FixedSpans(Vec<(u32, u32)>);
impl SpanSource for FixedSpans {
    fn spans_at(&mut self, _: i64) -> Option<Arc<Vec<(u32, u32)>>> {
        Some(Arc::new(self.0.clone()))
    }
}

#[test]
fn lib_rs_reads_report_test_lines_and_bytes_and_shell_reads_stay_apart() {
    let pf = Arc::new(ParsedFile {
        provider: "claude",
        session_id: "s".into(),
        thread: Some(Thread::Main),
        calls: vec![raw(1, "m", 1, 0, 1)],
        reads: vec![
            ReadEvent {
                path: "/r/src-tauri/src/lib.rs".into(),
                bytes: 300,
                via_shell: false,
                at_ms: 1,
                lines: vec![(5, 100), (8, 100), (20, 100)],
            },
            ReadEvent {
                path: "/r/src-tauri/src/lib.rs".into(),
                bytes: 50,
                via_shell: true,
                at_ms: 1,
                lines: vec![],
            },
        ],
        ..Default::default()
    });
    let report = build_report(
        ReportInput {
            root: "/r",
            files: vec![pf],
            windows: None,
            since_ms: None,
            session_filter: None,
            include_calls: false,
            instruction_bytes: vec![],
        },
        &mut FixedSpans(vec![(6, 10)]),
        None,
    );
    let f = &report.files[0];
    assert_eq!(f.path, "src-tauri/src/lib.rs");
    assert_eq!((f.reads, f.bytes, f.via_shell_bytes), (1, 300, 50));
    assert_eq!(
        (f.lines_read, f.test_lines, f.test_bytes),
        (Some(3), Some(1), Some(100))
    );
}

#[test]
fn the_sample_line_names_the_turn() {
    let s = Sample {
        provider: "codex",
        session: "abc".into(),
        model: "gpt-6-astra".into(),
        tokens: 79324,
        turn_calls: 3,
        turn_output: 400,
        cache_read: 123456,
        compactions: 1,
        five_hour_pct: Some(7.0),
        weekly_pct: Some(43.0),
        ..Default::default()
    };
    assert_eq!(
        format_sample(&s),
        "op=sample provider=codex session=abc model=gpt-6-astra tokens=79324 turnCalls=3 turnOutput=400 cacheRead=123456 compactions=1 fiveHourPct=7 weeklyPct=43"
    );
    let sub = Sample {
        subagent: true,
        agents: 2,
        provider: "claude",
        session: "s".into(),
        ..Default::default()
    };
    assert!(format_sample(&sub).starts_with("op=sample provider=claude thread=subagent session=s"));
    assert!(format_sample(&sub).ends_with("agents=2"));
}

#[test]
fn the_tail_reader_returns_only_whole_new_lines() {
    let dir = std::env::temp_dir().join(format!("ta-tail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("s.jsonl");
    std::fs::write(&p, "one\ntwo\npart").unwrap();
    let (text, off, reset) = read_tail(&p, 0).unwrap();
    assert_eq!((text.as_str(), off, reset), ("one\ntwo\n", 8, false));
    std::fs::write(&p, "one\ntwo\npartial\nthree\n").unwrap();
    let (text, off, _) = read_tail(&p, 8).unwrap();
    assert_eq!(text, "partial\nthree\n");
    assert_eq!(off, 22);
    std::fs::write(&p, "x\n").unwrap();
    let (_, _, reset) = read_tail(&p, 22).unwrap();
    assert!(reset, "a shorter file starts over");
    let _ = std::fs::remove_dir_all(&dir);
}
