"""Utility functions for Context Engine"""

import hashlib
import json
import math
import re
from pathlib import Path
from typing import Dict, List, Optional
from datetime import datetime

import tiktoken

def calculate_file_hash(file_path: Path) -> str:
    """Calculate SHA256 hash of a file"""
    sha256_hash = hashlib.sha256()
    with open(file_path, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256_hash.update(byte_block)
    return sha256_hash.hexdigest()

def load_hashes(hashes_file: Path) -> Dict[str, Dict]:
    """Load file hashes from storage"""
    if not hashes_file.exists():
        return {}
    try:
        with open(hashes_file, 'r') as f:
            return json.load(f)
    except (json.JSONDecodeError, IOError):
        return {}

def save_hashes(hashes_file: Path, hashes: Dict[str, Dict]) -> None:
    """Save file hashes to storage"""
    hashes_file.parent.mkdir(parents=True, exist_ok=True)
    with open(hashes_file, 'w') as f:
        json.dump(hashes, f, indent=2)

def check_staleness(file_path: Path, stored_hashes: Dict[str, Dict]) -> bool:
    """Check if a file has changed since last hash"""
    str_path = str(file_path)
    if str_path not in stored_hashes:
        return False
    
    current_hash = calculate_file_hash(file_path)
    return stored_hashes[str_path].get("hash") != current_hash

def update_hash(file_path: Path, stored_hashes: Dict[str, Dict]) -> None:
    """Update hash for a file"""
    str_path = str(file_path)
    stored_hashes[str_path] = {
        "hash": calculate_file_hash(file_path),
        "updated": datetime.now().isoformat()
    }

def count_tokens(text: str, model: str = "gpt-4") -> int:
    """Count tokens in text using tiktoken"""
    try:
        encoding = tiktoken.encoding_for_model(model)
    except KeyError:
        encoding = tiktoken.get_encoding("cl100k_base")
    return len(encoding.encode(text))

def _shannon_entropy(s: str) -> float:
    """Compute Shannon entropy of a string"""
    if not s:
        return 0.0
    prob = [float(s.count(c)) / len(s) for c in set(s)]
    return -sum(p * math.log(p, 2) for p in prob)

def is_high_entropy_token(token: str) -> bool:
    """Heuristic to detect likely secrets by entropy and length"""
    token = token.strip().strip('"\'')
    if len(token) < 20:
        return False
    entropy = _shannon_entropy(token)
    return entropy >= 3.5  # heuristic threshold

def redact_secrets(text: str) -> str:
    """Redact potential secrets from text using regex and entropy detection"""
    # First, handle specific known secret patterns (order matters!)
    # Match sk- keys with or without quotes
    text = re.sub(r'="(sk-[A-Za-z0-9\-_]{20,})"', r'="[REDACTED_KEY]"', text)
    text = re.sub(r"='(sk-[A-Za-z0-9\-_]{20,})'", r"='[REDACTED_KEY]'", text)
    text = re.sub(r'=(sk-[A-Za-z0-9\-_]{20,})(?=\s|$)', r'=[REDACTED_KEY]', text)
    text = re.sub(r'"(sk-[A-Za-z0-9\-_]{20,})"', r'"[REDACTED_KEY]"', text)
    
    # AWS keys
    text = re.sub(r'(["\']?)(AKIA[0-9A-Z]{16})(["\']?)', r'\1[REDACTED_AWS]\3', text)
    
    # Generic patterns for other secrets
    text = re.sub(r'(password|passwd|pwd|pass)\s*[=:]\s*["\']?([^"\'\s]+)["\']?', r'\1=[REDACTED]', text, flags=re.IGNORECASE)
    text = re.sub(r'(token|jwt|bearer)\s*[=:]\s*["\']?([^"\'\s]+)["\']?', r'\1=[REDACTED]', text, flags=re.IGNORECASE)
    
    # Environment variables - skip if already redacted
    if '[REDACTED_KEY]' not in text:
        text = re.sub(r'(API_KEY|SECRET|TOKEN|PASSWORD|PASSWD)\s*=\s*["\']?([^"\'\s]+)["\']?', r'\1=[REDACTED]', text)

    # Entropy-based redaction only for standalone hex-like strings (not variable names)
    def _mask_high_entropy(match: re.Match) -> str:
        token = match.group(0)
        # Skip if it looks like a variable name (contains underscores in middle)
        if '_' in token[1:-1] and not token.startswith('sk-'):
            return token
        return "[REDACTED]" if is_high_entropy_token(token) else token

    # Only match hex-like strings, not typical variable names
    text = re.sub(r'\b[a-fA-F0-9]{32,}\b', _mask_high_entropy, text)
    return text

def strip_comments(code: str, language: str = "python") -> str:
    """Strip inline comments from code while preserving docstrings"""
    if language in ["python", "py"]:
        # Remove single-line comments but keep docstrings
        lines = code.split('\n')
        result = []
        in_docstring = False
        docstring_char = None
        
        for line in lines:
            stripped = line.strip()
            
            # Check for docstring start/end
            if '"""' in line or "'''" in line:
                if not in_docstring:
                    in_docstring = True
                    docstring_char = '"""' if '"""' in line else "'''"
                elif docstring_char in line:
                    in_docstring = False
                    docstring_char = None
                result.append(line)
            elif in_docstring:
                result.append(line)
            else:
                # Remove inline comments
                if '#' in line:
                    code_part = line.split('#')[0].rstrip()
                    if code_part:
                        result.append(code_part)
                    elif not code_part and line.strip().startswith('#'):
                        continue
                else:
                    result.append(line)
        
        return '\n'.join(result)
    
    elif language in ["javascript", "js", "typescript", "ts", "java", "c", "cpp"]:
        # Remove // comments and /* */ comments
        # Keep /** */ documentation comments
        # Preserve JSDoc-style comments
        jsdoc_blocks = re.findall(r'/\*\*[^*]*\*+(?:[^/*][^*]*\*+)*/', code, flags=re.DOTALL)
        code = re.sub(r'//.*$', '', code, flags=re.MULTILINE)
        code = re.sub(r'/\*(?!\*)[^*]*\*+(?:[^/*][^*]*\*+)*/', '', code)
        # Reattach JSDoc blocks at the top to preserve docstrings
        return "\n".join([b for b in jsdoc_blocks if b.strip()])
    
    return code

def summarize_config(config_text: str) -> str:
    """Summarize configuration file without secrets"""
    config_text = redact_secrets(config_text)
    
    lines = config_text.split('\n')
    summary = []
    
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith('#'):
            continue
        
        # Keep structure indicators
        if any(char in stripped for char in ['{', '}', '[', ']']):
            summary.append(line)
        # Summarize value lines
        elif '=' in stripped or ':' in stripped:
            key_part = stripped.split('=' if '=' in stripped else ':')[0].strip()
            summary.append(f"{key_part}: [configured]")
    
    return '\n'.join(summary)

def deduplicate_content(content: str) -> str:
    """Remove duplicate patterns from content"""
    lines = content.split('\n')
    seen = set()
    result = []
    
    for line in lines:
        stripped = line.strip()
        if stripped and stripped not in seen:
            seen.add(stripped)
            result.append(line)
        elif not stripped:
            # keep single blank lines only
            if result and result[-1].strip() == "":
                continue
            result.append("")
    
    return '\n'.join(result)

def compress_whitespace(text: str) -> str:
    """Remove excessive blank lines and trailing whitespace"""
    lines = [l.rstrip() for l in text.split('\n')]
    comp = []
    for l in lines:
        if l.strip() == "":
            if comp and comp[-1] == "":
                continue
            comp.append("")
        else:
            comp.append(l)
    return '\n'.join(comp)

def is_subpath(child: Path, parent: Path) -> bool:
    """Check if child path is within parent directory"""
    try:
        child = child.resolve(strict=False)
        parent = parent.resolve(strict=False)
        # Handle Windows paths properly
        return child == parent or parent in child.parents
    except Exception:
        return False

def validate_path_in_project(path: Path, project_root: Path) -> None:
    """Raise click.BadParameter if path escapes project root"""
    from click import BadParameter
    if not is_subpath(path, project_root):
        raise BadParameter(f"Path '{path}' is outside the project root: {project_root}")

def sanitize_note_input(note: str, max_len: int = 2000) -> str:
    """Sanitize note content and enforce max length"""
    # remove control characters except common whitespace
    note = re.sub(r'[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]', '', note)
    if len(note) > max_len:
        note = note[:max_len] + "…"
    return note

def extract_api_docstrings(code: str, language: str = "python") -> str:
    """Extract only API docstrings/comments and signatures, not raw code"""
    if language in ["python", "py"]:
        # Keep only triple-quoted docstrings
        blocks = re.findall(r'("""[\s\S]*?"""|\'\'\'[\s\S]*?\'\'\')', code)
        return "\n\n".join(b.strip() for b in blocks if b.strip()) or "(no docstrings)"
    elif language in ["javascript", "js", "typescript", "ts"]:
        blocks = re.findall(r'/\*\*[^*]*\*+(?:[^/*][^*]*\*+)*/', code, flags=re.DOTALL)
        return "\n\n".join(b.strip() for b in blocks if b.strip()) or "(no API docs)"
    else:
        return "(no API docs)"

def compress_code(code: str, language: str = "python") -> str:
    """Strict compression: Keep docstrings only, remove comments/whitespace"""
    doc_only = extract_api_docstrings(code, language)
    doc_only = compress_whitespace(doc_only)
    return doc_only

def is_valid_api_key(key: str) -> bool:
    """Basic format validation for API key (never log key)"""
    if not key or not isinstance(key, str):
        return False
    key = key.strip()
    # Accept keys like sk-... with proper format
    if key.startswith("sk-"):
        if len(key) >= 32:
            # Ensure no special chars except dash and underscore  
            return bool(re.match(r'^sk-[A-Za-z0-9_\-]+$', key))
        return False
    # Test keys starting with "test-" are invalid
    if key.startswith("test-"):
        return False
    # For non-sk keys, require only alphanumeric and simple chars
    if len(key) >= 32:
        # Reject if contains special chars like @ # $
        if re.search(r'[@#$%^&*()+=\[\]{};:\'"<>,?/\\|`~]', key):
            return False
        return bool(re.match(r'^[A-Za-z0-9_\-]+$', key))
    return False
