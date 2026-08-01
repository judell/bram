import datetime as dt
import importlib.util
import json
import sys
import tempfile
import unittest
import urllib.parse
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "xmlui-howto-gap-miner.py"
SPEC = importlib.util.spec_from_file_location("xmlui_howto_gap_miner", SCRIPT)
miner = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = miner
SPEC.loader.exec_module(miner)


def event(minute, tool, text):
    arguments = {"component": text} if tool == "xmlui_component_docs" else {"query": text}
    if tool == "xmlui_read_file":
        arguments = {"path": text}
    return miner.Event(
        dt.datetime(2026, 7, 1, 12, minute, tzinfo=dt.timezone.utc),
        tool,
        text,
        arguments,
    )


def episode(**overrides):
    values = dict(
        start=dt.datetime(2026, 7, 1, tzinfo=dt.timezone.utc),
        end=dt.datetime(2026, 7, 1, 0, 5, tzinfo=dt.timezone.utc),
        howto_queries=["APICall execute parameter"],
        search_queries=["APICall execute parameter"],
        components=[],
        example_queries=[],
        source_reads=[],
        howto_reads=[],
        terms=["apicall", "execute", "parameter"],
        score=3,
    )
    values.update(overrides)
    return miner.Episode(**values)


class EpisodeTests(unittest.TestCase):
    def test_groups_shared_topics_but_splits_unrelated_nearby_queries(self):
        events = [
            event(0, "xmlui_search_howto", "HSplitter single visible child"),
            event(2, "xmlui_component_docs", "HSplitter"),
            event(4, "xmlui_search_howto", "Splitter hidden child full width"),
            event(5, "xmlui_read_file", "xmlui/src/components/Splitter/Splitter.tsx"),
            event(6, "xmlui_search_howto", "NavPanel footer custom controls"),
            event(7, "xmlui_component_docs", "NavPanel"),
        ]
        episodes = miner.build_episodes(events)
        self.assertEqual(len(episodes), 2)
        splitter = next(item for item in episodes if "splitter" in item.terms)
        nav = next(item for item in episodes if "navpanel" in item.terms)
        self.assertEqual(splitter.rephrases, 1)
        self.assertEqual(splitter.source_reads, ["xmlui/src/components/Splitter/Splitter.tsx"])
        self.assertEqual(nav.components, ["NavPanel"])

    def test_load_events_ignores_paired_search_query_and_result_count(self):
        records = [
            {
                "type": "tool_invocation",
                "timestamp": "2026-07-01T12:00:00Z",
                "tool_name": "xmlui_search_howto",
                "arguments": {"query": "preserve state across navigation"},
            },
            {
                "type": "search_query",
                "timestamp": "2026-07-01T12:00:00Z",
                "tool_name": "xmlui_search_howto",
                "query": "preserve state across navigation",
                "result_count": 999,
            },
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "analytics.jsonl"
            path.write_text("".join(json.dumps(row) + "\n" for row in records))
            loaded = miner.load_events(path)
        self.assertEqual(len(loaded), 1)
        self.assertEqual(loaded[0].text, "preserve state across navigation")

    def test_generic_long_term_does_not_chain_unrelated_topics(self):
        events = [
            event(0, "xmlui_search_howto", "persist state localStorage variable"),
            event(2, "xmlui_search_howto", "List variable height scroll container"),
            event(4, "xmlui_search_howto", "List scrollAnchor bottom scroll"),
        ]
        episodes = miner.build_episodes(events)
        self.assertEqual(len(episodes), 2)
        self.assertTrue(any("localstorage" in item.terms for item in episodes))

    def test_recurring_topic_consolidates_across_distant_dates(self):
        events = [
            miner.Event(
                dt.datetime(2026, 5, 1, tzinfo=dt.timezone.utc),
                "xmlui_search_howto",
                "APICall onSuccess execute params",
                {"query": "APICall onSuccess execute params"},
            ),
            miner.Event(
                dt.datetime(2026, 7, 1, tzinfo=dt.timezone.utc),
                "xmlui_search_howto",
                "APICall execute parameter request body",
                {"query": "APICall execute parameter request body"},
            ),
        ]
        episodes = miner.build_episodes(events)
        self.assertEqual(len(episodes), 1)
        self.assertEqual(episodes[0].occurrence_days, 2)
        self.assertGreaterEqual(episodes[0].score, 5)


class ClassificationTests(unittest.TestCase):
    def test_exact_current_doc_without_episode_read_is_discoverability(self):
        item = episode()
        matches = [{"title": "Pass arguments", "coverage": 1.0}]
        classification, reasons = miner.classify(
            item,
            [{"snippet": "A real project used this documented pattern."}],
            matches,
        )
        self.assertEqual(classification, "discoverability-only")
        self.assertTrue(any("did not open" in reason for reason in reasons))

    def test_current_doc_opened_is_already_covered(self):
        item = episode(howto_reads=["howto/pass-arguments.md"])
        classification, _ = miner.classify(
            item,
            [{"snippet": "The documented pattern worked."}],
            [{"title": "Pass arguments", "coverage": 1.0}],
        )
        self.assertEqual(classification, "already-covered")

    def test_later_reversal_wins_over_gap_classification(self):
        classification, _ = miner.classify(
            episode(),
            [{"snippet": "That was the wrong conclusion; the report was non-reproducible."}],
            [],
        )
        self.assertEqual(classification, "contradicted-or-reversed")

    def test_runtime_marker_requires_contract_test(self):
        classification, reasons = miner.classify(
            episode(),
            [
                {
                    "type": "commit",
                    "snippet": "$[param].id is not\nreliably available; it resolves to [undefined].",
                },
                {"type": "session", "snippet": "This needs a contract test."},
            ],
            [],
        )
        self.assertEqual(classification, "runtime-contract-test")
        self.assertTrue(any("verify current behavior" in reason for reason in reasons))
        self.assertTrue(any("commit" in reason for reason in reasons))

    def test_no_index_hits_is_unconfirmed_not_missing(self):
        classification, _ = miner.classify(episode(), [], [])
        self.assertEqual(classification, "unconfirmed")


class CorpusAndSearchTests(unittest.TestCase):
    def test_nomination_keeps_rare_discriminator_after_recurring_terms(self):
        frequencies = miner.collections.Counter(
            {
                "apicall": 100,
                "execute": 60,
                "param": 8,
                "body": 20,
                "onsuccess": 2,
                "request": 40,
            }
        )
        terms = miner.nominate_terms(
            [
                "APICall onSuccess execute params",
                "APICall execute parameter request body",
                "APICall body param execute",
            ],
            document_frequency=frequencies,
        )
        self.assertEqual(terms[:3], ["param", "execute", "apicall"])
        self.assertIn("onsuccess", terms)

    def test_howto_matching_requires_multi_term_coverage(self):
        docs = [
            miner.HowtoDoc(
                "splitter.md",
                "Create a split view",
                frozenset({"splitter", "resizable", "view"}),
                frozenset({"splitter", "resizable", "view"}),
            )
        ]
        matches = miner.howto_matches(["splitter", "single", "child"], docs)
        self.assertEqual(matches, [])

    def test_search_request_uses_and_mode_and_all_memory_types(self):
        captured = {}

        class Response:
            def __enter__(self):
                return self

            def __exit__(self, *_):
                return False

            def read(self):
                return b"[]"

        def opener(url, timeout):
            captured["url"] = url
            captured["timeout"] = timeout
            return Response()

        miner.query_search(
            "http://127.0.0.1:1234/__search",
            ["splitter", "single", "child"],
            opener=opener,
        )
        parsed = urllib.parse.urlparse(captured["url"])
        query = urllib.parse.parse_qs(parsed.query)
        self.assertEqual(query["q"], ["splitter single child"])
        self.assertEqual(query["mode"], ["and"])
        self.assertEqual(query["types"], [miner.DEFAULT_TYPES])
        self.assertEqual(captured["timeout"], 10)

    def test_search_ladder_drops_weak_tail_terms_until_it_finds_hits(self):
        seen = []

        class Response:
            def __init__(self, payload):
                self.payload = payload

            def __enter__(self):
                return self

            def __exit__(self, *_):
                return False

            def read(self):
                return json.dumps(self.payload).encode()

        def opener(url, timeout):
            query = urllib.parse.parse_qs(urllib.parse.urlparse(url).query)["q"][0]
            seen.append(query)
            return Response([] if len(seen) == 1 else [{"type": "session"}])

        hits, used, attempts = miner.query_search_ladder(
            "http://127.0.0.1:1234/__search",
            ["splitter", "single", "child", "hidden"],
            opener=opener,
        )
        self.assertEqual(used, ["splitter", "single", "child"])
        self.assertEqual(attempts, seen)
        self.assertEqual(hits, [{"type": "session"}])


if __name__ == "__main__":
    unittest.main()
