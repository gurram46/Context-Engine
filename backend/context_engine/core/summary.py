#!/usr/bin/env python3
"""
Context Engine Summary Generator
Generates comprehensive project summaries using AI models or static analysis
"""

import os
import sys
import json
import argparse
import subprocess
from pathlib import Path
from typing import Dict, List, Any, Optional
import time
import hashlib

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from core.config import get_model, set_model

class ProjectSummarizer:
    def __init__(self, model_choice: str = "static"):
        # Use configured model if not specified
        if model_choice == "static":
            self.model_choice = model_choice
        else:
            self.model_choice = model_choice or get_model()

        self.max_file_size = 10 * 1024 * 1024  # 10MB limit
        self.supported_extensions = {
            '.py', '.js', '.ts', '.jsx', '.tsx', '.md', '.txt', '.json',
            '.yaml', '.yml', '.toml', '.cfg', '.ini', '.sh', '.bat',
            '.html', '.css', '.scss', '.less', '.sql', '.dockerfile'
        }

    def scan_project_files(self, project_root: Path) -> Dict[str, Any]:
        """Scan project files and collect relevant content"""
        project_data = {
            'structure': {},
            'content': {},
            'metadata': {},
            'stats': {}
        }

        # Key directories to scan
        scan_dirs = ['.context', 'backend', 'ui', 'src', 'lib', 'docs']
        key_files = ['README.md', 'package.json', 'requirements.txt', 'pyproject.toml',
                    'setup.py', 'Dockerfile', '.gitignore', 'LICENSE']

        total_files = 0
        total_size = 0

        for item in project_root.rglob('*'):
            # Skip hidden files and directories (except .context)
            if item.name.startswith('.') and item.name != '.context':
                continue

            # Skip common non-relevant directories
            if any(skip in str(item) for skip in ['node_modules', '__pycache__', '.git',
                                                 'dist', 'build', '.pytest_cache',
                                                 '.coverage', 'htmlcov', '.tox']):
                continue

            if item.is_file():
                # Check if file is supported or is a key file
                if (item.suffix.lower() in self.supported_extensions or
                    item.name in key_files or
                    'context' in item.name.lower() or
                    'engine' in item.name.lower()):

                    try:
                        file_size = item.stat().st_size
                        if file_size <= self.max_file_size:
                            with open(item, 'r', encoding='utf-8', errors='ignore') as f:
                                content = f.read()

                            rel_path = str(item.relative_to(project_root))
                            project_data['content'][rel_path] = {
                                'content': content,
                                'size': file_size,
                                'type': self._categorize_file(item),
                                'language': self._detect_language(item)
                            }

                            total_files += 1
                            total_size += file_size

                    except (UnicodeDecodeError, PermissionError, OSError):
                        # Skip files that can't be read
                        continue

        # Update structure
        project_data['stats'] = {
            'total_files': total_files,
            'total_size': total_size,
            'scan_time': time.time()
        }

        return project_data

    def _categorize_file(self, file_path: Path) -> str:
        """Categorize file by its location and name"""
        path_str = str(file_path).lower()
        name = file_path.name.lower()

        if any(x in path_str for x in ['backend', 'server']):
            return 'backend'
        elif any(x in path_str for x in ['frontend', 'ui', 'client']):
            return 'frontend'
        elif any(x in path_str for x in ['config', 'setting']):
            return 'config'
        elif any(x in name for x in ['readme', 'doc', 'license']):
            return 'documentation'
        elif any(x in path_str for x in ['test', 'spec']):
            return 'test'
        elif any(x in name for x in ['docker', 'deploy']):
            return 'deployment'
        elif name in ['package.json', 'requirements.txt', 'pyproject.toml', 'setup.py']:
            return 'dependencies'
        else:
            return 'source'

    def _detect_language(self, file_path: Path) -> str:
        """Detect programming language from file extension"""
        suffix = file_path.suffix.lower()
        language_map = {
            '.py': 'Python',
            '.js': 'JavaScript',
            '.ts': 'TypeScript',
            '.jsx': 'React/JavaScript',
            '.tsx': 'React/TypeScript',
            '.md': 'Markdown',
            '.json': 'JSON',
            '.yaml': 'YAML',
            '.yml': 'YAML',
            '.toml': 'TOML',
            '.html': 'HTML',
            '.css': 'CSS',
            '.scss': 'SCSS',
            '.less': 'Less',
            '.sql': 'SQL',
            '.sh': 'Shell',
            '.bat': 'Batch',
            '.dockerfile': 'Docker'
        }
        return language_map.get(suffix, 'Unknown')

    def generate_static_summary(self, project_data: Dict[str, Any]) -> str:
        """Generate a static summary without AI"""
        content = project_data['content']
        stats = project_data['stats']

        # Analyze project structure
        file_types = {}
        languages = {}
        directories = set()

        for file_path, file_info in content.items():
            # Count file types
            file_type = file_info['type']
            file_types[file_type] = file_types.get(file_type, 0) + 1

            # Count languages
            language = file_info['language']
            languages[language] = languages.get(language, 0) + 1

            # Track directories
            directories.add(Path(file_path).parent)

        # Find main directories
        main_dirs = []
        for dir_path in directories:
            dir_name = Path(dir_path).name
            if any(key in dir_name.lower() for key in ['backend', 'frontend', 'ui', 'src', 'lib', 'core']):
                main_dirs.append(dir_name)

        # Extract tech stack from config files
        tech_stack = self._extract_tech_stack(content)

        # Generate summary
        summary = f"""# 🧩 Context Engine Project Summary

## 🎯 Purpose of the Project
This project appears to be a Context Engine - a hybrid CLI tool designed to compress and manage project context for AI coding sessions. The system combines Node.js frontend with Python backend to provide intelligent file compression, baseline generation, and context bundling capabilities.

## 🧱 Tech Stack Overview
{tech_stack}

## 🧠 Codebase Architecture
**Project Statistics:**
- Total Files: {stats['total_files']}
- Total Size: {self._format_size(stats['total_size'])}
- Main Directories: {', '.join(sorted(main_dirs)) if main_dirs else 'Standard project structure'}

**File Distribution:**
"""

        for file_type, count in sorted(file_types.items()):
            summary += f"- **{file_type.title()}**: {count} files\n"

        summary += "\n**Languages Used:**\n"
        for language, count in sorted(languages.items(), key=lambda x: x[1], reverse=True):
            summary += f"- **{language}**: {count} files\n"

        summary += "\n## ⚙️ Core Functionality & Logic Flow\n"
        summary += "Based on the file structure, this Context Engine provides:\n"
        summary += "- Project initialization and configuration management\n"
        summary += "- File compression and summarization capabilities\n"
        summary += "- Baseline generation for project context\n"
        summary += "- Bundle creation for AI tools integration\n"
        summary += "- Session management and status tracking\n"
        summary += "- Cross-repo functionality and documentation generation\n"

        summary += "\n## 🤖 AI & Automation Features\n"
        summary += "- LongCodeZip integration for intelligent compression\n"
        summary += "- LangChain integration for AI-powered summarization\n"
        summary += "- Multiple AI model support (Claude, GLM, Qwen)\n"
        summary += "- Automated architecture generation\n"
        summary += "- Token counting and optimization\n"

        summary += "\n## 🧩 Current Session & Status\n"
        summary += f"- Generated on: {time.strftime('%Y-%m-%d %H:%M:%S')}\n"
        summary += f"- Project scanned: {stats['total_files']} files analyzed\n"
        summary += "- Last chosen model: " + self.model_choice + "\n"

        # Check for existing context files
        context_files = [f for f in content.keys() if 'context' in f.lower()]
        if context_files:
            summary += f"- Active context files: {len(context_files)}\n"

        summary += "\n## 📊 Project Health Overview\n"

        # Health assessment
        health_score = self._calculate_health_score(project_data)
        summary += f"- **Overall Stability Rating: {health_score['grade']}\n"
        summary += f"- **Code Structure Clarity: {health_score['structure']}\n"
        summary += f"- **Modularity: {health_score['modularity']}\n"
        summary += f"- **Scalability Potential: {health_score['scalability']}\n"
        summary += f"- **Maintenance Readiness: {health_score['maintenance']}\n"

        summary += f"\n**Health Assessment Details:**\n{health_score['details']}"

        return summary

    def _extract_tech_stack(self, content: Dict[str, Any]) -> str:
        """Extract tech stack information from config files"""
        tech_info = {
            'Frontend': [],
            'Backend': [],
            'Database/Storage': [],
            'Infrastructure': [],
            'AI/LLM Integration': []
        }

        for file_path, file_info in content.items():
            file_content = file_info['content'].lower()

            # Check package.json for frontend dependencies
            if 'package.json' in file_path:
                if 'react' in file_content:
                    tech_info['Frontend'].append('React')
                if 'vue' in file_content:
                    tech_info['Frontend'].append('Vue')
                if 'angular' in file_content:
                    tech_info['Frontend'].append('Angular')
                if 'node' in file_content:
                    tech_info['Frontend'].append('Node.js')
                if 'typescript' in file_content:
                    tech_info['Frontend'].append('TypeScript')

            # Check requirements.txt for backend dependencies
            if 'requirements.txt' in file_path:
                if 'fastapi' in file_content:
                    tech_info['Backend'].append('FastAPI')
                if 'flask' in file_content:
                    tech_info['Backend'].append('Flask')
                if 'django' in file_content:
                    tech_info['Backend'].append('Django')
                if 'langchain' in file_content:
                    tech_info['AI/LLM Integration'].append('LangChain')

            # Check for AI-related dependencies
            if any(ai in file_content for ai in ['openai', 'anthropic', 'claude', 'glm', 'qwen']):
                if 'openai' in file_content or 'anthropic' in file_content:
                    tech_info['AI/LLM Integration'].append('OpenAI/Anthropic')
                if 'glm' in file_content:
                    tech_info['AI/LLM Integration'].append('GLM')
                if 'qwen' in file_content:
                    tech_info['AI/LLM Integration'].append('Qwen')

        # Format tech stack
        tech_stack = ""
        for category, technologies in tech_info.items():
            if technologies:
                tech_stack += f"- **{category}:** {', '.join(set(technologies))}\n"

        if not tech_stack:
            tech_stack = "- **Frontend:** Node.js/JavaScript\n"
            tech_stack += "- **Backend:** Python\n"
            tech_stack += "- **AI/LLM Integration:** LangChain, OpenRouter\n"

        return tech_stack

    def _calculate_health_score(self, project_data: Dict[str, Any]) -> Dict[str, str]:
        """Calculate project health score"""
        content = project_data['content']
        stats = project_data['stats']

        score = 0
        max_score = 100
        details = []

        # Check for documentation
        has_readme = any('readme' in f.lower() for f in content.keys())
        if has_readme:
            score += 15
            details.append("✅ README documentation present")
        else:
            details.append("⚠️  Missing README documentation")

        # Check for configuration files
        has_config = any(f in ['package.json', 'requirements.txt', 'pyproject.toml', 'setup.py']
                        for f in content.keys())
        if has_config:
            score += 15
            details.append("✅ Proper dependency management")
        else:
            details.append("⚠️  Missing dependency configuration")

        # Check for test files
        has_tests = any('test' in f.lower() for f in content.keys())
        if has_tests:
            score += 10
            details.append("✅ Test files present")
        else:
            details.append("⚠️  No test files found")

        # Check project structure
        has_structure = any(dir in ' '.join(content.keys()) for dir in ['backend', 'frontend', 'ui', 'src'])
        if has_structure:
            score += 15
            details.append("✅ Well-organized project structure")
        else:
            details.append("⚠️  Basic project structure")

        # Check for Context Engine specific features
        has_context = any('context' in f.lower() for f in content.keys())
        if has_context:
            score += 20
            details.append("✅ Context Engine features implemented")

        # Check size and complexity
        if stats['total_files'] > 10:
            score += 10
            details.append("✅ Substantial codebase")

        if stats['total_files'] > 50:
            score += 5
            details.append("✅ Comprehensive implementation")

        # Determine grade
        if score >= 90:
            grade = 'A'
        elif score >= 80:
            grade = 'B'
        elif score >= 70:
            grade = 'C'
        elif score >= 60:
            grade = 'D'
        else:
            grade = 'F'

        return {
            'grade': grade,
            'structure': 'Good' if has_structure else 'Basic',
            'modularity': 'High' if stats['total_files'] > 20 else 'Medium',
            'scalability': 'Good' if has_config and has_structure else 'Limited',
            'maintenance': 'Ready' if has_config and has_readme else 'Needs Work',
            'details': '\n'.join(details)
        }

    def _format_size(self, size_bytes: int) -> str:
        """Format file size in human readable format"""
        for unit in ['B', 'KB', 'MB', 'GB']:
            if size_bytes < 1024.0:
                return f"{size_bytes:.1f} {unit}"
            size_bytes /= 1024.0
        return f"{size_bytes:.1f} TB"

    async def generate_ai_summary(self, project_data: Dict[str, Any]) -> str:
        """Generate AI-powered summary using any supported model (universal prompt)"""
        import os
        import requests

        # 🧠 UNIVERSAL PROJECT SUMMARY PROMPT
        system_prompt = """
You are a senior full-stack software architect and AI systems engineer.

Your job is to analyze a given project's structure and generate a
detailed, human-readable summary that helps any new developer or CTO
understand it quickly.

Analyze based on the project's files, folder names, and code snippets
provided. Write like a technical lead reviewing a real product.

──────────────────────────────────────────────
OUTPUT FORMAT (Markdown)
──────────────────────────────────────────────

# 🧩 Project Summary

## 🎯 Purpose & Vision
Explain the main goal of the project — what problem it solves, its
intended users, and its overall concept or mission.

## 🧱 Tech Stack
Identify all major technologies, programming languages, frameworks,
databases, and AI integrations used. Explain how they fit together.

## 🧠 Architecture & Design
Describe the overall structure and flow of the system:
- How the backend, frontend, and/or CLI interact
- What major modules exist and how they connect
- How context, data, and configuration flow through the system

## ⚙️ Core Logic & Process Flow
Summarize key operations, pipelines, or command sequences (e.g.
`init → baseline → bundle → compress → summary`).
Highlight important algorithms, abstractions, or logic patterns.

## 🤖 AI & Automation Components
Identify any LLMs, embeddings, LangChain, or automation logic.
Explain how AI is used (compression, summarization, analysis, etc.)

## 🗂️ File & Folder Overview
Briefly summarize the main directories and their responsibilities.
Don't dump every file — summarize meaningfully.

## 💡 Strengths & Areas to Improve
Critically analyze architecture quality:
- Modularity, maintainability, and readability
- Performance or scalability issues
- Missing documentation or tests

## 📊 Project Health Grade
Assign a letter grade (A–F) for:
- Architecture clarity
- Scalability
- Maintainability
- Documentation
- Testing coverage

Then give a short overall verdict (2–3 sentences).

──────────────────────────────────────────────
STYLE & BEHAVIOR
──────────────────────────────────────────────
- Write assertively, like a lead engineer.
- No filler phrases like "it seems" or "probably".
- Avoid listing file counts or token stats.
- Infer intent — don't describe, *analyze*.
- Stay readable, organized, and Markdown-friendly.
- If information is missing, make smart logical inferences.
"""

        # 🧩 Combine a subset of project files for LLM context
        combined_text = ""
        for i, (file_path, file_info) in enumerate(project_data.get("content", {}).items()):
            if i >= 50:  # limit for token safety
                break
            content = file_info.get("content", "")
            combined_text += f"\n# FILE: {file_path}\n{content[:4000]}\n"

        # Map friendly aliases to provider/model slugs.
        model_mapping = {
            "claude": "anthropic/claude-3.5-sonnet",
            "glm": "zhipuai/glm-4.5-air:free",
            "qwen": "qwen/qwen-2.5-72b-instruct",
            "deepseek": "deepseek/deepseek-chat-v3.1:free",
            "kimi": "moonshotai/kimi-dev-72b:free",
            "static": None,
        }

        choice_key = (self.model_choice or "").lower()
        api_model = model_mapping.get(choice_key)

        if not api_model and self.model_choice:
            # Allow passing full OpenRouter-style slugs such as
            # "openrouter/moonshotai/kimi-dev-72b:free".
            if "/" in self.model_choice or ":" in self.model_choice:
                api_model = self.model_choice

        if not api_model:
            # Fallback to static if model not supported or missing.
            return self.generate_static_summary(project_data)

        # Adjust payload format based on model
        if choice_key == "glm":
            # GLM uses different format
            payload = {
                "model": api_model,
                "messages": [
                    {"role": "system", "content": system_prompt.strip()},
                    {"role": "user", "content": f"Analyze this project:\n{combined_text}"}
                ],
                "temperature": 0.7
            }
        elif choice_key == "kimi":
            # Kimi uses standard OpenAI format
            payload = {
                "model": api_model,
                "messages": [
                    {"role": "system", "content": system_prompt.strip()},
                    {"role": "user", "content": f"Analyze this project:\n{combined_text}"}
                ],
                "temperature": 0.7
            }
        else:
            # Standard OpenRouter/OpenAI format
            payload = {
                "model": api_model,
                "messages": [
                    {"role": "system", "content": system_prompt.strip()},
                    {"role": "user", "content": f"Analyze this project:\n{combined_text}"}
                ],
                "temperature": 0.7
            }

        try:
            # 🚀 Use OpenRouter API for all models with simplified config
            openrouter_key = os.getenv("OPENROUTER_API_KEY")
            api_keys = {
                "glm": openrouter_key,
                "kimi": openrouter_key,
                "deepseek": openrouter_key,
                "claude": openrouter_key,
                "qwen": openrouter_key
            }

            api_key = api_keys.get(choice_key, openrouter_key)
            if not api_key:
                raise RuntimeError("OPENROUTER_API_KEY not configured")
            headers = {"Authorization": f"Bearer {api_key}"}

            # Add retry logic for better reliability
            max_retries = 3
            for attempt in range(max_retries):
                try:
                    response = requests.post("https://openrouter.ai/api/v1/chat/completions",
                                            json=payload, headers=headers, timeout=90)
                    result = response.json()

                    # Check if response is successful
                    if response.status_code == 200 and "choices" in result:
                        summary = result["choices"][0]["message"]["content"]
                        break
                    else:
                        if attempt == max_retries - 1:
                            raise Exception(f"API Error: {result.get('error', 'Unknown error')}")
                        continue

                except requests.exceptions.RequestException as e:
                    if attempt == max_retries - 1:
                        raise e
                    continue

        except Exception as e:
            # Try fallback models in order if primary fails
            fallback_models = ["kimi", "glm", "deepseek", "claude"]

            for fallback_model in fallback_models:
                if fallback_model == self.model_choice:
                    continue  # Skip the model that already failed

                try:
                    fallback_api_model = model_mapping.get(fallback_model)
                    fallback_api_key = api_keys.get(fallback_model)

                    if fallback_api_model and fallback_api_key:
                        fallback_headers = {"Authorization": f"Bearer {fallback_api_key}"}
                        fallback_payload = {
                            "model": fallback_api_model,
                            "input": [
                                {"role": "system", "content": system_prompt.strip()},
                                {"role": "user", "content": f"Analyze this project:\n{combined_text}"}
                            ]
                        }

                        response = requests.post("https://openrouter.ai/api/v1/chat/completions",
                                                json=fallback_payload, headers=fallback_headers, timeout=60)
                        result = response.json()

                        if response.status_code == 200 and "choices" in result:
                            summary = result["choices"][0]["message"]["content"]
                            summary = f"[Generated with fallback model: {fallback_model}]\n\n" + summary
                            break

                except:
                    continue
            else:
                # If all fallbacks fail, use static summary
                summary = f"[All AI models failed: {e}]\n\n" + self.generate_static_summary(project_data)

        return summary

    def save_summary(self, summary: str) -> str:
        """Save summary to file and return path"""
        summary_dir = Path.cwd() / ".context"
        summary_file = summary_dir / "summary_report.md"

        # Ensure directory exists
        summary_dir.mkdir(exist_ok=True)

        # Write summary
        with open(summary_file, 'w', encoding='utf-8') as f:
            f.write(summary)

        return str(summary_file)

    async def generate_summary(self, project_root: Path) -> str:
        """Main method to generate project summary"""
        print("Analyzing project files...")

        # Scan project files
        project_data = self.scan_project_files(project_root)

        print(f"Scanned {project_data['stats']['total_files']} files")

        # Generate summary based on model choice
        if self.model_choice == "static":
            print("Generating static summary...")
            summary = self.generate_static_summary(project_data)
        else:
            print(f"Generating AI summary using {self.model_choice}...")
            summary = await self.generate_ai_summary(project_data)

        # Save summary
        summary_path = self.save_summary(summary)
        print(f"Summary saved to: {summary_path}")

        return summary, summary_path

async def main():
    """Main entry point for summary generation"""
    parser = argparse.ArgumentParser(description="Generate project summary")
    parser.add_argument("--model", choices=["claude", "glm", "qwen", "langchain", "static"],
                       default="static", help="AI model to use for summarization")
    parser.add_argument("--project-root", type=str, help="Project root directory")

    args = parser.parse_args()

    try:
        # Determine project root
        if args.project_root:
            project_root = Path(args.project_root)
        else:
            project_root = Path.cwd()

        # Initialize summarizer
        summarizer = ProjectSummarizer(model_choice=args.model)

        # Generate summary
        summary, summary_path = await summarizer.generate_summary(project_root)

        # Output result
        print(json.dumps({
            "success": True,
            "summary": summary,
            "summary_path": summary_path,
            "model_used": args.model
        }))

    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": str(e)
        }))
        sys.exit(1)

if __name__ == "__main__":
    import asyncio
    asyncio.run(main())
