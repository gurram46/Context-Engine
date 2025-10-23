"""Ultra-simplified AI model configuration for Context Engine v1.2.1"""
import os
import json
from pathlib import Path
from typing import Optional

# Default model - works out of the box
DEFAULT_MODEL = "qwen-1.5-mini"

# Minimal config structure - only what we actually need
DEFAULT_CONFIG = {"model": DEFAULT_MODEL, "api_key": None}


def get_config_file() -> Path:
    """Get the config file path"""
    return Path.cwd() / ".context" / "config.json"


def load_config() -> dict:
    """Load minimal config or create default"""
    config_file = get_config_file()

    if config_file.exists():
        try:
            with open(config_file, "r", encoding="utf-8") as f:
                return json.load(f)
        except (json.JSONDecodeError, UnicodeDecodeError):
            # Fallback to defaults if config is corrupted
            return DEFAULT_CONFIG.copy()
    else:
        # Create default config on first use
        config_file.parent.mkdir(parents=True, exist_ok=True)
        with open(config_file, "w", encoding="utf-8") as f:
            json.dump(DEFAULT_CONFIG, f, indent=2)
        return DEFAULT_CONFIG.copy()


def get_model() -> str:
    """Get the current AI model - environment override first, then config, then default"""
    return os.getenv("CONTEXT_ENGINE_MODEL") or load_config().get("model", DEFAULT_MODEL)


def set_model(model: str) -> None:
    """Set and save the model preference"""
    config = load_config()
    config["model"] = model

    config_file = get_config_file()
    config_file.parent.mkdir(parents=True, exist_ok=True)

    with open(config_file, "w", encoding="utf-8") as f:
        json.dump(config, f, indent=2)


def get_api_key() -> str:
    """Get API key from environment or config"""
    return os.getenv("OPENROUTER_API_KEY") or load_config().get("api_key")


# Legacy compatibility class providing filesystem helpers
class Config:
    """Lightweight configuration wrapper exposing common paths."""

    def __init__(self, project_root: Optional[Path] = None, create_dirs: bool = True):
        self.project_root = Path(project_root or Path.cwd())
        self.context_dir = self.project_root / ".context"
        self.baseline_dir = self.context_dir / "baseline"
        self.notes_dir = self.context_dir / "notes"
        self.session_file = self.context_dir / "session.md"
        self.cross_repo_file = self.context_dir / "cross_repo.json"
        self.config_file = self.context_dir / "config.json"
        self.context_file = self.context_dir / "context_for_ai.md"
        self.hashes_file = self.context_dir / "hashes.json"

        if create_dirs:
            self.context_dir.mkdir(parents=True, exist_ok=True)
            self.baseline_dir.mkdir(exist_ok=True)
            self.notes_dir.mkdir(exist_ok=True)

        self._config = self._load_local_config()

    def _load_local_config(self) -> dict:
        if self.config_file.exists():
            try:
                return json.loads(self.config_file.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                pass
        else:
            self.config_file.parent.mkdir(parents=True, exist_ok=True)
        self.config_file.write_text(json.dumps(DEFAULT_CONFIG, indent=2), encoding="utf-8")
        return DEFAULT_CONFIG.copy()

    def save(self) -> None:
        self.config_file.write_text(json.dumps(self._config, indent=2), encoding="utf-8")

    def get(self, key: str, default=None):
        return self._config.get(key, default)

    @property
    def model(self) -> str:
        return self._config.get("model", DEFAULT_MODEL)

    @property
    def api_key(self) -> Optional[str]:
        return self._config.get("api_key")
