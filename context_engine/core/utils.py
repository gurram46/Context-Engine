"""Utility functions for Context Engine"""

import hashlib
import json
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

def redact_secrets(text: str) -> str:
    """Redact potential secrets from text"""
    # Redact API keys
    text = re.sub(r'(["\']?)([A-Za-z0-9]{32,}|sk-[A-Za-z0-9]{48,})(["\']?)', r'\1[REDACTED_KEY]\3', text)
    
    # Redact passwords
    text = re.sub(r'(password|passwd|pwd|pass)\s*[=:]\s*["\']?([^"\'\s]+)["\']?', 
                  r'\1=[REDACTED]', text, flags=re.IGNORECASE)
    
    # Redact tokens
    text = re.sub(r'(token|jwt|bearer)\s*[=:]\s*["\']?([^"\'\s]+)["\']?', 
                  r'\1=[REDACTED]', text, flags=re.IGNORECASE)
    
    # Redact environment variables that likely contain secrets
    text = re.sub(r'(API_KEY|SECRET|TOKEN|PASSWORD|PASSWD)\s*=\s*["\']?([^"\'\s]+)["\']?',
                  r'\1=[REDACTED]', text)
    
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
        code = re.sub(r'//.*$', '', code, flags=re.MULTILINE)
        code = re.sub(r'/\*(?!\*)[^*]*\*+(?:[^/*][^*]*\*+)*/', '', code)
        return code
    
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
            result.append(line)
    
    return '\n'.join(result)
