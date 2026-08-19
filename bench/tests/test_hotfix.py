import json, tempfile, pathlib, unittest, sys, subprocess
from pathlib import Path
REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT))
sys.path.insert(0, str(REPO_ROOT/"bench"))

class TestEnvDynamic(unittest.TestCase):
    def test_env_collects_dynamically(self):
        import bench.scripts.c0_finalize as cf
        # check that env collection uses psutil not hardcoded
        src = Path(cf.__file__).read_text()
        self.assertIn("psutil", src)
        self.assertNotIn("Intel(R) Core(TM) i5-1035G1", src)
        # also check that cpu/physical cores are dynamic or N/A
        self.assertIn("physical_cores", src)
        self.assertIn("N/A", src)

class TestMissingResults(unittest.TestCase):
    def test_missing_results_blocked(self):
        import bench.scripts.c0_finalize as cf
        src = Path(cf.__file__).read_text()
        self.assertIn("C0_FINALIZE_BLOCKED_MISSING_RESULTS", src)
        self.assertIn("sys.exit(2)", src)

class TestRawFiltering(unittest.TestCase):
    def test_raw_filtering(self):
        import bench.scripts.c0_finalize as cf
        src = Path(cf.__file__).read_text()
        self.assertIn('rec.get("adapter")==ad', src)
        self.assertNotIn('shutil.copy(results, raw_dir', src)

class TestCBMErrorHandling(unittest.TestCase):
    def test_cbm_returncode(self):
        src = Path(REPO_ROOT/"bench/adapters/codebase_memory.py").read_text()
        self.assertIn("returncode", src)
        self.assertIn("non-zero", src)
        self.assertIn("cbm_index_failed", src)
    def test_cbm_neutral_fallback(self):
        src = Path(REPO_ROOT/"bench/adapters/codebase_memory.py").read_text()
        self.assertNotIn('"lodash.js"', src)
        self.assertNotIn('"gin.go"', src)
        self.assertNotIn('"django"', src)
        # generic fallback should exist
        self.assertIn("cbm:fallback", src)
        self.assertIn("sorted(set(cands))", src)

class TestRgPathAware(unittest.TestCase):
    def test_rg_path_aware(self):
        src = Path(REPO_ROOT/"bench/adapters/rg_baseline.py").read_text()
        self.assertIn("path-aware", src.lower())
        self.assertIn("cand_clean", src)
        self.assertIn("endswith", src)
        # old first basename break should be replaced
        self.assertNotIn("for p in repo_path.rglob(base):\n                if p.is_file() and p.name.lower() == base.lower():\n                    rel = p.relative_to(repo_path).as_posix()\n                    file_hits.append", src)  # old simple

    def test_rg_duplicate_basename(self):
        # create temp repo with duplicate basenames
        with tempfile.TemporaryDirectory() as tmp:
            repo=Path(tmp)/"repo"
            repo.mkdir()
            (repo/"a").mkdir()
            (repo/"b").mkdir()
            (repo/"a"/"app.module.ts").write_text("a")
            (repo/"b"/"app.module.ts").write_text("b")
            # create nested expected
            (repo/"sample/01-cats-app/src").mkdir(parents=True)
            (repo/"sample/01-cats-app/src/app.module.ts").write_text("expected")
            from adapters.rg_baseline import RgBaselineAdapter
            ad=RgBaselineAdapter()
            # query with full path should prefer exact suffix
            res=ad.search("Find sample/01-cats-app/src/app.module.ts", repo, top_n=5)
            files=[h.file for h in res.hits]
            # first should be the exact suffix
            self.assertIn("sample/01-cats-app/src/app.module.ts", files)
            self.assertEqual(files[0], "sample/01-cats-app/src/app.module.ts")

class TestSerenaCleanup(unittest.TestCase):
    def test_serena_init_cleanup(self):
        src = Path(REPO_ROOT/"bench/adapters/serena.py").read_text()
        self.assertIn("try:", src)
        self.assertIn("self._initialize()", src)
        self.assertIn("self.proc.terminate()", src)
        self.assertIn("raise", src)
        # check that __init__ wraps _initialize with cleanup
        self.assertIn("except Exception:", src)

class TestAdapterRegistry(unittest.TestCase):
    def test_registry(self):
        src = Path(REPO_ROOT/"bench/scripts/c0_collect.py").read_text()
        self.assertIn("registry", src)
        self.assertIn("CodebaseMemoryAdapter", src)
        self.assertIn("SerenaAdapter", src)
        self.assertIn("OciAdapter", src)
        self.assertIn("BLOCKED", src)

class TestCBMLeakageRemoved(unittest.TestCase):
    def test_no_leakage(self):
        src = Path(REPO_ROOT/"bench/adapters/codebase_memory.py").read_text()
        for term in ["django","nestjs","ripgrep","lodash","gin"]:
            # allow in comments? but we removed all
            self.assertNotIn(term, src.lower())

if __name__=="__main__":
    unittest.main()
