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
        key: format!("k{at}"),
        at_ms: at,
        model: model.into(),
        context,
        input: context - cache_read,
        cache_read,
        cache_creation: 0,
        output,
        reasoning: None,
        attribution: None,
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
            ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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
                key: String::new(),
                path: "/r/src-tauri/src/lib.rs".into(),
                bytes: 300,
                via_shell: false,
                at_ms: 1,
                lines: vec![(5, 100), (8, 100), (20, 100)],
            },
            ReadEvent {
                key: String::new(),
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
            ..Default::default()
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

// token-accounting-status-explains-columns: files rows carry the size on disk.
struct NoSpansHere;
impl SpanSource for NoSpansHere {
    fn spans_at(&mut self, _at_ms: i64) -> Option<Arc<Vec<(u32, u32)>>> {
        None
    }
}

fn report_for(root: &str, path: &str) -> Report {
    let pf = Arc::new(ParsedFile {
        provider: "claude",
        session_id: "s".into(),
        thread: Some(Thread::Main),
        reads: vec![ReadEvent {
            key: String::new(),
            path: path.into(),
            bytes: 5,
            via_shell: false,
            at_ms: 1,
            lines: vec![],
        }],
        ..Default::default()
    });
    build_report(
        ReportInput {
            root,
            files: vec![pf],
            windows: None,
            since_ms: None,
            session_filter: None,
            include_calls: false,
            instruction_bytes: vec![],
            ..Default::default()
        },
        &mut NoSpansHere,
        None,
    )
}

#[test]
fn a_files_row_serializes_size_bytes_and_null_when_missing() {
    let dir = std::env::temp_dir().join(format!("bram-ta-size-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), b"12345").unwrap();
    let root = dir.to_string_lossy().into_owned();
    let present = report_for(&root, "a.txt");
    assert_eq!(
        serde_json::to_value(&present.files[0]).unwrap()["sizeBytes"],
        5
    );
    let missing = report_for(&root, "gone.txt");
    let v = serde_json::to_value(&missing.files[0]).unwrap();
    assert!(v.get("sizeBytes").is_some_and(|s| s.is_null()));
    let _ = std::fs::remove_dir_all(&dir);
}

fn read_event(path: &str, bytes: u64) -> ReadEvent {
    ReadEvent {
        key: String::new(),
        path: path.into(),
        bytes,
        via_shell: false,
        at_ms: 5,
        lines: vec![],
    }
}

fn call_at(at_ms: i64, context: u64) -> RawCall {
    RawCall {
        at_ms,
        model: "m".into(),
        context,
        ..Default::default()
    }
}

#[test]
fn a_files_row_splits_main_and_subagent_reads_and_counts_sessions() {
    let main_a = Arc::new(ParsedFile {
        provider: "claude",
        session_id: "a".into(),
        thread: Some(Thread::Main),
        calls: vec![call_at(1000, 10), call_at(2000, 10)],
        reads: vec![read_event("/r/x.rs", 100), read_event("/r/x.rs", 50)],
        ..Default::default()
    });
    let sub_a = Arc::new(ParsedFile {
        provider: "claude",
        session_id: "a".into(),
        thread: Some(Thread::Subagent),
        agent_id: Some("g".into()),
        reads: vec![read_event("/r/x.rs", 400)],
        ..Default::default()
    });
    let main_b = Arc::new(ParsedFile {
        provider: "claude",
        session_id: "b".into(),
        thread: Some(Thread::Main),
        calls: vec![call_at(500, 10)],
        reads: vec![read_event("/r/x.rs", 30)],
        ..Default::default()
    });
    let report = build_report(
        ReportInput {
            root: "/r",
            files: vec![main_a, sub_a, main_b],
            ..Default::default()
        },
        &mut NoSpansHere,
        None,
    );
    let f = &report.files[0];
    assert_eq!(f.path, "x.rs");
    assert_eq!((f.reads, f.bytes), (4, 580));
    assert_eq!((f.main_reads, f.main_bytes), (3, 180));
    assert_eq!((f.subagent_reads, f.subagent_bytes), (1, 400));
    assert_eq!(f.sessions, 2, "a's subagent counts for a");
    assert_eq!(f.max_session_bytes, 550, "a: 150 main + 400 subagent");
    let range = report.range.unwrap();
    assert_eq!((range.from_ms, range.to_ms), (500, 2000));
    let v = serde_json::to_value(f).unwrap();
    assert_eq!(v["mainReads"], 3);
    assert_eq!(v["maxSessionBytes"], 550);
}

#[test]
fn a_replayed_tool_result_is_one_read() {
    // Resumed transcripts repeat earlier records; a tool call is one read.
    let use_line = r#"{"type":"assistant","message":{"id":"m","model":"x","content":[{"type":"tool_use","id":"tu1","name":"Read","input":{"file_path":"/r/a.rs"}}]}}"#;
    let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","content":"     1\thello"}]}}"#;
    let text = [use_line, result, result].join("\n");
    let pf = parse_claude_text(&text, "s", Thread::Main, None);
    assert_eq!(pf.reads.len(), 1);
}

#[test]
fn floor_parts_add_up_to_the_floor() {
    // 81k floor, 18k native, 45,000 bytes of conventions (11,250 tokens).
    let native = Some(NativeFloor {
        p50: 18_000,
        sessions: 4,
    });
    let p = floor_parts(81_000, native, "same model", 45_000);
    assert_eq!(p.conventions.approx_tokens, 11_250);
    assert_eq!(p.project, Some(81_000 - 18_000 - 11_250));
    // Parts never go negative when the estimate overshoots.
    let small = floor_parts(
        20_000,
        Some(NativeFloor {
            p50: 18_000,
            sessions: 1,
        }),
        "same model",
        45_000,
    );
    assert_eq!(small.project, Some(0));
    // Without a native measurement there is no remainder to name.
    let none = floor_parts(81_000, None, "not measured", 45_000);
    assert_eq!(none.project, None);
    assert_eq!(none.native_basis, "not measured");
}

#[test]
fn native_floor_prefers_the_model_then_the_provider() {
    let s = |provider: &str, model: &str, context: u64| NativeSample {
        provider: provider.into(),
        model: model.into(),
        context,
    };
    let samples = vec![
        s("claude", "a", 10_000),
        s("claude", "a", 20_000),
        s("claude", "a", 30_000),
        s("claude", "b", 50_000),
        s("codex", "g", 7_000),
    ];
    let (n, basis) = native_floor(&samples, "claude", "a");
    assert_eq!((n.unwrap().p50, basis), (20_000, "same model"));
    let (n, basis) = native_floor(&samples, "claude", "c");
    let n = n.unwrap();
    assert_eq!(
        (n.p50, n.sessions, basis),
        (20_000, 4, "same provider, other models")
    );
    let (n, basis) = native_floor(&samples, "opencode", "z");
    assert_eq!((n, basis), (None, "not measured"));
}

#[test]
fn the_model_row_carries_floor_parts_and_the_report_serializes_them() {
    let pf = Arc::new(ParsedFile {
        provider: "claude",
        session_id: "s".into(),
        thread: Some(Thread::Main),
        calls: vec![call_at(1, 81_000)],
        ..Default::default()
    });
    let mut conv = BTreeMap::new();
    conv.insert("claude".to_string(), 44_000u64);
    let report = build_report(
        ReportInput {
            root: "/r",
            files: vec![pf],
            native_samples: vec![NativeSample {
                provider: "claude".into(),
                model: "m".into(),
                context: 18_000,
            }],
            conventions_bytes: conv,
            ..Default::default()
        },
        &mut NoSpansHere,
        None,
    );
    let v = serde_json::to_value(&report.constrained.by_model[0]).unwrap();
    let fp = &v["floorParts"];
    assert_eq!(fp["native"]["p50"], 18_000);
    assert_eq!(fp["nativeBasis"], "same model");
    assert_eq!(fp["conventions"]["approxTokens"], 11_000);
    assert_eq!(fp["project"], 52_000);
}

#[test]
fn the_bram_marker_block_is_the_text_between_the_markers() {
    let t = "intro\n<!-- bram:start -->\nabc\n<!-- bram:end -->\ntail";
    assert_eq!(
        bram_marker_block(t),
        Some("<!-- bram:start -->\nabc\n<!-- bram:end -->")
    );
    assert_eq!(bram_marker_block("no markers"), None);
}

// --- the ledger -------------------------------------------------------------

fn ledger_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("bram-ledger-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tool_use(id: &str, ts: &str, tool_id: &str, file: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5-5","content":[{{"type":"tool_use","id":"{tool_id}","name":"Read","input":{{"file_path":"{file}"}}}}],"usage":{{"input_tokens":1,"cache_read_input_tokens":10,"cache_creation_input_tokens":0,"output_tokens":5}}}}}}"#
    )
}

fn tool_result(ts: &str, tool_id: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{ts}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_id}","content":"{text}"}}]}}}}"#
    )
}

fn append(path: &Path, lines: &[String]) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

fn claude_source(dir: &Path) -> Source {
    Source::ClaudeMain {
        id: "sess1".into(),
        path: dir.join("sess1.jsonl"),
    }
}

fn report_from(ledger: &Ledger) -> Report {
    build_report(
        ReportInput {
            root: "/r",
            files: ledger.load(None),
            ..Default::default()
        },
        &mut NoSpans,
        None,
    )
}

fn count(ledger: &Ledger, table: &str) -> i64 {
    ledger
        .conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_call_ingested_twice_is_stored_once() {
    let dir = ledger_dir("twice");
    let src = claude_source(&dir);
    append(
        src.path(),
        &[
            assistant("msg_1", "2026-11-01T10:00:00.000Z", 2, 100, 50, 10),
            assistant("msg_2", "2026-11-01T10:00:09.000Z", 3, 200, 0, 7),
        ],
    );
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    let first = ledger.ingest_sources(&[src.clone()], None);
    assert_eq!((first.calls_inserted, first.sources), (2, 1));
    // Forget how far the file was read: every record is seen again.
    ledger.conn.execute("DELETE FROM sources", []).unwrap();
    let again = ledger.ingest_sources(&[src.clone()], None);
    assert_eq!((again.calls_inserted, again.skipped), (0, 2));
    assert_eq!(count(&ledger, "calls"), 2);
    // Untouched file: not even read.
    let third = ledger.ingest_sources(&[src], None);
    assert_eq!((third.sources, third.unchanged), (0, 1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_source_deleted_after_ingest_still_counts_in_the_report() {
    let dir = ledger_dir("deleted");
    let src = claude_source(&dir);
    append(
        src.path(),
        &[
            tool_use("msg_1", "2026-11-01T10:00:00.000Z", "tu1", "/r/a.txt"),
            tool_result("2026-11-01T10:00:01.000Z", "tu1", "hello"),
            assistant("msg_2", "2026-11-01T10:00:09.000Z", 3, 200, 0, 7),
        ],
    );
    let db = dir.join("l.db");
    let before = {
        let mut ledger = Ledger::open(&db).unwrap();
        ledger.ingest_sources(&[src.clone()], None);
        serde_json::to_value(report_from(&ledger)).unwrap()
    };
    std::fs::remove_file(src.path()).unwrap();
    // A later pass finds nothing on disk and changes nothing.
    let mut ledger = Ledger::open(&db).unwrap();
    let st = ledger.ingest_sources(&[], None);
    assert_eq!(st.inserted(), 0);
    let after = serde_json::to_value(report_from(&ledger)).unwrap();
    assert_eq!(after["sessions"][0]["calls"], 2);
    assert_eq!(after["files"][0]["path"], "a.txt");
    let strip = |mut v: serde_json::Value| {
        v.as_object_mut().unwrap().remove("ms");
        v
    };
    assert_eq!(strip(before), strip(after));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_incremental_pass_inserts_only_the_new_lines_and_resumes_a_pending_read() {
    let dir = ledger_dir("incr");
    let src = claude_source(&dir);
    append(
        src.path(),
        &[
            assistant("msg_1", "2026-11-01T10:00:00.000Z", 2, 100, 50, 10),
            // A read whose result has not been written yet.
            tool_use("msg_2", "2026-11-01T10:00:05.000Z", "tu1", "/r/a.txt"),
        ],
    );
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    let first = ledger.ingest_sources(&[src.clone()], None);
    assert_eq!((first.calls_inserted, first.reads_inserted), (2, 0));
    append(
        src.path(),
        &[
            tool_result("2026-11-01T10:00:06.000Z", "tu1", "hello"),
            assistant("msg_3", "2026-11-01T10:00:09.000Z", 3, 200, 0, 7),
        ],
    );
    let second = ledger.ingest_sources(&[src], None);
    assert_eq!(
        (second.calls_inserted, second.reads_inserted, second.skipped),
        (1, 1, 0),
        "only the appended lines are read"
    );
    assert_eq!(count(&ledger, "calls"), 3);
    assert_eq!(count(&ledger, "reads"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_streamed_message_split_across_passes_keeps_the_last_usage_and_first_time() {
    let dir = ledger_dir("stream");
    let src = claude_source(&dir);
    append(
        src.path(),
        &[assistant(
            "msg_1",
            "2026-11-01T10:00:00.000Z",
            2,
            100,
            50,
            10,
        )],
    );
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    ledger.ingest_sources(&[src.clone()], None);
    append(
        src.path(),
        &[assistant(
            "msg_1",
            "2026-11-01T10:00:01.000Z",
            2,
            100,
            50,
            430,
        )],
    );
    ledger.ingest_sources(&[src], None);
    let pf = ledger.load(None);
    assert_eq!(pf[0].calls.len(), 1);
    assert_eq!(pf[0].calls[0].output, 430);
    assert_eq!(pf[0].calls[0].at_ms, t("2026-11-01T10:00:00.000Z"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_shrank_is_read_again_from_the_start() {
    let dir = ledger_dir("shrunk");
    let src = claude_source(&dir);
    append(
        src.path(),
        &[
            assistant("msg_1", "2026-11-01T10:00:00.000Z", 2, 100, 50, 10),
            assistant("msg_2", "2026-11-01T10:00:09.000Z", 3, 200, 0, 7),
        ],
    );
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    ledger.ingest_sources(&[src.clone()], None);
    std::fs::write(
        src.path(),
        assistant("msg_9", "2026-11-02T10:00:00.000Z", 1, 1, 1, 1) + "\n",
    )
    .unwrap();
    let st = ledger.ingest_sources(&[src], None);
    assert_eq!(
        st.calls_inserted, 1,
        "msg_9 is new; the old two stay stored"
    );
    assert_eq!(count(&ledger, "calls"), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_report_from_the_ledger_equals_the_report_from_the_file() {
    let dir = ledger_dir("same");
    let src = claude_source(&dir);
    let lines = [
        assistant("msg_1", "2026-11-01T10:00:00.000Z", 2, 100, 50, 10),
        tool_use(
            "msg_2",
            "2026-11-01T10:00:05.000Z",
            "tu1",
            "/r/src-tauri/src/lib.rs",
        ),
        tool_result(
            "2026-11-01T10:00:06.000Z",
            "tu1",
            "     1\\tfn a() {}\\n     2\\tfn b() {}",
        ),
        assistant("msg_3", "2026-11-01T10:00:09.000Z", 3, 200, 0, 7),
    ];
    append(src.path(), &lines);
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    ledger.ingest_sources(&[src.clone()], None);
    let from_ledger = serde_json::to_value(report_from(&ledger)).unwrap();
    let text = std::fs::read_to_string(src.path()).unwrap();
    let direct = build_report(
        ReportInput {
            root: "/r",
            files: vec![Arc::new(parse_claude_text(
                &text,
                "sess1",
                Thread::Main,
                None,
            ))],
            ..Default::default()
        },
        &mut NoSpans,
        None,
    );
    let mut direct = serde_json::to_value(direct).unwrap();
    let mut from_ledger = from_ledger;
    for v in [&mut direct, &mut from_ledger] {
        v.as_object_mut().unwrap().remove("ms");
    }
    assert_eq!(direct, from_ledger);
    // The wire shape: the same top-level fields as before the ledger.
    let mut keys: Vec<&str> = from_ledger
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "attribution",
            "constrained",
            "files",
            "filesTotal",
            "providers",
            "range",
            "root",
            "sessions"
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_codex_rollout_read_in_two_passes_matches_one_pass() {
    let dir = ledger_dir("codex");
    let path = dir.join("rollout-x.jsonl");
    let meta =
        r#"{"type":"session_meta","timestamp":"2026-11-09T14:59:00.000Z","payload":{"id":"cx1"}}"#;
    let ctx = r#"{"type":"turn_context","timestamp":"2026-11-09T14:59:01.000Z","payload":{"model":"gpt-5.5"}}"#;
    let first = [
        meta.to_string(),
        ctx.to_string(),
        codex_count("2026-11-09T15:00:05.000Z", 2, 1000, 900, 800, 100),
    ];
    let second = [
        codex_count("2026-11-09T15:00:05.500Z", 3, 1000, 900, 800, 100),
        codex_count("2026-11-09T15:00:09.000Z", 4, 2400, 1300, 1200, 100),
    ];
    append(&path, &first);
    let src = Source::Codex {
        id: "fallback".into(),
        path: path.clone(),
    };
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    ledger.ingest_sources(&[src.clone()], None);
    append(&path, &second);
    ledger.ingest_sources(&[src], None);
    let whole = parse_codex_text(&std::fs::read_to_string(&path).unwrap(), "fallback");
    let got = ledger.load(None);
    assert_eq!(got[0].session_id, whole.session_id);
    assert_eq!(got[0].calls.len(), whole.calls.len());
    assert_eq!(got[0].calls[1].model, "gpt-5.5");
    assert_eq!(
        got[0].calls.iter().map(|c| c.context).collect::<Vec<_>>(),
        whole.calls.iter().map(|c| c.context).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn old_calls_are_stamped_from_the_claims_and_young_ones_are_left_to_report_time() {
    let dir = ledger_dir("stamp");
    let src = claude_source(&dir);
    let now = now_epoch_ms();
    let old = chrono_like(now - 3_600_000);
    let young = chrono_like(now - 1_000);
    append(
        src.path(),
        &[
            assistant("msg_old", &old, 1, 1, 0, 1),
            assistant("msg_new", &young, 1, 1, 0, 1),
        ],
    );
    let claims = rusqlite::Connection::open_in_memory().unwrap();
    claims
        .execute_batch(
            "CREATE TABLE claims (written_at_ms INTEGER, cleared_at_ms INTEGER, kind TEXT, ids TEXT);",
        )
        .unwrap();
    claims
        .execute(
            "INSERT INTO claims VALUES (?1, ?2, 'approved', '[\"item-a\"]')",
            rusqlite::params![now - 4_000_000, now + 10],
        )
        .unwrap();
    let mut ledger = Ledger::open(&dir.join("l.db")).unwrap();
    ledger.ingest_sources(&[src], Some(&claims));
    let pf = ledger.load(None);
    let by = |k: &str| {
        pf[0]
            .calls
            .iter()
            .find(|c| c.key == k)
            .unwrap()
            .attribution
            .clone()
    };
    assert_eq!(
        by("msg_old"),
        Some(Attribution::Items(vec!["item-a".into()]))
    );
    assert_eq!(by("msg_new"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

fn chrono_like(ms: i64) -> String {
    // "YYYY-MM-DDTHH:MM:SS.mmmZ" for an epoch ms, via the days-from-civil inverse.
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        ms.rem_euclid(1000)
    )
}

// --- Antigravity ---------------------------------------------------------

#[test]
fn agy_planner_response_is_a_call_whose_context_adds_cache_back() {
    let line = r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-12T21:47:06Z","input_tokens":2547,"cache_read_tokens":"19733","output_tokens":352.0,"content":"x"}"#;
    let c = agy_call(line).unwrap().unwrap();
    assert_eq!(c.key, "3");
    assert_eq!(c.input, 2547);
    assert_eq!(c.cache_read, 19733, "strings and numbers both read");
    assert_eq!(c.context, 2547 + 19733, "input excludes cache");
    assert_eq!(c.output, 352);
    assert_eq!(c.at_ms, t("2026-11-12T21:47:06Z"));
    // other record types are not calls; a broken line is an error
    let other = r#"{"step_index":2,"type":"VIEW_FILE","created_at":"2026-11-12T21:47:06Z"}"#;
    assert_eq!(agy_call(other).unwrap(), None);
    assert!(agy_call("nope").is_err());
}

#[test]
fn agy_conversation_summaries_match_the_root_and_mark_subagents() {
    let dir = std::env::temp_dir().join(format!("agy-root-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.to_string_lossy().into_owned();
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, \
         workspace_uris TEXT, project_id TEXT, parent_conversation_id TEXT, nesting_depth INTEGER);",
    )
    .unwrap();
    let ins = |id: &str, uris: &str, parent: Option<&str>| {
        conn.execute(
            "INSERT INTO conversation_summaries VALUES (?1,'t',?2,'p',?3,0)",
            rusqlite::params![id, uris, parent],
        )
        .unwrap();
    };
    ins("main1", &format!(r#"["file://{root}"]"#), None);
    ins("sub1", &format!(r#"["file://{root}"]"#), Some("main1"));
    ins("other", r#"["file:///somewhere/else"]"#, None);
    let got = agy_conversations(&conn, &dir);
    assert_eq!(
        got,
        vec![
            AgyConversation {
                id: "main1".into(),
                parent: None
            },
            AgyConversation {
                id: "sub1".into(),
                parent: Some("main1".into())
            },
        ]
    );
    let pf = parse_antigravity_text("", "sub1", Some("main1"), &[]);
    assert_eq!(pf.thread, Some(Thread::Subagent));
    assert_eq!(pf.session_id, "main1");
    assert_eq!(pf.agent_id.as_deref(), Some("sub1"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agy_log_line_gives_a_model_label_with_its_time() {
    let line = r#"I1112 13:45:59.815853     471 model_config_manager.go:375] Propagating selected model override to backend: label="Gemini 4 Argon (Medium)""#;
    let (stamp, label) = parse_agy_selection_line(line).unwrap();
    assert_eq!(label, "Gemini 4 Argon (Medium)");
    assert_eq!(
        stamp,
        GlogStamp {
            month: 11,
            day: 12,
            hour: 13,
            minute: 45,
            second: 59,
            millis: 815
        }
    );
    assert_eq!(agy_log_year("cli-20261112_134518.log"), Some(2026));
    assert!(parse_agy_selection_line("I1112 13:45:59.8 x.go:1] something else").is_none());
}

#[test]
fn agy_step_takes_the_last_selection_at_or_before_it() {
    let sels = vec![
        ModelSelection {
            at_ms: 1000,
            label: "A".into(),
        },
        ModelSelection {
            at_ms: 2000,
            label: "B".into(),
        },
    ];
    assert_eq!(agy_model_at(&sels, 999), "unknown");
    assert_eq!(agy_model_at(&sels, 1000), "A");
    assert_eq!(agy_model_at(&sels, 2500), "B");
    let text = r#"{"step_index":1,"type":"PLANNER_RESPONSE","created_at":"2026-11-12T21:47:00Z","input_tokens":5,"cache_read_tokens":0,"output_tokens":1}"#;
    let pf = parse_antigravity_text(text, "c1", None, &sels);
    assert_eq!(pf.calls.len(), 1);
    assert_eq!(pf.calls[0].model, "B");
}
