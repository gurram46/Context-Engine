"""Bundle command to generate context_for_ai.md"""

import click
from pathlib import Path

from ..core import (
    Config, 
    count_tokens, 
    strip_comments, 
    summarize_config,
    deduplicate_content,
    redact_secrets
)
from ..models import OpenRouterClient

@click.command()
@click.option('--use-ai/--no-ai', default=True, help="Use AI for summarization")
def bundle(use_ai):
    """Generate context_for_ai.md bundle"""
    config = Config()
    
    # Collect all content
    baseline_content = _collect_baseline(config)
    adrs_content = _collect_adrs(config)
    session_content = _read_session(config)
    cross_repo_content = _read_cross_repo(config)
    
    # Apply compression rules
    compression = config.get("compression_rules", {})
    
    if compression.get("strip_comments", True):
        baseline_content = _apply_comment_stripping(baseline_content)
    
    if compression.get("summarize_configs", True):
        baseline_content = _apply_config_summarization(baseline_content)
    
    if compression.get("deduplicate", True):
        baseline_content = deduplicate_content(baseline_content)
    
    # Redact secrets
    baseline_content = redact_secrets(baseline_content)
    session_content = redact_secrets(session_content)
    
    # Generate bundle
    if use_ai and config.openrouter_api_key:
        client = OpenRouterClient(config.openrouter_api_key)
        content = client.generate_context_bundle(
            baseline_content, 
            adrs_content,
            session_content, 
            cross_repo_content
        )
    else:
        # Manual bundle generation
        content = _manual_bundle(
            baseline_content,
            adrs_content,
            session_content,
            cross_repo_content
        )
    
    # Save bundle
    config.context_file.write_text(content, encoding='utf-8')
    
    # Calculate token count
    tokens = count_tokens(content)
    
    click.echo(f"✅ Generated context bundle: {config.context_file}")
    click.echo(f"📊 Token count: {tokens:,}")
    
    if tokens > config.get("max_tokens", 100000):
        click.echo(f"⚠️  Warning: Token count exceeds limit ({config.get('max_tokens', 100000):,})")
        click.echo("Consider removing some baseline files or older session notes.")

def _collect_baseline(config: Config) -> str:
    """Collect all baseline files"""
    if not config.baseline_dir.exists():
        return ""
    
    contents = []
    for file in config.baseline_dir.glob("*"):
        if file.is_file():
            contents.append(f"### {file.name}\n\n{file.read_text(encoding='utf-8')}")
    
    return "\n\n".join(contents)

def _collect_adrs(config: Config) -> str:
    """Collect all ADR files"""
    if not config.adrs_dir.exists():
        return ""
    
    contents = []
    for file in sorted(config.adrs_dir.glob("*.md")):
        contents.append(file.read_text(encoding='utf-8'))
    
    return "\n\n---\n\n".join(contents)

def _read_session(config: Config) -> str:
    """Read session notes"""
    if not config.session_file.exists():
        return ""
    return config.session_file.read_text(encoding='utf-8')

def _read_cross_repo(config: Config) -> str:
    """Read cross-repo notes"""
    if not config.cross_repo_file.exists():
        return ""
    return config.cross_repo_file.read_text(encoding='utf-8')

def _apply_comment_stripping(content: str) -> str:
    """Apply comment stripping to code files"""
    # Simple heuristic: strip comments from known code extensions
    lines = content.split('\n')
    result = []
    
    current_file = None
    for line in lines:
        if line.startswith("### ") and line.endswith((".py", ".js", ".java", ".cpp", ".c")):
            current_file = line
            result.append(line)
        elif current_file and current_file.endswith(".py"):
            # Python file - use strip_comments
            result.append(strip_comments(line, "python"))
        elif current_file and current_file.endswith((".js", ".ts")):
            result.append(strip_comments(line, "javascript"))
        else:
            result.append(line)
    
    return '\n'.join(result)

def _apply_config_summarization(content: str) -> str:
    """Apply config summarization"""
    lines = content.split('\n')
    result = []
    
    in_config = False
    config_lines = []
    
    for line in lines:
        if line.startswith("### ") and any(ext in line for ext in [".json", ".yml", ".yaml", ".toml", ".ini", ".env"]):
            if config_lines:
                result.append(summarize_config('\n'.join(config_lines)))
                config_lines = []
            in_config = True
            result.append(line)
        elif line.startswith("### "):
            if config_lines:
                result.append(summarize_config('\n'.join(config_lines)))
                config_lines = []
            in_config = False
            result.append(line)
        elif in_config:
            config_lines.append(line)
        else:
            result.append(line)
    
    if config_lines:
        result.append(summarize_config('\n'.join(config_lines)))
    
    return '\n'.join(result)

def _manual_bundle(baseline: str, adrs: str, session: str, cross_repo: str) -> str:
    """Generate manual bundle without AI"""
    sections = []
    
    sections.append("# Project Context for AI Tools")
    sections.append("\n*Generated by Context Engine V1*\n")
    
    if baseline:
        sections.append("## Baseline\n")
        sections.append(baseline)
    
    if adrs:
        sections.append("\n## Architectural Decision Records (ADRs)\n")
        sections.append(adrs)
    
    if session:
        sections.append("\n## Session Notes\n")
        sections.append(session)
    
    if cross_repo:
        sections.append("\n## Cross-Repository Notes\n")
        sections.append(cross_repo)
    
    return "\n".join(sections)
