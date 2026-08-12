#!/usr/bin/env python3
"""Nominate XMLUI how-to gaps, then validate them with Bram search.

The XMLUI MCP analytics tape is global and outcome-blind. This tool therefore
uses it only to nominate behavioral search episodes. It groups nearby how-to
queries by shared topic terms, records rephrasing and fallback to component
docs/examples/source, and then asks Bram's project-local SQLite search API what
happened in sessions, commits, issues, and worklist history.

The final classification is a review aid, not an automated verdict. Every row
includes the MCP behavior, indexed evidence, and current-corpus matches that
produced the hint. No score uses MCP result_count or co-timed frustration.

Read-only, standard library only. Prints text or JSON; never writes.
"""

from __future__ import annotations

import argparse
import collections
import dataclasses
import datetime as dt
import json
import re
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Callable, Sequence


DEFAULT_ANALYTICS = Path(
    "~/Library/Caches/xmlui/xmlui-mcp/xmlui-mcp-analytics.json"
).expanduser()
DEFAULT_HOWTO_DIR = Path(
    "~/xmlui/website/content/docs/pages/howto"
).expanduser()
DEFAULT_PORT_FILE = Path("resources/.bram-port")
DEFAULT_TYPES = "session,issue,commit,worklist-history"

SEARCH_TOOLS = {
    "xmlui_search_howto",
    "xmlui_search",
    "xmlui_examples",
    "xmlui_pattern",
}
FALLBACK_TOOLS = SEARCH_TOOLS | {"xmlui_component_docs", "xmlui_read_file"}

STOPWORDS = frozenset(
    """
    a an and are as at be by can could did do does for from get gets getting
    how i in into is it its make me my need not of on or our should show that
    the their them these this those to use used using want we what when where
    which with without would you your xmlui howto how-to component components
    doc docs documentation example examples pattern patterns
    """.split()
)
TOKEN_ALIASES = {
    "hsplitter": "splitter",
    "vsplitter": "splitter",
    "parameter": "param",
    "parameters": "param",
    "params": "param",
}

REVERSAL_MARKERS = (
    "contradict",
    "reversed",
    "wrong conclusion",
    "no longer think",
    "not reproducible",
    "non-reproducible",
    "does not reproduce",
    "already covered",
    "gap was closed",
)
RUNTIME_MARKERS = (
    "not reliably available",
    "resolves to undefined",
    "is undefined",
    "silently abort",
    "silent no-op",
    "runtime gotcha",
    "engine bug",
    "needs a reproducer",
    "repro wall",
    "contract test",
)


@dataclasses.dataclass(frozen=True)
class Event:
    timestamp: dt.datetime
    tool: str
    text: str
    arguments: dict


@dataclasses.dataclass
class Episode:
    start: dt.datetime
    end: dt.datetime
    howto_queries: list[str]
    search_queries: list[str]
    components: list[str]
    example_queries: list[str]
    source_reads: list[str]
    howto_reads: list[str]
    terms: list[str]
    score: int
    occurrence_days: int = 1

    @property
    def rephrases(self) -> int:
        return max(0, len(dict.fromkeys(self.howto_queries)) - 1)

    @property
    def repeated_queries(self) -> int:
        return len(self.howto_queries) - len(dict.fromkeys(self.howto_queries))

@dataclasses.dataclass(frozen=True)
class HowtoDoc:
    path: str
    title: str
    tokens: frozenset[str]
    title_tokens: frozenset[str]


@dataclasses.dataclass(frozen=True)
class SearchSignal:
    """One schema-v2 `search_query` analytics record: the search OUTCOME."""

    timestamp: dt.datetime
    query: str
    confidence: str
    top_score: float
    score_gap: float
    title_match_count: int
    # salient_* fields (xmlui-mcp #12): match counts scoped to the query's
    # salient terms, so generic tokens (button, list, form) no longer inflate
    # them. `salient_title_match_count` falls back to the any-term
    # `title_match_count` for pre-#12 records that lack it.
    salient_title_match_count: int
    salient_content_match_count: int
    # term_coverage (xmlui-mcp #13): per-term (term, title_matches,
    # content_matches) for every kept token. Generic-immune, and the only
    # place the content-gap tell lives cleanly: a term with title=0 AND
    # content=0 is absent from the whole result set. Empty for pre-#13 records.
    term_coverage: tuple
    matched_file_count: int
    yielded_results: bool
    corpus_version: str

    def absent_terms(self) -> list[str]:
        """Query terms absent from every result (title=0 and content=0)."""
        return [
            term
            for term, title_matches, content_matches in self.term_coverage
            if title_matches == 0 and content_matches == 0
        ]


class SearchFailure(RuntimeError):
    pass


def parse_timestamp(value: str) -> dt.datetime:
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt.timezone.utc)
    return parsed


def tokenize(text: str) -> list[str]:
    words = re.findall(r"[A-Za-z0-9_$]+", text.lower())
    return [
        TOKEN_ALIASES.get(word.strip("$_"), word.strip("$_"))
        for word in words
        if len(word.strip("$_")) >= 2 and word.strip("$_") not in STOPWORDS
    ]


def event_text(tool: str, arguments: dict) -> str:
    if tool == "xmlui_component_docs":
        return str(arguments.get("component") or "")
    return str(arguments.get("query") or arguments.get("path") or "")


def load_events(path: Path, since: dt.datetime | None = None) -> list[Event]:
    events: list[Event] = []
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            try:
                record = json.loads(line)
            except (json.JSONDecodeError, TypeError):
                continue
            # A search emits both tool_invocation and search_query. Keeping the
            # invocation gives us one event plus its original arguments.
            if record.get("type") != "tool_invocation":
                continue
            tool = str(record.get("tool_name") or "")
            if tool not in FALLBACK_TOOLS:
                continue
            arguments = record.get("arguments")
            if not isinstance(arguments, dict):
                arguments = {}
            text = event_text(tool, arguments).strip()
            if not text:
                continue
            try:
                timestamp = parse_timestamp(str(record.get("timestamp") or ""))
            except ValueError:
                continue
            if since is not None and timestamp < since:
                continue
            events.append(Event(timestamp, tool, text, arguments))
    events.sort(key=lambda event: event.timestamp)
    return events


def is_howto_path(value: str) -> bool:
    normalized = value.lower().replace("\\", "/")
    return "howto/" in normalized or "/howto/" in normalized


def is_source_path(value: str) -> bool:
    normalized = value.lower().replace("\\", "/")
    if is_howto_path(normalized):
        return False
    return (
        "/src/" in normalized
        or normalized.startswith("src/")
        or normalized.endswith((".ts", ".tsx", ".js", ".jsx", ".xs"))
    )


def _topic_tokens(text: str) -> set[str]:
    return set(tokenize(text))


def _union(parent: list[int], left: int, right: int) -> None:
    def root(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    left_root = root(left)
    right_root = root(right)
    if left_root != right_root:
        parent[right_root] = left_root


def _root(parent: list[int], index: int) -> int:
    while parent[index] != index:
        parent[index] = parent[parent[index]]
        index = parent[index]
    return index


def nominate_terms(
    queries: Sequence[str],
    limit: int = 4,
    document_frequency: collections.Counter[str] | None = None,
) -> list[str]:
    per_query: collections.Counter[str] = collections.Counter()
    total: collections.Counter[str] = collections.Counter()
    first_seen: dict[str, int] = {}
    for query_index, query in enumerate(queries):
        tokens = tokenize(query)
        total.update(tokens)
        per_query.update(set(tokens))
        for token in tokens:
            first_seen.setdefault(token, query_index)
    def rank_key(token: str) -> tuple:
        return (
            -per_query[token],
            (document_frequency or {}).get(token, 0),
            -total[token],
            first_seen[token],
            token,
        )

    recurring = sorted(
        (token for token in total if per_query[token] >= 2),
        key=rank_key,
    )
    recurring = recurring[: max(0, limit - 1)]
    remaining = sorted(
        (token for token in total if token not in recurring),
        key=lambda token: (
            0 if token.startswith("on") and len(token) > 4 else 1,
            (document_frequency or {}).get(token, 0),
            -per_query[token],
            -total[token],
            first_seen[token],
            token,
        ),
    )
    ranked = recurring + remaining
    return ranked[:limit]


def build_episodes(
    events: Sequence[Event],
    topic_gap: dt.timedelta = dt.timedelta(minutes=20),
    fallback_window: dt.timedelta = dt.timedelta(minutes=10),
) -> list[Episode]:
    anchors = [event for event in events if event.tool == "xmlui_search_howto"]
    if not anchors:
        return []

    document_frequency: collections.Counter[str] = collections.Counter()
    for event in events:
        if event.tool in SEARCH_TOOLS:
            document_frequency.update(set(tokenize(event.text)))

    parent = list(range(len(anchors)))
    anchor_tokens = [_topic_tokens(anchor.text) for anchor in anchors]
    for left in range(len(anchors)):
        for right in range(left + 1, len(anchors)):
            distance = anchors[right].timestamp - anchors[left].timestamp
            overlap = anchor_tokens[left] & anchor_tokens[right]
            # A shared component name alone is too broad: several unrelated
            # APICall or List tasks can occur in one work session. Two shared
            # meaningful terms preserve reformulations without conflating all
            # work on the same component.
            union_size = len(anchor_tokens[left] | anchor_tokens[right])
            similarity = len(overlap) / union_size if union_size else 0
            nearby_match = distance <= topic_gap and len(overlap) >= 2
            recurring_match = (
                distance > topic_gap and len(overlap) >= 3 and similarity >= 0.5
            )
            if nearby_match or recurring_match:
                _union(parent, left, right)

    groups: dict[int, list[int]] = collections.defaultdict(list)
    for index in range(len(anchors)):
        groups[_root(parent, index)].append(index)

    group_events: dict[int, list[Event]] = {
        group: [anchors[index] for index in indexes]
        for group, indexes in groups.items()
    }
    group_tokens: dict[int, set[str]] = {
        group: set().union(*(anchor_tokens[index] for index in indexes))
        for group, indexes in groups.items()
    }

    anchor_ids = {id(anchor) for anchor in anchors}
    for event in events:
        if id(event) in anchor_ids:
            continue
        event_tokens = _topic_tokens(event.text)
        candidates: list[tuple[int, float, float, int]] = []
        for group, indexes in groups.items():
            nearest = min(
                abs((event.timestamp - anchors[index].timestamp).total_seconds())
                for index in indexes
            )
            if nearest > fallback_window.total_seconds():
                continue
            overlap = len(event_tokens & group_tokens[group])
            # Unnamed source reads often have little lexical overlap. Attach
            # only when they occur very close to an anchor; otherwise require
            # a shared topic term to avoid clock-only correlation.
            if overlap == 0 and nearest > 120:
                continue
            candidates.append((-overlap, nearest, -len(group_tokens[group]), group))
        if candidates:
            candidates.sort()
            group_events[candidates[0][3]].append(event)

    episodes: list[Episode] = []
    for group, attached in group_events.items():
        attached.sort(key=lambda event: event.timestamp)
        howto_queries = [
            event.text for event in attached if event.tool == "xmlui_search_howto"
        ]
        search_queries = [
            event.text for event in attached if event.tool in SEARCH_TOOLS
        ]
        components = [
            event.text for event in attached if event.tool == "xmlui_component_docs"
        ]
        example_queries = [
            event.text for event in attached if event.tool == "xmlui_examples"
        ]
        read_values = [
            event.text for event in attached if event.tool == "xmlui_read_file"
        ]
        source_reads = [value for value in read_values if is_source_path(value)]
        howto_reads = [value for value in read_values if is_howto_path(value)]
        unique_howto = len(dict.fromkeys(howto_queries))
        rephrases = max(0, unique_howto - 1)
        repeated = len(howto_queries) - unique_howto
        occurrence_days = len({
            event.timestamp.date()
            for event in attached
            if event.tool == "xmlui_search_howto"
        })
        score = (
            2 * min(rephrases, 4)
            + min(repeated, 2)
            + 3 * min(max(0, occurrence_days - 1), 3)
            + min(len(set(components)), 3)
            + min(len(set(example_queries)), 2)
            + 3 * min(len(set(source_reads)), 2)
            - 2 * min(len(set(howto_reads)), 4)
        )
        terms = nominate_terms(
            search_queries or howto_queries,
            document_frequency=document_frequency,
        )
        episode = Episode(
                start=attached[0].timestamp,
                end=attached[-1].timestamp,
                howto_queries=howto_queries,
                search_queries=search_queries,
                components=list(dict.fromkeys(components)),
                example_queries=list(dict.fromkeys(example_queries)),
                source_reads=list(dict.fromkeys(source_reads)),
                howto_reads=list(dict.fromkeys(howto_reads)),
                terms=terms,
                score=score,
                occurrence_days=occurrence_days,
            )
        episodes.append(episode)
    episodes.sort(key=lambda episode: (-episode.score, episode.start))
    return episodes


def load_howtos(directory: Path) -> list[HowtoDoc]:
    docs: list[HowtoDoc] = []
    if not directory.is_dir():
        return docs
    for path in sorted(directory.glob("*.md")):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        title_line = next(
            (line for line in text.splitlines() if line.startswith("# ")),
            f"# {path.stem.replace('-', ' ')}",
        )
        title = title_line[2:].strip()
        docs.append(
            HowtoDoc(
                str(path),
                title,
                frozenset(tokenize(text)),
                frozenset(tokenize(title + " " + path.stem)),
            )
        )
    return docs


def howto_matches(terms: Sequence[str], docs: Sequence[HowtoDoc]) -> list[dict]:
    wanted = set(terms)
    if not wanted:
        return []
    rows = []
    for doc in docs:
        if not (wanted & doc.title_tokens):
            continue
        matched = wanted & doc.tokens
        if not matched:
            continue
        ratio = len(matched) / len(wanted)
        threshold = 1 if len(wanted) == 1 else 2
        if len(matched) < threshold:
            continue
        rows.append(
            {
                "path": doc.path,
                "title": doc.title,
                "matched_terms": sorted(matched),
                "coverage": round(ratio, 3),
            }
        )
    rows.sort(key=lambda row: (-row["coverage"], row["title"].lower()))
    return rows[:5]


def search_endpoint(search_url: str | None, port_file: Path) -> str:
    if search_url:
        return search_url.rstrip("/")
    try:
        port = port_file.read_text(encoding="utf-8").strip()
    except OSError as error:
        raise SearchFailure(f"cannot read Bram port file {port_file}: {error}") from error
    if not port.isdigit():
        raise SearchFailure(f"invalid Bram port in {port_file}: {port!r}")
    return f"http://127.0.0.1:{port}/__search"


def query_search(
    endpoint: str,
    terms: Sequence[str],
    *,
    types: str = DEFAULT_TYPES,
    limit: int = 20,
    opener: Callable = urllib.request.urlopen,
) -> list[dict]:
    params = urllib.parse.urlencode(
        {
            "q": " ".join(terms),
            "mode": "and",
            "types": types,
            "limit": str(limit),
        }
    )
    url = f"{endpoint}?{params}"
    try:
        with opener(url, timeout=10) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except Exception as error:  # urllib exposes several unrelated subclasses
        raise SearchFailure(f"search request failed for {' '.join(terms)}: {error}") from error
    if not isinstance(payload, list):
        raise SearchFailure(f"search returned {type(payload).__name__}, expected a list")
    return [hit for hit in payload if isinstance(hit, dict)]


def query_search_ladder(
    endpoint: str,
    terms: Sequence[str],
    *,
    types: str = DEFAULT_TYPES,
    limit: int = 20,
    opener: Callable = urllib.request.urlopen,
) -> tuple[list[dict], list[str], list[str]]:
    """Try the compact AND query, dropping only its weakest tail terms."""
    attempts: list[str] = []
    minimum = min(2, len(terms))
    for size in range(len(terms), minimum - 1, -1):
        candidate = list(terms[:size])
        attempts.append(" ".join(candidate))
        hits = query_search(
            endpoint,
            candidate,
            types=types,
            limit=limit,
            opener=opener,
        )
        if hits:
            return hits, candidate, attempts
    return [], list(terms), attempts


def _evidence_text(hits: Sequence[dict]) -> str:
    text = " ".join(
        str(hit.get("snippet") or "") + " " + str(hit.get("source") or "")
        for hit in hits
    ).lower()
    # Bram brackets matched FTS terms in snippets. Remove only those display
    # markers so phrases such as "resolves to [undefined]" still trigger the
    # underlying durable-evidence marker.
    text = text.replace("[", "").replace("]", "")
    return " ".join(text.split())


def _marker_hit(hits: Sequence[dict], markers: Sequence[str]) -> tuple[str, dict] | None:
    durable = [hit for hit in hits if hit.get("type") != "session"]
    sessions = [hit for hit in hits if hit.get("type") == "session"]
    for hit in durable + sessions:
        evidence = _evidence_text([hit])
        for marker in markers:
            if marker in evidence:
                return marker, hit
    return None


def classify(
    episode: Episode,
    hits: Sequence[dict],
    matches: Sequence[dict],
    search_error: str | None = None,
) -> tuple[str, list[str]]:
    reasons: list[str] = []
    if search_error:
        return "unconfirmed", [search_error]
    if not hits:
        return "unconfirmed", ["SQLite search found no corroborating project-memory hit"]

    reversal = _marker_hit(hits, REVERSAL_MARKERS)
    if reversal:
        marker, hit = reversal
        return "contradicted-or-reversed", [
            f"indexed {hit.get('type', 'memory')} evidence contains reversal marker: {marker!r}"
        ]

    runtime = _marker_hit(hits, RUNTIME_MARKERS)
    if runtime:
        marker, hit = runtime
        return "runtime-contract-test", [
            f"indexed {hit.get('type', 'memory')} evidence contains runtime marker: {marker!r}",
            "verify current behavior before documenting a workaround",
        ]

    exact_match = next((match for match in matches if match["coverage"] >= 1.0), None)
    if exact_match:
        reasons.append(f"current how-to covers all nominated terms: {exact_match['title']}")
        if episode.howto_reads:
            reasons.append("the MCP episode opened a how-to")
            return "already-covered", reasons
        reasons.append("the episode did not open the matching how-to")
        return "discoverability-only", reasons

    reasons.append(f"SQLite search returned {len(hits)} corroborating hit(s)")
    if episode.rephrases:
        reasons.append(f"episode contains {episode.rephrases} query reformulation(s)")
    if episode.source_reads:
        reasons.append(f"episode fell back to {len(episode.source_reads)} source read(s)")
    if matches:
        reasons.append("current corpus has only partial term coverage")
    else:
        reasons.append("current corpus has no multi-term match")
    return "missing", reasons


def load_search_signals(
    path: Path, since: dt.datetime | None = None
) -> dict[str, SearchSignal]:
    """Latest schema-v2 `search_query` record per query text.

    Schema-v2 records carry `top_score`; older records don't, so that key gates
    membership. `dict` preserves insertion order and the last write wins, so a
    re-run of the same query keeps its most recent (post-fix) outcome.
    """
    out: dict[str, SearchSignal] = {}
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            try:
                record = json.loads(line)
            except (json.JSONDecodeError, TypeError):
                continue
            if record.get("type") != "search_query" or record.get("top_score") is None:
                continue
            query = str(record.get("query") or "").strip()
            if not query:
                continue
            try:
                timestamp = parse_timestamp(str(record.get("timestamp") or ""))
            except ValueError:
                continue
            if since is not None and timestamp < since:
                continue
            out[query] = SearchSignal(
                timestamp=timestamp,
                query=query,
                confidence=str(record.get("confidence") or ""),
                top_score=float(record.get("top_score") or 0.0),
                score_gap=float(record.get("score_gap") or 0.0),
                title_match_count=int(record.get("title_match_count") or 0),
                salient_title_match_count=int(
                    record.get("salient_title_match_count")
                    if record.get("salient_title_match_count") is not None
                    else (record.get("title_match_count") or 0)
                ),
                salient_content_match_count=int(
                    record.get("salient_content_match_count") or 0
                ),
                term_coverage=tuple(
                    (
                        str(entry.get("term") or ""),
                        int(entry.get("title_matches") or 0),
                        int(entry.get("content_matches") or 0),
                    )
                    for entry in (record.get("term_coverage") or [])
                    if isinstance(entry, dict)
                ),
                matched_file_count=int(record.get("matched_file_count") or 0),
                yielded_results=bool(record.get("yielded_results", True)),
                corpus_version=str(record.get("corpus_version") or ""),
            )
    return out


# signal_verdict thresholds, calibrated against the live analytics tape.
STANDOUT_TOP = 7.0  # a strong lexical top hit
STANDOUT_GAP = 3.0  # ...clearly ahead of the #2 result
WEAK_TOP = 2.5      # essentially nothing matched


def signal_verdict(signal: SearchSignal) -> tuple[bool, str, str]:
    """(is_gap_candidate, lean, note) from the recorded search outcome alone.

    Since xmlui-mcp #12 the record carries salient-term-scoped match counts, so
    the content-vs-discoverability split is now a record-level signal rather
    than pure guesswork: a salient term in a title or a non-trivially-ranked
    body means a relevant doc exists (discoverability); its absence everywhere
    means no doc exists (content).

    Primary content-gap detector (xmlui-mcp #13 `term_coverage`): a query term
    with title=0 AND content=0 is absent from the entire result set. This is
    generic-immune (a chrome word like "button" is never absent) and needs no
    corpus statistics, so it is the most reliable content-gap tell — it catches
    "restore scroll position" via `restore` being absent.

    Since xmlui-mcp #14 the aggregate `salient_*` counts are derived
    consistently with `term_coverage` (they no longer contradict it — the old
    single rarity-picked salient term is gone), so the finer discoverability
    split below is a near-verdict. The one residual the counts cannot resolve:
    an incidental body mention (e.g. a "clipboard" icon name, content=1) is
    indistinguishable from real coverage, so a case like "copy to the clipboard"
    still needs the top-hit URL to confirm. The lean stays a strong hint, not a
    hard verdict.
    """
    if signal.confidence == "high":
        return (
            False,
            "covered",
            "confidence=high — term-coverage guard confirmed a salient-term title match",
        )
    if not signal.yielded_results:
        return (True, "content", "search yielded no results")
    if signal.confidence == "low":
        # A high top_score on a low record is the off-salient filename-match
        # signature (e.g. "record voice input" -> set-width-for-input-fields);
        # the guard already decided no on-topic title exists, so never promote.
        return (
            True,
            "content",
            f"confidence=low — top={signal.top_score:.1f} is a non-salient match; "
            "the guard found no on-topic title",
        )
    # confidence == medium: guard withheld high, so the salient term is not in
    # the top title. None of these are confidently covered.
    if signal.top_score >= STANDOUT_TOP and signal.score_gap >= STANDOUT_GAP:
        return (
            True,
            "verify-standout",
            f"strong lexical top ({signal.top_score:.1f}, gap {signal.score_gap:.2f}) "
            "but guard withheld high — verify the top doc actually answers the query",
        )
    # term_coverage content-gap tell: a query term absent from every result.
    # Checked before the salient split because it is generic-immune and
    # independent of the salient aggregates — it stays right even where an
    # incidental body mention would otherwise read as coverage.
    absent = signal.absent_terms()
    if absent:
        return (
            True,
            "content",
            f"term_coverage: {', '.join(absent[:3])} absent from all results "
            "(title=0, content=0) — no doc covers this",
        )
    # An on-topic doc exists when a salient term is in a title, or in the body
    # of a doc that actually ranks (a trivially-weak body echo, top < WEAK_TOP,
    # is not a real match — that separates a genuine content gap like
    # "restore scroll position" from a poorly-titled real doc like fit-content).
    if signal.salient_title_match_count > 0:
        return (
            True,
            "discoverability",
            f"salient term in {signal.salient_title_match_count} title(s) but guard "
            "withheld high — a relevant doc exists; likely a ranking/titling gap",
        )
    if signal.salient_content_match_count > 0 and signal.top_score >= WEAK_TOP:
        return (
            True,
            "discoverability",
            f"salient term in {signal.salient_content_match_count} doc body(ies), none "
            f"in a title (top={signal.top_score:.1f}) — a relevant doc exists but is "
            "poorly titled; retitle/cross-link",
        )
    return (
        True,
        "content",
        f"no salient title match and no non-trivial body match "
        f"(top={signal.top_score:.1f}, salient_content={signal.salient_content_match_count}) "
        "— likely no doc on this topic",
    )


_LEAN_CLASSIFICATION = {
    "verify-standout": "needs-review",
    "discoverability": "discoverability-only",
    "content": "missing",
}


def classify_signal(
    signal: SearchSignal,
    lean: str,
    note: str,
    hits: Sequence[dict],
    matches: Sequence[dict],
    search_error: str | None = None,
) -> tuple[str, list[str]]:
    """Classification for a confidence-nominated query.

    The class comes from the record's `lean`, never from a local corpus
    re-rank: `howto_matches` on generic query tokens is noisier than the MCP's
    own ranking (it false-positives, e.g. virtualize -> find-in-page). Corpus
    matches and project hits are surfaced as evidence only. Project-memory
    reversal / runtime markers still override, matching the episode path.
    """
    if search_error:
        return _LEAN_CLASSIFICATION.get(lean, "missing"), [note, search_error]
    reasons = [note]
    reversal = _marker_hit(hits, REVERSAL_MARKERS)
    if reversal:
        marker, hit = reversal
        return "contradicted-or-reversed", [
            note,
            f"indexed {hit.get('type', 'memory')} evidence contains reversal marker: {marker!r}",
        ]
    runtime = _marker_hit(hits, RUNTIME_MARKERS)
    if runtime:
        marker, hit = runtime
        return "runtime-contract-test", [
            note,
            f"indexed {hit.get('type', 'memory')} evidence contains runtime marker: {marker!r}",
            "verify current behavior before documenting a workaround",
        ]
    if hits:
        reasons.append(f"SQLite search returned {len(hits)} corroborating project hit(s)")
    else:
        reasons.append("no corroborating project-memory hit (nominated from search outcome)")
    if matches:
        reasons.append(
            f"corpus candidate (title-token overlap, not a verdict): {matches[0]['title']}"
        )
    return _LEAN_CLASSIFICATION.get(lean, "missing"), reasons


def analyze_signals(
    signals: dict[str, SearchSignal],
    docs: Sequence[HowtoDoc],
    *,
    endpoint: str | None,
    types: str,
    search_limit: int,
    opener: Callable = urllib.request.urlopen,
) -> list[dict]:
    rows: list[dict] = []
    for query, signal in signals.items():
        is_gap, lean, note = signal_verdict(signal)
        if not is_gap:
            continue
        terms = nominate_terms([query])
        matches = howto_matches(terms, docs)
        hits: list[dict] = []
        used_terms = list(terms)
        search_attempts: list[str] = []
        error_text = None
        if endpoint:
            try:
                hits, used_terms, search_attempts = query_search_ladder(
                    endpoint, terms, types=types, limit=search_limit, opener=opener
                )
            except SearchFailure as error:
                error_text = str(error)
        else:
            error_text = "SQLite search disabled"
        classification, reasons = classify_signal(
            signal, lean, note, hits, matches, error_text
        )
        # Rank weaker matches (likelier gaps) first within a class; low
        # confidence adds urgency.
        score = int(max(0, round(10 - signal.top_score))) + (
            3 if signal.confidence == "low" else 0
        )
        rows.append(
            {
                "classification": classification,
                "reasons": reasons,
                "source": "confidence",
                "start": signal.timestamp.isoformat(),
                "end": signal.timestamp.isoformat(),
                "score": score,
                "occurrence_days": 1,
                "terms": terms,
                "search_query": " ".join(used_terms),
                "search_attempts": search_attempts,
                "signal": {
                    "query": query,
                    "confidence": signal.confidence,
                    "top_score": signal.top_score,
                    "score_gap": signal.score_gap,
                    "title_match_count": signal.title_match_count,
                    "salient_title_match_count": signal.salient_title_match_count,
                    "salient_content_match_count": signal.salient_content_match_count,
                    "absent_terms": signal.absent_terms(),
                    "matched_file_count": signal.matched_file_count,
                    "yielded_results": signal.yielded_results,
                    "lean": lean,
                    "note": note,
                },
                "behavior": {
                    "howto_queries": [query],
                    "rephrases": 0,
                    "repeated_queries": 0,
                    "components": [],
                    "example_queries": [],
                    "source_reads": [],
                    "howto_reads": [],
                },
                "howto_matches": matches,
                "search_hits": [
                    {
                        key: hit.get(key)
                        for key in ("type", "key", "source", "date", "link", "snippet")
                    }
                    for hit in hits
                ],
                "search_error": error_text,
            }
        )
    return rows


def analyze(
    episodes: Sequence[Episode],
    docs: Sequence[HowtoDoc],
    *,
    endpoint: str | None,
    types: str,
    search_limit: int,
    opener: Callable = urllib.request.urlopen,
) -> list[dict]:
    rows: list[dict] = []
    for episode in episodes:
        matches = howto_matches(episode.terms, docs)
        hits: list[dict] = []
        used_terms = list(episode.terms)
        search_attempts: list[str] = []
        error_text = None
        if endpoint:
            try:
                hits, used_terms, search_attempts = query_search_ladder(
                    endpoint,
                    episode.terms,
                    types=types,
                    limit=search_limit,
                    opener=opener,
                )
            except SearchFailure as error:
                error_text = str(error)
        else:
            error_text = "SQLite search disabled"
        classification, reasons = classify(episode, hits, matches, error_text)
        rows.append(
            {
                "classification": classification,
                "reasons": reasons,
                "start": episode.start.isoformat(),
                "end": episode.end.isoformat(),
                "score": episode.score,
                "occurrence_days": episode.occurrence_days,
                "terms": episode.terms,
                "search_query": " ".join(used_terms),
                "search_attempts": search_attempts,
                "behavior": {
                    "howto_queries": episode.howto_queries,
                    "rephrases": episode.rephrases,
                    "repeated_queries": episode.repeated_queries,
                    "components": episode.components,
                    "example_queries": episode.example_queries,
                    "source_reads": episode.source_reads,
                    "howto_reads": episode.howto_reads,
                },
                "howto_matches": matches,
                "search_hits": [
                    {
                        key: hit.get(key)
                        for key in ("type", "key", "source", "date", "link", "snippet")
                    }
                    for hit in hits
                ],
                "search_error": error_text,
            }
        )
    return rows


CLASSIFICATION_PRIORITY = {
    "runtime-contract-test": 0,
    "missing": 1,
    "needs-review": 2,
    "discoverability-only": 3,
    "contradicted-or-reversed": 4,
    "unconfirmed": 5,
    "already-covered": 6,
}


def sort_rows(rows: list[dict]) -> list[dict]:
    rows.sort(
        key=lambda row: (
            CLASSIFICATION_PRIORITY.get(row["classification"], 99),
            -row["score"],
            row["start"],
        )
    )
    return rows


def render_text(
    rows: Sequence[dict], metadata: dict, title: str = "XMLUI how-to gap miner"
) -> None:
    print(title)
    print(f"analytics: {metadata['analytics']}")
    print(f"how-tos:  {metadata['howto_dir']} ({metadata['howto_count']} docs)")
    print(f"search:   {metadata['search_endpoint'] or 'disabled'}")
    print(f"candidates: {len(rows)} reported")
    for index, row in enumerate(rows, 1):
        behavior = row["behavior"]
        print()
        range_text = row["start"][:10]
        if row["end"][:10] != row["start"][:10]:
            range_text += f"..{row['end'][:10]}"
        print(
            f"{index:2d}. {row['classification']}  score={row['score']}  "
            f"occurrences={row['occurrence_days']}  {range_text}"
        )
        print(f"    search: {row['search_query']}")
        print("    how-to queries:")
        for query in behavior["howto_queries"]:
            print(f"      - {query}")
        signals = [
            f"rephrases={behavior['rephrases']}",
            f"components={len(behavior['components'])}",
            f"examples={len(behavior['example_queries'])}",
            f"source_reads={len(behavior['source_reads'])}",
            f"howto_reads={len(behavior['howto_reads'])}",
            f"sqlite_hits={len(row['search_hits'])}",
        ]
        print(f"    behavior: {', '.join(signals)}")
        signal = row.get("signal")
        if signal:
            print(
                f"    signal: conf={signal['confidence']} top={signal['top_score']:.2f} "
                f"gap={signal['score_gap']:.2f} "
                f"salient_title={signal.get('salient_title_match_count', 0)} "
                f"salient_body={signal.get('salient_content_match_count', 0)} "
                f"absent={signal.get('absent_terms') or '-'} "
                f"[{signal['lean']}]"
            )
            print(f"    signal-note: {signal['note']}")
        for reason in row["reasons"]:
            print(f"    reason: {reason}")
        for match in row["howto_matches"][:3]:
            print(
                f"    current doc ({match['coverage']:.0%}): "
                f"{match['title']} [{match['path']}]"
            )
        for hit in row["search_hits"][:3]:
            snippet = " ".join(str(hit.get("snippet") or "").split())
            print(
                f"    {hit.get('type')}: {hit.get('source') or hit.get('key')} — "
                f"{snippet[:220]}"
            )


def parse_since(value: str | None) -> dt.datetime | None:
    if not value:
        return None
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt.timezone.utc)
    return parsed


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--analytics", type=Path, default=DEFAULT_ANALYTICS)
    parser.add_argument("--howto-dir", type=Path, default=DEFAULT_HOWTO_DIR)
    parser.add_argument("--port-file", type=Path, default=DEFAULT_PORT_FILE)
    parser.add_argument(
        "--search-url",
        help="full Bram /__search endpoint; defaults from resources/.bram-port",
    )
    parser.add_argument("--types", default=DEFAULT_TYPES)
    parser.add_argument("--since", help="ISO timestamp/date lower bound")
    parser.add_argument(
        "--source",
        choices=("episode", "confidence", "both"),
        default="both",
        help="nominate from behavioral episodes, confidence search outcomes, or both",
    )
    parser.add_argument("--gap-minutes", type=float, default=20)
    parser.add_argument("--fallback-minutes", type=float, default=10)
    parser.add_argument("--min-score", type=int, default=3)
    parser.add_argument("--top", type=int, default=20)
    parser.add_argument("--search-limit", type=int, default=20)
    parser.add_argument("--no-search", action="store_true")
    parser.add_argument("--json", action="store_true", dest="as_json")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    analytics = args.analytics.expanduser()
    howto_dir = args.howto_dir.expanduser()
    if not analytics.is_file():
        print(f"analytics log not found: {analytics}", file=sys.stderr)
        return 2
    try:
        since = parse_since(args.since)
    except ValueError as error:
        print(f"invalid --since value: {error}", file=sys.stderr)
        return 2
    docs = load_howtos(howto_dir)
    endpoint = None
    endpoint_error = None
    if not args.no_search:
        try:
            endpoint = search_endpoint(args.search_url, args.port_file)
        except SearchFailure as error:
            endpoint_error = str(error)

    source = args.source
    episode_rows: list[dict] = []
    confidence_rows: list[dict] = []
    episode_count = 0
    signal_count = 0

    if source in ("episode", "both"):
        events = load_events(analytics, since)
        episodes = build_episodes(
            events,
            topic_gap=dt.timedelta(minutes=args.gap_minutes),
            fallback_window=dt.timedelta(minutes=args.fallback_minutes),
        )
        episodes = [episode for episode in episodes if episode.score >= args.min_score]
        episodes = episodes[: max(0, args.top)]
        episode_count = len(episodes)
        episode_rows = analyze(
            episodes,
            docs,
            endpoint=endpoint,
            types=args.types,
            search_limit=args.search_limit,
        )
        if endpoint_error:
            for row in episode_rows:
                row["classification"] = "unconfirmed"
                row["search_error"] = endpoint_error
                row["reasons"] = [endpoint_error]
        sort_rows(episode_rows)

    if source in ("confidence", "both"):
        signals = load_search_signals(analytics, since)
        signal_count = len(signals)
        confidence_rows = analyze_signals(
            signals,
            docs,
            endpoint=endpoint,
            types=args.types,
            search_limit=args.search_limit,
        )
        if endpoint_error:
            for row in confidence_rows:
                row["search_error"] = endpoint_error
                row["reasons"] = row["reasons"] + [endpoint_error]
        sort_rows(confidence_rows)
        confidence_rows = confidence_rows[: max(0, args.top)]

    metadata = {
        "analytics": str(analytics),
        "howto_dir": str(howto_dir),
        "howto_count": len(docs),
        "search_endpoint": endpoint,
        "source": source,
        "episode_count": episode_count,
        "signal_count": signal_count,
        "method": "MCP outcome/behavior nomination -> Bram SQLite validation -> current-corpus reconciliation",
    }
    if args.as_json:
        print(
            json.dumps(
                {
                    "metadata": metadata,
                    "confidence_candidates": confidence_rows,
                    "episode_candidates": episode_rows,
                },
                indent=2,
            )
        )
    else:
        if source in ("confidence", "both"):
            render_text(
                confidence_rows,
                metadata,
                title="XMLUI how-to gap miner — confidence-nominated gaps",
            )
        if source in ("episode", "both"):
            if source == "both":
                print()
            render_text(
                episode_rows,
                metadata,
                title="XMLUI how-to gap miner — behavior-nominated episodes",
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
