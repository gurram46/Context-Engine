"""Configuration management for Context Engine."""

import os
import yaml
from pathlib import Path
from typing import Dict, List, Optional, Any
from dataclasses import dataclass, asdict

@dataclass
class ProjectConfig:
    """Project-specific configuration."""
    name: str = "my-project"
    description: str = ""
    modules: List[str] = None
    
    def __post_init__(self):
        if self.modules is None:
            self.modules = []

@dataclass
class EmbeddingConfig:
    """Embedding provider configuration."""
    provider: str = "local"  # local, openai, openrouter
    model: str = "all-MiniLM-L6-v2"
    chunk_size: int = 1000
    chunk_overlap: int = 150
    api_key_env: Optional[str] = None

@dataclass
class IndexingConfig:
    """Indexing configuration."""
    ignore_patterns: List[str] = None
    redact_patterns: List[str] = None
    max_file_size_mb: int = 10
    
    def __post_init__(self):
        if self.ignore_patterns is None:
            self.ignore_patterns = [
                ".git/*",
                "node_modules/*",
                "__pycache__/*",
                "*.pyc",
                "*.pyo",
                "*.pyd",
                ".DS_Store",
                "*.log",
                "*.tmp",
                "*.temp",
                "context_engine/*",
                ".context_payload/*",
                "*.min.js",
                "*.min.css",
                "dist/*",
                "build/*",
                "target/*",
                "*.jar",
                "*.war",
                "*.zip",
                "*.tar.gz",
                "*.exe",
                "*.dll",
                "*.so",
                "*.dylib"
            ]
        
        if self.redact_patterns is None:
            self.redact_patterns = [
                r"(?i)(api[_-]?key|secret|password|token|auth)[\s]*[=:][\s]*['\"]?([^\s'\"\n]+)",
                r"(?i)(bearer|basic)\s+([a-zA-Z0-9+/=]+)",
                r"(?i)-----BEGIN [A-Z ]+-----[\s\S]*?-----END [A-Z ]+-----",
                r"(?i)(mongodb://|postgres://|mysql://)[^\s]+",
                r"(?i)([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})"
            ]

@dataclass
class SharedContextConfig:
    """Shared context repository configuration."""
    enabled: bool = False
    repo_url: Optional[str] = None
    branch: str = "main"
    auto_push: bool = False

@dataclass
class ContextConfig:
    """Main configuration class."""
    project: ProjectConfig
    embedding: EmbeddingConfig
    indexing: IndexingConfig
    shared_context: SharedContextConfig
    project_root: Optional[Path] = None
    
    def __post_init__(self):
        """Initialize computed properties."""
        if self.project_root is None:
            self.project_root = Path.cwd()
    
    @property
    def data_dir(self) -> Path:
        """Get the data directory path."""
        return self.project_root / '.context' / 'data'
    
    @property
    def handoff_dir(self) -> Path:
        """Get the handoff notes directory path."""
        return self.project_root / '.context' / 'handoffs'
    
    @property
    def logs_dir(self) -> Path:
        """Get the logs directory path."""
        return self.project_root / '.context' / 'logs'
    
    @classmethod
    def default(cls) -> 'ContextConfig':
        """Create default configuration."""
        return cls(
            project=ProjectConfig(),
            embedding=EmbeddingConfig(),
            indexing=IndexingConfig(),
            shared_context=SharedContextConfig(),
            project_root=Path.cwd()
        )
    
    @classmethod
    def load_from_file(cls, config_path: Path) -> 'ContextConfig':
        """Load configuration from YAML file."""
        if not config_path.exists():
            raise FileNotFoundError(f"Configuration file not found: {config_path}")
        
        with open(config_path, 'r', encoding='utf-8') as f:
            data = yaml.safe_load(f) or {}
        
        return cls(
            project=ProjectConfig(**data.get('project', {})),
            embedding=EmbeddingConfig(**data.get('embedding', {})),
            indexing=IndexingConfig(**data.get('indexing', {})),
            shared_context=SharedContextConfig(**data.get('shared_context', {})),
            project_root=config_path.parent.parent.parent
        )
    
    def save_to_file(self, config_path: Path) -> None:
        """Save configuration to YAML file."""
        config_path.parent.mkdir(parents=True, exist_ok=True)
        
        data = {
            'project': asdict(self.project),
            'embedding': asdict(self.embedding),
            'indexing': asdict(self.indexing),
            'shared_context': asdict(self.shared_context)
        }
        
        with open(config_path, 'w', encoding='utf-8') as f:
            yaml.dump(data, f, default_flow_style=False, indent=2)
    
    @classmethod
    def load_or_create(cls, project_root: Optional[Path] = None) -> 'ContextConfig':
        """Load existing config or create default one."""
        if project_root is None:
            project_root = Path.cwd()
        
        config_path = project_root / "context_engine" / "config" / "context.yml"
        
        if config_path.exists():
            return cls.load_from_file(config_path)
        else:
            # Create default config
            config = cls.default()
            # Try to infer project name from directory
            config.project.name = project_root.name
            return config
    
    def get_context_engine_dir(self, project_root: Optional[Path] = None) -> Path:
        """Get the context engine directory path."""
        if project_root is None:
            project_root = Path.cwd()
        return project_root / "context_engine"
    
    def get_team_context_dir(self, project_root: Optional[Path] = None) -> Path:
        """Get the team context directory path."""
        if project_root is None:
            project_root = Path.cwd()
        return project_root / "team_context"
    
    def get_context_payload_dir(self, project_root: Optional[Path] = None) -> Path:
        """Get the context payload directory path."""
        if project_root is None:
            project_root = Path.cwd()
        return project_root / ".context_payload"