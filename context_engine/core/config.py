"""Configuration management for Context Engine"""

import os
import json
from pathlib import Path
from typing import Dict, Any, Optional

class Config:
    """Manages Context Engine configuration"""
    
    DEFAULT_CONFIG = {
        "openrouter_api_key": "",
        "model": "qwen/qwen3-coder:free",
        "max_tokens": 100000,
        "context_dir": ".context",
        "auto_refresh": False,
        "compression_rules": {
            "strip_comments": True,
            "keep_docstrings": True,
            "summarize_configs": True,
            "deduplicate": True,
            "remove_blank_lines": True
        },
        "linked_repos": [],
        # Security-related defaults
        "allowed_extensions": [
            ".md", ".json", ".yml", ".yaml", ".toml", ".ini",
            ".py", ".js", ".ts", ".java", ".c", ".cpp"
        ],
        "max_file_size_kb": 1024,
        "note_max_length": 2000
    }
    
    def __init__(self, project_root: Optional[Path] = None):
        self.project_root = project_root or Path.cwd().resolve()
        self.context_dir = self.project_root / ".context"
        self.config_file = self.context_dir / "config.json"
        self._config = self.DEFAULT_CONFIG.copy()
        self.load()
    
    def load(self) -> None:
        """Load configuration from file if it exists"""
        if self.config_file.exists():
            try:
                with open(self.config_file, 'r') as f:
                    stored_config = json.load(f)
                    self._config.update(stored_config)
            except (json.JSONDecodeError, IOError):
                pass
    
    def save(self) -> None:
        """Save configuration to file"""
        self.context_dir.mkdir(exist_ok=True)
        with open(self.config_file, 'w') as f:
            json.dump(self._config, f, indent=2)
    
    def get(self, key: str, default: Any = None) -> Any:
        """Get configuration value"""
        return self._config.get(key, default)
    
    def set(self, key: str, value: Any) -> None:
        """Set configuration value"""
        self._config[key] = value
        self.save()
    
    @property
    def openrouter_api_key(self) -> str:
        """Get OpenRouter API key from config or environment"""
        return self.get("openrouter_api_key") or os.getenv("OPENROUTER_API_KEY", "")
    
    @property
    def baseline_dir(self) -> Path:
        """Get baseline directory path"""
        return self.context_dir / "baseline"
    
    @property
    def adrs_dir(self) -> Path:
        """Get ADRs directory path"""
        return self.context_dir / "adrs"
    
    @property
    def session_file(self) -> Path:
        """Get session file path"""
        return self.context_dir / "session.md"
    
    @property
    def cross_repo_file(self) -> Path:
        """Get cross-repo file path"""
        return self.context_dir / "cross_repo.md"
    
    @property
    def context_file(self) -> Path:
        """Get context_for_ai.md file path"""
        return self.context_dir / "context_for_ai.md"
    
    @property
    def hashes_file(self) -> Path:
        """Get hashes.json file path"""
        return self.context_dir / "hashes.json"
