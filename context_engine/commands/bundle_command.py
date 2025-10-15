"""Bundle command to generate context_for_ai.md"""

from pathlib import Path

import click

from ..ui import success, info, warn, error
from ..core import (
    Config,
    count_tokens,
    summarize_config,
    deduplicate_content,
    redact_secrets,
)
from ..core.utils import compress_whitespace
from ..core.task_manager import get_task
from ..models import OpenRouterClient


@click.command()
@click.option("--use-ai/--no-ai", default=True, help="Use AI for summarization")
def bundle(use_ai):
    """Generate context_for_ai.md bundle with fixed structure"""
    config = Config()

    # Check if session task exists
    current_task = get_task()
    
    if current_task:
        # Task-based session: include baseline + compressed_src sections
        info(f"Task-based session detected: {current_task}")
        
        # Collect structured content
        arch, apis, conf, schema = _collect_structured_baseline(config)
        session_content = _read_session(config)
        cross_repo_content = _read_cross_repo(config)
        expanded = _read_expanded_section(config)
        
        # Add compressed source files section if present
        compressed_src_content = _read_compressed_src_section(config)
        if expanded and compressed_src_content:
            expanded = expanded + "\n\n" + compressed_src_content
        elif compressed_src_content:
            expanded = compressed_src_content
            
        # Add task section
        task_content = f"### Current Task\n{current_task}"
        
    else:
        # No task: compression disabled - baseline-only mode
        warn("⚠ Compression disabled — baseline-only mode.")
        
        # Collect structured content
        arch, apis, conf, schema = _collect_structured_baseline(config)
        session_content = _read_session(config)
        cross_repo_content = _read_cross_repo(config)
        expanded = _read_expanded_section(config)
        task_content = "### Current Task\nNone"

    # Redact secrets
    arch = redact_secrets(arch)
    apis = redact_secrets(apis)
    conf = redact_secrets(conf)
    schema = redact_secrets(schema)
    session_content = redact_secrets(session_content)
    cross_repo_content = redact_secrets(cross_repo_content)
    expanded = redact_secrets(expanded)
    task_content = redact_secrets(task_content)

    # Apply whitespace compression and deduplication
    arch = deduplicate_content(compress_whitespace(arch))
    apis = deduplicate_content(compress_whitespace(apis))
    conf = deduplicate_content(compress_whitespace(conf))
    schema = deduplicate_content(compress_whitespace(schema))
    session_content = deduplicate_content(compress_whitespace(session_content))
    cross_repo_content = deduplicate_content(compress_whitespace(cross_repo_content))
    expanded = deduplicate_content(compress_whitespace(expanded))
    task_content = deduplicate_content(compress_whitespace(task_content))

    # Generate bundle
    if use_ai and config.openrouter_api_key:
        client = OpenRouterClient(config.openrouter_api_key)
        content = client.generate_fixed_context_bundle(
            architecture=arch or "None",
            apis=apis or "None",
            configuration=conf or "None",
            schema=schema or "None",
            session=session_content or "None",
            cross_repo=cross_repo_content or "None",
            expanded=expanded or "None",
            task=task_content or "None",
        )
    else:
        content = _manual_fixed_bundle(
            arch or "None",
            apis or "None",
            conf or "None",
            schema or "None",
            session=session_content or "None",
            cross_repo=cross_repo_content or "None",
            expanded=expanded or "None",
            task=task_content or "None",
        )

    try:
        config.context_file.write_text(content, encoding="utf-8")
        tokens = count_tokens(content)
        success(f"Generated context bundle: {config.context_file}")
        info(f"Token count: {tokens:,}")
        if tokens > config.get("max_tokens", 100000):
            warn(
                f"Token count exceeds limit ({config.get('max_tokens', 100000):,})"
            )
            info("Consider removing some baseline files or older session notes.")
    except IOError as e:
        error(f"Failed to write context bundle: {e}")
        raise click.ClickException(f"Could not write context file: {e}")
    except Exception as e:
        error(f"Unexpected error while generating bundle: {e}")
        raise click.ClickException(f"Bundle generation failed: {e}")


def _collect_structured_baseline(config: Config):
    """Collect baseline into fixed sections: architecture, apis, configuration, schema"""
    if not config.baseline_dir.exists():
        return "", "", "", ""

    # Map specific filenames to sections
    def read_if_exists(name_patterns):
        for pattern in name_patterns:
            for file in config.baseline_dir.glob(pattern):
                if file.is_file():
                    try:
                        return file.read_text(encoding="utf-8")
                    except UnicodeDecodeError:
                        warn(f"Could not read {file.name}: Invalid UTF-8 encoding")
                        return f"# [Error: Could not read {file.name} - invalid encoding]\n"
                    except IOError as e:
                        warn(f"Could not read {file.name}: {e}")
                        return f"# [Error: Could not read {file.name} - {e}]\n"
        return ""

    architecture = read_if_exists(["architecture.*", "arch.*", "system.*"])
    apis = read_if_exists(["apis.*", "api.*"])
    configuration = read_if_exists(["config.*", "configuration.*", "settings.*"])
    schema = read_if_exists(["schema.*", "db.*", "database.*"])
    return architecture, apis, configuration, schema


def _collect_adrs(config: Config) -> str:
    """Collect all ADR files"""
    if not config.adrs_dir.exists():
        return ""

    contents = []
    for file in sorted(config.adrs_dir.glob("*.md")):
        try:
            contents.append(file.read_text(encoding="utf-8"))
        except UnicodeDecodeError:
            warn(f"Could not read {file.name}: Invalid UTF-8 encoding")
            contents.append(f"# [Error: Could not read {file.name} - invalid encoding]\n")
        except IOError as e:
            warn(f"Could not read {file.name}: {e}")
            contents.append(f"# [Error: Could not read {file.name} - {e}]\n")

    return "\n\n---\n\n".join(contents)


def _read_session(config: Config) -> str:
    """Read session notes"""
    if not config.session_file.exists():
        return ""
    try:
        return config.session_file.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        warn(f"⚠️ Could not read session file: Invalid UTF-8 encoding")
        return "# [Error: Could not read session file - invalid encoding]\n"
    except IOError as e:
        warn(f"⚠️ Could not read session file: {e}")
        return f"# [Error: Could not read session file - {e}]\n"


def _read_cross_repo(config: Config) -> str:
    """Read cross-repo notes"""
    if not config.cross_repo_file.exists():
        return ""
    try:
        return config.cross_repo_file.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        warn(f"⚠️ Could not read cross-repo file: Invalid UTF-8 encoding")
        return "# [Error: Could not read cross-repo file - invalid encoding]\n"
    except IOError as e:
        warn(f"⚠️ Could not read cross-repo file: {e}")
        return f"# [Error: Could not read cross-repo file - {e}]\n"


def _read_expanded_section(config: Config) -> str:
    """Read the Expanded Files section from existing context (if present)"""
    if not config.context_file.exists():
        return ""
    try:
        content = config.context_file.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        warn(f"⚠️ Could not read context file: Invalid UTF-8 encoding")
        return "# [Error: Could not read expanded section - invalid encoding]\n"
    except IOError as e:
        warn(f"⚠️ Could not read context file: {e}")
        return f"# [Error: Could not read expanded section - {e}]\n"

    # Extract section starting with ## Expanded Files
    parts = content.split("\n## Expanded Files\n", 1)
    if len(parts) == 2:
        return parts[1].strip()
    return ""


def _read_compressed_src_section(config: Config) -> str:
    """Read compressed source files if they exist"""
    compressed_src_dir = config.context_dir / "compressed_src"
    if not compressed_src_dir.exists():
        return ""

    # Look for markdown files (generated by our compression)
    compressed_files = list(compressed_src_dir.glob("*.md"))
    if not compressed_files:
        return ""

    content = "### Compressed Source Files\n\n"
    for file in sorted(compressed_files):
        try:
            file_content = file.read_text(encoding="utf-8")
            # Validate that the content has at least one code block
            if not file_content.strip() or "```" not in file_content:
                warn(f"Compressed file {file.name} appears to be empty or malformed")
                file_content = f"# [Warning: {file.name} appears to be empty or malformed]\n"
            # Add the entire markdown content (it's already properly formatted)
            content += file_content + "\n\n"
        except UnicodeDecodeError:
            warn(f"⚠️ Could not read compressed file {file.name}: Invalid UTF-8 encoding")
            content += f"# [Error: Could not read {file.name} - invalid encoding]\n\n"
        except IOError as e:
            warn(f"⚠️ Could not read compressed file {file.name}: {e}")
            content += f"# [Error: Could not read {file.name} - {e}]\n\n"

    return content


def _apply_config_summarization(content: str) -> str:
    """Apply config summarization"""
    lines = content.split("\n")
    result = []

    in_config = False
    config_lines = []

    for line in lines:
        if line.startswith("### ") and any(
            ext in line for ext in [".json", ".yml", ".yaml", ".toml", ".ini", ".env"]
        ):
            if config_lines:
                result.append(summarize_config("\n".join(config_lines)))
                config_lines = []
            in_config = True
            result.append(line)
        elif line.startswith("### "):
            if config_lines:
                result.append(summarize_config("\n".join(config_lines)))
                config_lines = []
            in_config = False
            result.append(line)
        elif in_config:
            config_lines.append(line)
        else:
            result.append(line)

    if config_lines:
        result.append(summarize_config("\n".join(config_lines)))

    return "\n".join(result)


def _manual_fixed_bundle(
    architecture: str,
    apis: str,
    configuration: str,
    schema: str,
    session: str,
    cross_repo: str,
    expanded: str,
    task: str,
) -> str:
    """Generate manual bundle with fixed section order and placeholders"""
    lines = []
    lines.append("# Project Context for AI Tools\n")
    lines.append("*Generated by Context Engine V1*\n")
    lines.append("## Architecture\n")
    lines.append(architecture if architecture.strip() else "None")
    lines.append("\n## APIs\n")
    lines.append(apis if apis.strip() else "None")
    lines.append("\n## Configuration\n")
    lines.append(configuration if configuration.strip() else "None")
    lines.append("\n## Database Schema\n")
    lines.append(schema if schema.strip() else "None")
    lines.append("\n## Task\n")
    lines.append(task if task.strip() else "None")
    lines.append("\n## Session Notes\n")
    lines.append(session if session.strip() else "None")
    lines.append("\n## Cross-Repo Notes\n")
    lines.append(cross_repo if cross_repo.strip() else "None")
    lines.append("\n## Expanded Files\n")
    lines.append(expanded if expanded.strip() else "None")
    return "\n".join(lines)