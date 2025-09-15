"""OpenRouter integration for Qwen3 Coder model"""

import json
import requests
from typing import Optional, Dict, Any

class OpenRouterClient:
    """Client for OpenRouter API"""
    
    SYSTEM_PROMPT = (
        "You are the Context Engine formatter.\n\n"
        "Always output `.context/context_for_ai.md` exactly in this structure and order:\n"
        "## Architecture\n## APIs\n## Configuration\n## Database Schema\n## Session Notes\n## Cross-Repo Notes\n## Expanded Files\n\n"
        "Rules:\n"
        "- Apply strict compression: strip all inline code comments, keep API docstrings only.\n"
        "- Summarize configs without secrets.\n"
        "- Remove blank lines and extra whitespace.\n"
        "- Deduplicate repetitive patterns.\n"
        "- Include all headings even if empty (use 'None' when empty).\n"
        "- Do not re-order or rename sections.\n"
        "- Never include raw, uncompressed code.\n"
        "- Always output valid Markdown.\n"
    )
    
    def __init__(self, api_key: str):
        from ..core.utils import is_valid_api_key
        self.api_key = api_key if is_valid_api_key(api_key) else ""
        self.base_url = "https://openrouter.ai/api/v1"
        self.model = "qwen/qwen3-coder:free"
    
    def summarize(self, content: str, task: str = "summarize") -> Optional[str]:
        """Summarize content using Qwen3 Coder"""
        if not self.api_key:
            return None
        
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
            "HTTP-Referer": "https://github.com/context-engine",
            "X-Title": "Context Engine"
        }
        
        prompt = self._build_prompt(content, task)
        
        data = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": self.SYSTEM_PROMPT},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.2,
            "max_tokens": 4000
        }
        
        try:
            response = requests.post(
                f"{self.base_url}/chat/completions",
                headers=headers,
                json=data,
                timeout=(10, 30),  # connect, read timeouts
            )
            response.raise_for_status()
            
            result = response.json()
            return result.get("choices", [{}])[0].get("message", {}).get("content", "")
        
        except (requests.RequestException, json.JSONDecodeError, KeyError):
            # Do not leak implementation details
            return None
    
    def _build_prompt(self, content: str, task: str) -> str:
        """Build prompt based on task type"""
        if task == "summarize":
            return (
                "Compress this source file for context bundling. Keep only API docstrings;"
                " remove comments and blank lines. Do not include raw code.\n\n"
                f"Content:\n{content}"
            )
        
        elif task == "compress":
            return (
                "Compress this configuration file: remove secrets, summarize values, keep structure.\n\n"
                f"Content:\n{content}"
            )
        
        elif task == "bundle":
            return (
                "Create a `.context/context_for_ai.md` using the fixed section order and rules."
                " Include all sections with 'None' when empty.\n\n"
                f"Content:\n{content}"
            )
        
        else:
            return content
    
    def generate_fixed_context_bundle(self, *, architecture: str, apis: str, configuration: str,
                                      schema: str, session: str, cross_repo: str, expanded: str) -> str:
        """Generate the final context_for_ai.md content in the fixed structure"""
        content = (
            "## Architecture\n" + architecture + "\n\n"
            "## APIs\n" + apis + "\n\n"
            "## Configuration\n" + configuration + "\n\n"
            "## Database Schema\n" + schema + "\n\n"
            "## Session Notes\n" + session + "\n\n"
            "## Cross-Repo Notes\n" + cross_repo + "\n\n"
            "## Expanded Files\n" + expanded + "\n"
        )
        ai_version = self.summarize(content, task="bundle")
        if not ai_version:
            # Fallback to manual fixed content
            return "# Project Context for AI Tools\n\n" + content
        return ai_version
