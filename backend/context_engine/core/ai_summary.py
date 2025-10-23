"""AI-assisted session summarisation utilities."""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, Optional

import requests

from context_engine.core.config import get_api_key, get_model
from context_engine.ui import error, info, success, warn


SYSTEM_PROMPT = """You are an engineering assistant producing concise summaries of coding sessions.

Return short markdown with:
- Key accomplishments
- Important files touched
- Notable commands or tests
- Recommended next steps

Keep it under 200 words and make it scannable."""


class AISummarizer:
    """Generate AI-powered or static summaries of session activity."""

    def __init__(self) -> None:
        self.model = get_model()
        self.api_key = get_api_key()

    # ------------------------------------------------------------------ utils
    def _fallback_summary(self, content: str) -> str:
        lines = [line.strip() for line in content.splitlines() if line.strip()]
        activity = [line for line in lines if line.startswith("[")]
        edits = sum(1 for line in activity if "Edited:" in line)
        created = sum(1 for line in activity if "Created:" in line)
        deleted = sum(1 for line in activity if "Deleted:" in line)
        commands = sum(1 for line in activity if "Ran:" in line)

        preview = activity[-5:]

        summary = [
            "# Session Summary",
            "",
            "## Activity Overview",
            f"- Files edited: {edits}",
            f"- Files created: {created}",
            f"- Files deleted: {deleted}",
            f"- Commands recorded: {commands}",
            "",
            "## Recent Events",
        ]
        summary.extend(f"- {entry}" for entry in preview)
        return "\n".join(summary)

    def _call_ai(self, content: str) -> Optional[str]:
        if not self.api_key:
            return None

        payload: Dict[str, Any] = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": f"Session log:\n{content}"},
            ],
            "temperature": 0.5,
        }
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }
        try:
            response = requests.post(
                "https://openrouter.ai/api/v1/chat/completions",
                json=payload,
                headers=headers,
                timeout=60,
            )
            if response.status_code != 200:
                warn(f"AI request failed with status {response.status_code}, using fallback summary.")
                return None

            data = response.json()
            choices = data.get("choices") or []
            if not choices:
                warn("AI response missing choices; using fallback summary.")
                return None
            return choices[0].get("message", {}).get("content")
        except requests.RequestException as exc:
            warn(f"AI request error: {exc}. Using fallback summary.")
        except json.JSONDecodeError:
            warn("AI response was not valid JSON; using fallback summary.")
        return None

    # ------------------------------------------------------------------ public
    def summarize_session(self, session_file: Path, save_to_file: bool = True) -> str:
        if not session_file.exists():
            return "No session file found."

        content = session_file.read_text(encoding="utf-8")
        if not content.strip():
            return "Session file is empty."

        summary = self._call_ai(content) or self._fallback_summary(content)

        if save_to_file:
            summary_file = session_file.parent / "session_summary.md"
            summary_file.write_text(summary, encoding="utf-8")
            success(f"Session summary saved -> {summary_file}")

        return summary

    def quick_summary(self, session_file: Path) -> str:
        if not session_file.exists():
            return "No session activity recorded."

        content = session_file.read_text(encoding="utf-8")
        lines = [line for line in content.splitlines() if line.strip()]
        if not lines:
            return "No session activity recorded."

        activity = [line for line in lines if line.startswith("[")]
        edits = sum(1 for line in activity if "Edited:" in line)
        commands = sum(1 for line in activity if "Ran:" in line)

        if edits:
            highlight = f"edited {edits} files"
        elif commands:
            highlight = f"ran {commands} commands"
        else:
            highlight = "made project updates"

        return f"Summary:\nMost recent work {highlight}."


_summarizer: Optional[AISummarizer] = None


def get_summarizer() -> AISummarizer:
    global _summarizer
    if _summarizer is None:
        _summarizer = AISummarizer()
    return _summarizer


def generate_session_summary(session_file: Optional[Path] = None) -> str:
    if session_file is None:
        from .config import get_config_file

        session_file = get_config_file().parent / "session.md"

    summarizer = get_summarizer()
    info("Generating session summary...")
    summary = summarizer.summarize_session(session_file, save_to_file=True)
    return summary


def show_quick_summary(session_file: Optional[Path] = None) -> str:
    if session_file is None:
        from .config import get_config_file

        session_file = get_config_file().parent / "session.md"

    summarizer = get_summarizer()
    summary = summarizer.quick_summary(session_file)
    print(summary)
    return summary
