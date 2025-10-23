"""Setup configuration for Context Engine"""

from setuptools import setup, find_packages
from pathlib import Path

# Read README for long description
readme_file = Path(__file__).parent / "README.md"
long_description = readme_file.read_text(encoding="utf-8") if readme_file.exists() else ""

setup(
    name="context-engine-dev",
    version="1.2.0",
    author="Context Engine Team",
    description="Context Engine CLI - Compress the Chaos",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/gurram46/Context-Engine",
    packages=find_packages(where="backend"),
    package_dir={"": "backend"},
    classifiers=[
        "Development Status :: 4 - Beta",
        "Intended Audience :: Developers",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "Topic :: Software Development :: Code Generators",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
    ],
    python_requires=">=3.8",
    install_requires=[
        "click>=8.0.0",
        "tiktoken>=0.5.0",
        "requests>=2.28.0",
        "rich>=13.0.0",
        "pyyaml>=6.0",
        "openrouter>=0.2.0",
    ],
    entry_points={
        "console_scripts": [
            "context-engine-backend=context_engine.main:main",
        ],
    },
    package_data={
        "context_engine": ["*.yaml", "*.yml", "templates/*.md"],
    },
    include_package_data=True,
    zip_safe=False,
    keywords=["context", "compression", "cli", "ai", "documentation", "productivity"],
    project_urls={
        "Bug Reports": "https://github.com/gurram46/Context-Engine/issues",
        "Source": "https://github.com/gurram46/Context-Engine",
        "Documentation": "https://github.com/gurram46/Context-Engine/blob/main/README.md",
    },
)
