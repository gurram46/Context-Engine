import json, pathlib
from pathlib import Path

def test_normalize():
    import sys
    sys.path.insert(0, str(Path(".").resolve()))
    from bench.d0.v2.run import _normalize_tool
    assert _normalize_tool("contextd_symbol_lookup") == "symbol_lookup"
    assert _normalize_tool("contextd_context_search") == "context_search"
    assert _normalize_tool("contextd_dependency_trace") == "dependency_trace"
    assert _normalize_tool("read") == "read"
    assert _normalize_tool("unknown_tool") == "unknown_tool"

def test_token_sums():
    import sys
    sys.path.insert(0, str(Path(".").resolve()))
    from bench.d0.v2.run import parse_metrics_exact
    sample = '{"type":"step_finish","part":{"tokens":{"input":100,"output":50,"cache":{"read":10,"write":5}}}}\n{"type":"step_finish","part":{"tokens":{"input":200,"output":70,"cache":{"read":20,"write":10}}}}\n{"type":"tool_use","part":{"tool":"contextd_symbol_lookup","state":{"status":"completed","output":"hello"}}}\n'
    parsed = parse_metrics_exact(Path("."), sample)
    assert parsed["input_tokens"] == 300, parsed["input_tokens"]
    assert parsed["output_tokens"] == 120
    assert parsed["tool_counts"]["symbol_lookup"] == 1
    assert parsed["tool_output_tokens_cl100k"] > 0

def test_patch_sha():
    import sys
    sys.path.insert(0, str(Path(".").resolve()))
    from bench.d0.v2.run import _verify_patch_sha
    manifest = json.loads(Path("bench/d0/v2/tasks/manifest.json").read_text())
    for task in manifest["tasks"]:
        assert _verify_patch_sha(task)

def test_leakage():
    import sys
    sys.path.insert(0, str(Path(".").resolve()))
    from bench.d0.v2.run import leakage_audit
    assert leakage_audit()

if __name__ == "__main__":
    test_normalize()
    print("norm PASS")
    test_token_sums()
    print("token PASS")
    test_patch_sha()
    print("patch PASS")
    test_leakage()
    print("leakage PASS")
    print("all PASS")
