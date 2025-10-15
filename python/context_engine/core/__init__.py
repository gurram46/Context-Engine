"""Core utilities for Context Engine"""

from .config import Config
from .auto_architecture import generate_auto_architecture
from .utils import (
    calculate_file_hash,
    load_hashes,
    save_hashes,
    check_staleness,
    update_hash,
    count_tokens,
    redact_secrets,
    strip_comments,
    summarize_config,
    deduplicate_content,
)

__all__ = [
    'Config',
    'generate_auto_architecture',
    'calculate_file_hash',
    'load_hashes',
    'save_hashes',
    'check_staleness',
    'update_hash',
    'count_tokens',
    'redact_secrets',
    'strip_comments',
    'summarize_config',
    'deduplicate_content',
]
