"""Automatic architecture summarization utilities."""
from __future__ import annotations
import json
import os
from collections import Counter
from datetime import datetime
from pathlib import Path
from typing import Dict, Iterable, List, Set, Tuple
try:
    import tomllib  # type: ignore[attr-defined]
    TOMLDecodeError = tomllib.TOMLDecodeError  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover - fallback for older runtimes
    tomllib = None  # type: ignore[assignment]
    class TOMLDecodeError(Exception):
        """Fallback TOML decode error when tomllib is unavailable."""
        pass
from .config import Config
from .utils import redact_secrets
MAX_DIR_SIZE_BYTES = 50 * 1024 * 1024  # 50 MB safety cut-off
MAX_TRAVERSAL_DEPTH = 34
IGNORE_DIR_NAMES = {
    '.git',
    '.hg',
    '.svn',
    '.context',
    '.tox',
    '.venv',
    'venv',
    'env',
    'node_modules',
    'dist',
    'build',
    '__pycache__',
    '.pytest_cache',
    'coverage',
    'tmp',
    'logs',
    '.idea',
    '.vs',
}
LANGUAGE_EXTENSIONS: Dict[str, Tuple[str, ...]] = {
    'Python': ('.py', '.pyw'),
    'JavaScript': ('.js', '.jsx'),
    'TypeScript': ('.ts', '.tsx'),
    'Java': ('.java',),
    'Kotlin': ('.kt', '.kts'),
    'Go': ('.go',),
    'Rust': ('.rs',),
    'C#': ('.cs',),
    'C++': ('.cpp', '.cc', '.cxx', '.hpp', '.hh', '.hxx'),
    'C': ('.c', '.h'),
    'Swift': ('.swift',),
    'Ruby': ('.rb',),
    'PHP': ('.php',),
    'Scala': ('.scala',),
    'Shell': ('.sh', '.bash'),
    'SQL': ('.sql',),
    'HTML': ('.html', '.htm'),
    'CSS': ('.css', '.scss', '.sass'),
}
FRAMEWORK_PATTERNS: Dict[str, Tuple[str, ...]] = {
    'Django': ('django',),
    'Flask': ('flask',),
    'FastAPI': ('fastapi',),
    'Starlette': ('starlette',),
    'SQLAlchemy': ('sqlalchemy',),
    'Celery': ('celery',),
    'React': ('react', 'react-dom'),
    'Next.js': ('next',),
    'Express': ('express',),
    'NestJS': ('@nestjs/core',),
    'Vue': ('vue', '@vue/runtime-core'),
    'Angular': ('@angular/core',),
    'Svelte': ('svelte',),
    'Vite': ('vite',),
    'Jest': ('jest',),
    'Electron': ('electron',),
    'Tailwind CSS': ('tailwindcss',),
    'Spring Boot': ('spring-boot-starter', 'spring-boot'),
    'TensorFlow': ('tensorflow', 'tensorflow-gpu'),
    'PyTorch': ('torch', 'pytorch'),
    'Fastify': ('fastify',),
}
DIRECTORY_HINTS: Dict[str, str] = {
    'api': 'API endpoints or interface layer',
    'apis': 'API contracts or definitions',
    'app': 'Application bootstrap / runtime',
    'backend': 'Backend services',
    'server': 'Server application',
    'services': 'Service layer logic',
    'models': 'Data or ORM models',
    'schemas': 'Validation schemas or data contracts',
    'frontend': 'Frontend application',
    'client': 'Client-side application code',
    'ui': 'UI components',
    'components': 'Reusable interface components',
    'hooks': 'React or front-end hooks',
    'public': 'Static assets',
    'static': 'Static files',
    'config': 'Configuration files',
    'infrastructure': 'Infrastructure or IaC assets',
    'infra': 'Infrastructure or deployment scripts',
    'scripts': 'Automation scripts',
    'tests': 'Automated tests',
    'docs': 'Documentation',
    'notebooks': 'Data science notebooks',
    'data': 'Data assets',
    'core': 'Core domain logic',
    'lib': 'Shared libraries',
    'bin': 'Executable scripts',
}
ENTRYPOINT_FILENAMES = {
    'main.py',
    'app.py',
    'manage.py',
    'run.py',
    'wsgi.py',
    'asgi.py',
    'server.py',
    'index.js',
    'index.ts',
    'index.tsx',
    'index.jsx',
    'main.ts',
    'main.tsx',
    'main.jsx',
    'app.js',
    'cli.py',
}
def generate_auto_architecture(project_root: Path) -> Path:
    """Scan the project structure and produce a lightweight architecture summary."""
    project_root = project_root.resolve()
    config = Config(project_root=project_root)
    baseline_dir = config.baseline_dir
    baseline_dir.mkdir(parents=True, exist_ok=True)
    architecture_path = baseline_dir / 'architecture_auto.md'
    files = _collect_project_files(project_root)
    languages = _detect_languages(files)
    frameworks = _detect_frameworks(project_root)
    directories = _detect_key_directories(project_root)
    entrypoints = _detect_entrypoints(files, project_root)
    summary = _render_markdown_summary(
        languages=languages,
        frameworks=frameworks,
        directories=directories,
        entrypoints=entrypoints,
    )
    sanitized = redact_secrets(summary)
    encoded = sanitized.encode('utf-8')
    max_bytes = int(config.get('max_file_size_kb', 1024)) * 1024
    if len(encoded) > max_bytes:
        truncated = encoded[: max_bytes - 256].decode('utf-8', errors='ignore')
        sanitized = (
            f"{truncated}\n\n"
            "_Output truncated to satisfy baseline size limits._"
        )
    architecture_path.write_text(sanitized, encoding='utf-8')
    return architecture_path
def _collect_project_files(project_root: Path) -> List[Path]:
    files: List[Path] = []
    stack: List[Tuple[Path, int]] = [(project_root, 0)]
    while stack:
        current, depth = stack.pop()
        try:
            entries = list(current.iterdir())
        except (PermissionError, OSError):
            continue
        for entry in entries:
            name = entry.name
            if entry.is_symlink():
                continue
            if name in IGNORE_DIR_NAMES:
                continue
            if entry.is_dir():
                if depth + 1 >= MAX_TRAVERSAL_DEPTH:
                    continue
                if name.startswith('.') and name not in {'.context'}:
                    continue
                if _dir_too_large(entry):
                    continue
                stack.append((entry, depth + 1))
            elif entry.is_file():
                files.append(entry)
    return files
def _dir_too_large(directory: Path) -> bool:
    total = 0
    try:
        for root, dirs, files in os.walk(directory, topdown=True):
            rel_parts = Path(root).relative_to(directory).parts
            if len(rel_parts) >= MAX_TRAVERSAL_DEPTH:
                dirs[:] = []
            dirs[:] = [d for d in dirs if d not in IGNORE_DIR_NAMES and not d.startswith('.')]
            for filename in files:
                file_path = Path(root, filename)
                try:
                    total += file_path.stat().st_size
                except OSError:
                    continue
                if total > MAX_DIR_SIZE_BYTES:
                    return True
    except (OSError, ValueError):
        return False
    return False
def _detect_languages(files: Iterable[Path]) -> List[str]:
    counter: Counter[str] = Counter()
    extension_map = {
        ext: language
        for language, extensions in LANGUAGE_EXTENSIONS.items()
        for ext in extensions
    }
    for file_path in files:
        ext = file_path.suffix.lower()
        language = extension_map.get(ext)
        if language:
            counter[language] += 1
    return [language for language, _ in counter.most_common()]
def _detect_frameworks(project_root: Path) -> List[str]:
    candidates: Set[str] = set()
    package_json = project_root / 'package.json'
    if package_json.exists():
        try:
            data = json.loads(package_json.read_text(encoding='utf-8'))
        except (json.JSONDecodeError, OSError):
            data = {}
        for section in ('dependencies', 'devDependencies', 'peerDependencies'):
            section_data = data.get(section, {})
            if isinstance(section_data, dict):
                candidates.update(map(str.lower, section_data.keys()))
    requirements = project_root / 'requirements.txt'
    if requirements.exists():
        try:
            lines = requirements.read_text(encoding='utf-8').splitlines()
        except OSError:
            lines = []
        for line in lines:
            cleaned = line.strip()
            if not cleaned or cleaned.startswith('#'):
                continue
            pkg = cleaned.split('==')[0].split('[')[0].split('>=')[0].split('<=')[0]
            candidates.add(pkg.lower())
    pyproject = project_root / 'pyproject.toml'
    if pyproject.exists() and tomllib is not None:
        try:
            data = tomllib.loads(pyproject.read_text(encoding='utf-8'))
        except (OSError, TOMLDecodeError):
            data = {}
        project_section = data.get('project', {}) if isinstance(data, dict) else {}
        deps = project_section.get('dependencies', [])
        if isinstance(deps, list):
            candidates.update(
                dep.split(' ')[0].split('==')[0].split('>=')[0].split('<=')[0].lower()
                for dep in deps
                if isinstance(dep, str)
            )
        optional = project_section.get('optional-dependencies', {})
        if isinstance(optional, dict):
            for dep_list in optional.values():
                if isinstance(dep_list, list):
                    candidates.update(
                        dep.split(' ')[0].split('==')[0].split('>=')[0].split('<=')[0].lower()
                        for dep in dep_list
                        if isinstance(dep, str)
                    )
    frameworks: Set[str] = set()
    for name, patterns in FRAMEWORK_PATTERNS.items():
        if any(pattern.lower() in candidates for pattern in patterns):
            frameworks.add(name)
    return sorted(frameworks)
def _detect_key_directories(project_root: Path) -> List[Tuple[str, str]]:
    results: List[Tuple[str, str]] = []
    for directory in sorted(project_root.glob('*')):
        if not directory.is_dir():
            continue
        lower_name = directory.name.lower()
        if lower_name in IGNORE_DIR_NAMES:
            continue
        hint = DIRECTORY_HINTS.get(lower_name)
        if hint:
            results.append((f"{lower_name}/", hint))
            continue
        for sub in sorted(directory.glob('*')):
            if not sub.is_dir():
                continue
            sub_name = sub.name.lower()
            hint = DIRECTORY_HINTS.get(sub_name)
            if hint:
                rel = sub.relative_to(project_root)
                results.append((f"{rel.as_posix()}/", hint))
    seen: Set[str] = set()
    unique: List[Tuple[str, str]] = []
    for path, hint in results:
        if path not in seen:
            seen.add(path)
            unique.append((path, hint))
    return unique[:12]
def _detect_entrypoints(files: Iterable[Path], project_root: Path) -> List[str]:
    project_root = project_root.resolve()
    entrypoints: List[str] = []
    for file_path in files:
        if file_path.name.lower() in ENTRYPOINT_FILENAMES:
            try:
                rel = file_path.relative_to(project_root)
            except ValueError:
                continue
            entrypoints.append(rel.as_posix())
    package_json = project_root / 'package.json'
    if package_json.exists():
        try:
            data = json.loads(package_json.read_text(encoding='utf-8'))
            main_field = data.get('main')
            if isinstance(main_field, str):
                entrypoints.append(main_field)
        except (json.JSONDecodeError, OSError):
            pass
    seen: Set[str] = set()
    ordered: List[str] = []
    for item in entrypoints:
        if item not in seen:
            seen.add(item)
            ordered.append(item)
    return ordered[:12]
def _render_markdown_summary(
    *,
    languages: List[str],
    frameworks: List[str],
    directories: List[Tuple[str, str]],
    entrypoints: List[str],
) -> str:
    timestamp = datetime.utcnow().strftime('%Y-%m-%d %H:%M UTC')
    lines: List[str] = [
        '# Auto-Generated Architecture Overview',
        '',
        f'_Generated automatically on {timestamp}_',
        '',
    ]
    lines.append('## Languages')
    if languages:
        for language in languages:
            lines.append(f'- {language}')
    else:
        lines.append('- None detected')
    lines.append('')
    lines.append('## Frameworks & Libraries')
    if frameworks:
        for framework in frameworks:
            lines.append(f'- {framework}')
    else:
        lines.append('- None detected')
    lines.append('')
    lines.append('## Key Directories')
    if directories:
        for path, hint in directories:
            lines.append(f'- {path}  {hint}')
    else:
        lines.append('- No matching directories found')
    lines.append('')
    lines.append('## Entrypoints & Important Files')
    if entrypoints:
        for item in entrypoints:
            lines.append(f'- {item}')
    else:
        lines.append('- None detected')
    lines.append('')
    lines.append('---')
    lines.append(
        'This summary is generated locally by scanning the repository. '
        'Review and update as needed for accuracy.'
    )
    lines.append('')
    return '\n'.join(lines)
