import sys
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT_FOR_IMPORT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT_FOR_IMPORT))
sys.path.insert(0, str(REPO_ROOT_FOR_IMPORT / "bench"))

import bench.scripts.run as run
from adapters.interface import SearchResult, IndexingMetrics


class TestTimingHelper(unittest.TestCase):
    def test_timing_helper_six_fields_elapsed_ignored(self):
        cold = SearchResult(query="q", elapsed_ms=999, wall_ms=101, internal_ms=22)
        warm = SearchResult(query="q", elapsed_ms=999, wall_ms=202, internal_ms=33)
        one = SearchResult(query="q", elapsed_ms=999, wall_ms=303, internal_ms=44)
        rec = run._timing_record(cold, warm, one, neutral_query="Model", had_index_before=True, adapter="context_engine", repo="test", profile="official")
        self.assertEqual(rec["cold_wall_ms"], 101)
        self.assertEqual(rec["cold_internal_ms"], 22)
        self.assertEqual(rec["warm_wall_ms"], 202)
        self.assertEqual(rec["warm_internal_ms"], 33)
        self.assertEqual(rec["one_file_wall_ms"], 303)
        self.assertEqual(rec["one_file_internal_ms"], 44)
        for k in ["cold_wall_ms", "cold_internal_ms", "warm_wall_ms", "warm_internal_ms", "one_file_wall_ms", "one_file_internal_ms"]:
            self.assertIn(k, rec)
            self.assertNotEqual(rec[k], 999, f"{k} must not be elapsed_ms")
        self.assertNotIn("cold_first_search_wall_ms", rec)
        self.assertNotIn("warm_no_change_wall_ms", rec)
        self.assertNotIn("one_file_change_wall_ms", rec)
        self.assertEqual(rec["type"], "timing")
        self.assertEqual(rec["neutral_query"], "Model")
        self.assertEqual(rec["had_index_before"], True)
        self.assertEqual(rec["adapter"], "context_engine")
        self.assertEqual(rec["repo"], "test")
        self.assertEqual(rec["profile"], "official")
        # None handling
        none_cold = SearchResult(query="q", elapsed_ms=999, wall_ms=None, internal_ms=None)
        none_warm = SearchResult(query="q", elapsed_ms=999, wall_ms=None, internal_ms=None)
        none_one = SearchResult(query="q", elapsed_ms=999, wall_ms=None, internal_ms=None)
        rec_none = run._timing_record(none_cold, none_warm, none_one)
        self.assertIsNone(rec_none["cold_wall_ms"])
        self.assertIsNone(rec_none["cold_internal_ms"])
        self.assertIsNone(rec_none["warm_wall_ms"])
        self.assertIsNone(rec_none["warm_internal_ms"])
        self.assertIsNone(rec_none["one_file_wall_ms"])
        self.assertIsNone(rec_none["one_file_internal_ms"])


class TestSmokeIntegration(unittest.TestCase):
    def test_main_smoke_ensure_before_profile_and_official_cleanup(self):
        expected_django = (
            "# bench smoke profile: ignore large vendor/static for fast local iteration (NEVER for official)\n"
            "django/contrib/admin/static/**\n"
            "docs/**\n"
            "django/contrib/gis/**\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            src = tmp_path / "src_repo"
            src.mkdir()
            subprocess.run(["git", "init"], cwd=src, check=True, capture_output=True)
            subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=src, check=True)
            subprocess.run(["git", "config", "user.name", "Test"], cwd=src, check=True)
            (src / "README.md").write_text("# test\n", encoding="utf-8")
            (src / "file.txt").write_text("hello", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=src, check=True, capture_output=True)
            subprocess.run(["git", "commit", "-m", "init"], cwd=src, check=True, capture_output=True)
            commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=src, text=True).strip()

            questions_dir = tmp_path / "bench" / "questions"
            questions_dir.mkdir(parents=True, exist_ok=True)
            manifest_path = tmp_path / "bench" / "manifest.json"
            manifest_path.parent.mkdir(parents=True, exist_ok=True)
            results_dir = tmp_path / "bench" / "results"

            q = {"id": "q1", "repo": "django", "category": "test", "query": "hello", "expected_files": ["file.txt"]}
            (questions_dir / "q.jsonl").write_text(json.dumps(q) + "\n", encoding="utf-8")
            manifest = {"repos": [{"name": "django", "url": str(src), "commit": commit}]}
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            orig_root = run.REPO_ROOT
            orig_qdir = run.QUESTIONS_DIR
            orig_rdir = run.RESULTS_DIR
            orig_manifest = run.MANIFEST
            orig_results_jsonl = run.RESULTS_JSONL
            orig_adapters = dict(run.ADAPTERS)
            orig_argv = sys.argv[:]
            orig_env = os.environ.get("CONTEXT_BENCH_TIMING")
            orig_results_md = run.SUMMARY_MD
            try:
                run.REPO_ROOT = tmp_path
                run.QUESTIONS_DIR = questions_dir
                run.RESULTS_DIR = results_dir
                run.MANIFEST = manifest_path
                run.RESULTS_JSONL = results_dir / "results.jsonl"
                run.SUMMARY_MD = results_dir / "summary.md"

                class FakeAdapter:
                    name = "fake"
                    def index(self, repo_path: Path):
                        return IndexingMetrics(files_indexed=1)
                    def search(self, query, repo_path, top_n=5):
                        return SearchResult(query=query, hits=[], wall_ms=10, internal_ms=5, elapsed_ms=999, candidate_count=0, evidence_count=0, files_returned=0, candidate_tokens=0, packed_tokens=0)

                run.ADAPTERS = {"fake": FakeAdapter}
                if "CONTEXT_BENCH_TIMING" in os.environ:
                    del os.environ["CONTEXT_BENCH_TIMING"]

                dest = tmp_path / "bench" / "repos" / "django"
                self.assertFalse(dest.exists())

                output_path = results_dir / "results.jsonl"

                sys.argv = ["run.py", "--adapters", "fake", "--repos", "django", "--profile", "smoke", "--manifest", str(manifest_path), "--output", str(output_path), "--top-n", "5"]
                run.main()

                self.assertTrue(dest.exists())
                ignore_path = dest / ".ignore"
                self.assertTrue(ignore_path.exists())
                actual = ignore_path.read_text(encoding="utf-8")
                self.assertEqual(actual, expected_django)

                # rerun smoke idempotent
                before = actual
                sys.argv = ["run.py", "--adapters", "fake", "--repos", "django", "--profile", "smoke", "--manifest", str(manifest_path), "--output", str(output_path)]
                run.main()
                after = ignore_path.read_text(encoding="utf-8")
                self.assertEqual(after, before)
                self.assertEqual(after, expected_django)

                # sentinel .context/index
                ctx_index = dest / ".context" / "index"
                ctx_index.mkdir(parents=True, exist_ok=True)
                sentinel = ctx_index / "sentinel.txt"
                sentinel.write_text("sentinel", encoding="utf-8")
                self.assertTrue(ctx_index.exists())
                self.assertTrue(sentinel.exists())

                # official should remove bench ignore and clean sentinel
                sys.argv = ["run.py", "--adapters", "fake", "--repos", "django", "--profile", "official", "--manifest", str(manifest_path), "--output", str(output_path)]
                run.main()
                self.assertFalse(ignore_path.exists())
                self.assertFalse(ctx_index.exists())
                self.assertTrue(dest.exists())
            finally:
                run.REPO_ROOT = orig_root
                run.QUESTIONS_DIR = orig_qdir
                run.RESULTS_DIR = orig_rdir
                run.MANIFEST = orig_manifest
                run.RESULTS_JSONL = orig_results_jsonl
                run.SUMMARY_MD = orig_results_md
                run.ADAPTERS = orig_adapters
                sys.argv = orig_argv
                if orig_env is not None:
                    os.environ["CONTEXT_BENCH_TIMING"] = orig_env
                elif "CONTEXT_BENCH_TIMING" in os.environ:
                    del os.environ["CONTEXT_BENCH_TIMING"]


if __name__ == "__main__":
    unittest.main()
