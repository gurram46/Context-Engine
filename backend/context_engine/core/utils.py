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
    # Store original text to avoid duplicate processing
    original_text = text

    # 1. Handle OpenAI API keys (sk- format) - most specific first
    sk_patterns = [
        (r'="(sk-[A-Za-z0-9\-_]{20,})"', r'="[REDACTED_KEY]"'),
        (r"='(sk-[A-Za-z0-9\-_]{20,})'", r"'[REDACTED_KEY]'"),
        (r'^\s*(sk-[A-Za-z0-9\-_]{20,})\s*$', r'[REDACTED_KEY]'),  # Standalone sk- keys
        (r':\s*"(sk-[A-Za-z0-9\-_]{20,})"', r': "[REDACTED_KEY]"'),
        (r":\s*'(sk-[A-Za-z0-9\-_]{20,})'", r": '[REDACTED_KEY]'"),
        (r'=\s*(sk-[A-Za-z0-9\-_]{20,})(?=\s|$)', r'=[REDACTED_KEY]'),  # Assignment
        (r'\b(sk-[A-Za-z0-9\-_]{20,})\b', r'[REDACTED_KEY]')  # In natural text
    ]
    for pattern, replacement in sk_patterns:
        text = re.sub(pattern, replacement, text)
    # Special handling for quoted sk- keys - remove quotes around the redacted value
    text = re.sub(r'\b(API_KEY)\s*=\s*"\[REDACTED_KEY\]"', r'\1 = [REDACTED_KEY]', text)
    text = re.sub(r'\b(API_KEY)\s*=\s*\'\[REDACTED_KEY\]\'', r'\1 = [REDACTED_KEY]', text)
    text = re.sub(r'=\s*"?\[REDACTED_KEY\]"?\s*$', r' = [REDACTED_KEY]', text)
    text = re.sub(r'=\s*\'?\[REDACTED_KEY\]\'?\s*$', r' = [REDACTED_KEY]', text)

    # 2. Handle AWS keys
    text = re.sub(r'(AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY)\s*([=:])\s*["\']?(AKIA[0-9A-Z]{16}|[A-Za-z0-9+/]{40})["\']?',
                  r'\1 \2 [REDACTED_AWS]', text, flags=re.IGNORECASE)
    text = re.sub(r'(["\']?)(AKIA[0-9A-Z]{16})(["\']?)', r'\1[REDACTED_AWS]\3', text)

    # 3. Handle Bearer tokens in various formats
    bearer_patterns = [
        r'Authorization:\s*Bearer\s+([A-Za-z0-9_\-\.]{20,})',
        r'bearer:\s*["\']?([A-Za-z0-9_\-\.]{20,})["\']?',
        r'Bearer\s+([A-Za-z0-9_\-\.]{20,})',
        r'\bbearer[_\s-]*([A-Za-z0-9_\-\.]{20,})\b',
        r'\b(bearer[a-z0-9_]{10,})\b',  # Handles cases like "bearer_token_1234567890"
    ]
    for pattern in bearer_patterns:
        text = re.sub(pattern, 'Bearer [REDACTED]', text, flags=re.IGNORECASE)

    # 4. Handle generic password patterns
    text = re.sub(r'(password|passwd|pwd|pass|secret|private_key|privatekey)\s*([=:])\s*["\']?([^"\'\s]{6,})["\']?',
                  r'\1 \2 [REDACTED]', text, flags=re.IGNORECASE)
    text = re.sub(r'(DB_PASSWORD|DATABASE_PASSWORD|USER_PASSWORD|ADMIN_PASSWORD)\s*([=:])\s*["\']?([^"\'\s]{6,})["\']?',
                  r'\1 \2 [REDACTED]', text, flags=re.IGNORECASE)
    text = re.sub(r'\b\w*(?:password|passwd|pwd|pass|secret)\w*\s+[A-Za-z0-9_]{8,}\b',
                  r'password = [REDACTED]', text, flags=re.IGNORECASE)

    # 5. Handle JWT and API tokens
    text = re.sub(r'(jwt|token|auth_token|access_token|refresh_token|session_token)\s*[=:]\s*["\']?([A-Za-z0-9_\-\.]{20,})["\']?',
                  r'token=[REDACTED]', text, flags=re.IGNORECASE)
    text = re.sub(r'["\']([a-zA-Z_]*token[a-zA-Z_]*|auth_token|access_token)["\']:\s*["\']([A-Za-z0-9_\-\.]{20,})["\']',
                  r'"\1": "[REDACTED]"', text, flags=re.IGNORECASE)
    text = re.sub(r'\b(?:jwt|token)[_\s-]*([A-Za-z0-9_\-\.]{20,})\b',
                  r'token=[REDACTED]', text, flags=re.IGNORECASE)

    # 6. Handle environment variable patterns in ${VAR} and $VAR format
    env_var_patterns = [
        (r'\$\{?([A-Z_]*(?:SECRET|TOKEN|PASSWORD|PASSWD|KEY|WEBHOOK|DB|DATABASE)[A-Z_]*)\}?', '[REDACTED]'),
    ]
    for pattern, replacement in env_var_patterns:
        text = re.sub(pattern, replacement, text)

    # 7. Handle specific high-value patterns
    text = re.sub(r'(JWT_SECRET|SECRET_KEY|API_SECRET|AUTH_SECRET)\s*([=:])\s*["\']?([^"\'\s]{10,})["\']?',
                  r'\1 \2 [REDACTED]', text, flags=re.IGNORECASE)
    # API_KEY handling - be careful not to override sk- keys
    if '[REDACTED_KEY]' not in text:
        text = re.sub(r'(API_KEY)\s*([=:])\s*["\']?([^"\'\s]{20,})["\']?',
                      r'\1 \2 [REDACTED]', text, flags=re.IGNORECASE)
    text = re.sub(r'(DATABASE_URL|MONGODB_URI|REDIS_URL)\s*([=:])\s*["\']?([^"\'\s]{10,})["\']?',
                  r'\1 \2 [REDACTED]', text, flags=re.IGNORECASE)

    # 8. Entropy-based redaction for hex-like strings (last resort)
    def _mask_high_entropy(match: re.Match) -> str:
        token = match.group(0)
        # Skip common non-secret patterns
        skip_patterns = [
            r'^[a-fA-F0-9]{8}$',  # 8-char hex (likely hash or ID)
            r'^[0-9a-fA-F]{32}$',  # 32-char hex (MD5 hash)
            r'^[0-9a-fA-F]{40}$',  # 40-char hex (SHA1)
            r'^[0-9a-fA-F]{64}$',  # 64-char hex (SHA256)
        ]
        for skip_pat in skip_patterns:
            if re.match(skip_pat, token):
                return token

        # Skip if it looks like a variable name
        if '_' in token[1:-1] and not token.startswith('sk-'):
            return token

        return "[REDACTED]" if is_high_entropy_token(token) else token

    # Only match hex-like strings that aren't obviously hashes
    text = re.sub(r'\b[a-fA-F0-9]{40,}\b', _mask_high_entropy, text)

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
        # Extract class and function definitions with their docstrings
        lines = code.split('\n')
        result = []
        i = 0

        # First, look for module-level docstring
        module_docstring = _find_docstring(lines, 0)
        if module_docstring:
            result.append(module_docstring)
            result.append("")

        while i < len(lines):
            line = lines[i].strip()

            # Look for class definitions
            if line.startswith('class '):
                class_match = re.match(r'class\s+(\w+)', line)
                if class_match:
                    class_name = class_match.group(1)
                    result.append(f"class {class_name}:")
                    # Look for docstring in next few lines
                    docstring = _find_docstring(lines, i + 1)
                    if docstring:
                        result.append(docstring)
                    result.append("")

            # Look for function definitions
            elif line.startswith('def '):
                func_match = re.match(r'def\s+(\w+)\s*\(', line)
                if func_match:
                    func_name = func_match.group(1)
                    # Get the full function signature
                    func_line = line
                    # Look for closing parenthesis
                    j = i
                    while j < len(lines) and '(' in func_line and ')' not in func_line:
                        j += 1
                        if j < len(lines):
                            func_line += " " + lines[j].strip()

                    result.append(f"def {func_name}(...):")
                    # Look for docstring in next few lines
                    docstring = _find_docstring(lines, j + 1)
                    if docstring:
                        result.append(docstring)
                    result.append("")

            i += 1

        return "\n".join(result).strip() or "(no docstrings)"

    elif language in ["javascript", "js", "typescript", "ts"]:
        # Extract function declarations and JSDoc comments
        lines = code.split('\n')
        result = []
        i = 0

        while i < len(lines):
            line = lines[i].strip()

            # Look for JSDoc comments
            if line.startswith('/**'):
                jsdoc_comment = line
                j = i + 1
                while j < len(lines) and not lines[j].strip().endswith('*/'):
                    jsdoc_comment += "\n" + lines[j]
                    j += 1
                if j < len(lines):
                    jsdoc_comment += "\n" + lines[j]
                result.append(jsdoc_comment)

                # Look for function declaration after JSDoc
                k = j + 1
                while k < len(lines) and not lines[k].strip():
                    k += 1
                if k < len(lines):
                    func_line = lines[k].strip()
                    if func_line.startswith('function ') or 'function ' in func_line:
                        result.append(f"// {func_line}")

                result.append("")

            # Look for function declarations without JSDoc
            elif line.startswith('function '):
                func_match = re.match(r'function\s+(\w+)', line)
                if func_match:
                    func_name = func_match.group(1)
                    result.append(f"function {func_name}(...) {{")
                    result.append("")

            i += 1

        return "\n".join(result).strip() or "(no API docs)"

    else:
        return "(no API docs)"


def _find_docstring(lines: list, start_idx: int) -> str:
    """Find docstring starting from given line index"""
    i = start_idx
    while i < len(lines):
        line = lines[i].strip()
        if line.startswith('"""') or line.startswith("'''"):
            # Found docstring
            docstring = line
            if not (line.endswith('"""') or line.endswith("'''")):
                # Multi-line docstring
                j = i + 1
                while j < len(lines):
                    docstring += "\n" + lines[j]
                    if lines[j].strip().endswith('"""') or lines[j].strip().endswith("'''"):
                        break
                    j += 1
            return docstring
        elif line and not line.startswith('#') and line != '':
            # Non-empty, non-comment line found - no docstring
            break
        i += 1
    return None

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
