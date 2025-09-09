#!/usr/bin/env python3
"""Setup script for Context Engine."""

from setuptools import setup, find_packages
from pathlib import Path

# Read the contents of README file
this_directory = Path(__file__).parent
long_description = (this_directory / "README.md").read_text(encoding='utf-8') if (this_directory / "README.md").exists() else ""

setup(
    name="context-engine",
    version="0.1.0",
    description="Local project brain for AI tools - intelligent code context and session management",
    long_description=long_description,
    long_description_content_type="text/markdown",
    author="Context Engine Team",
    author_email="team@contextengine.dev",
    url="https://github.com/contextengine/context-engine",
    packages=find_packages(),
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Topic :: Software Development :: Tools",
        "Topic :: Text Processing :: Indexing",
        "Topic :: Scientific/Engineering :: Artificial Intelligence",
    ],
    python_requires=">=3.8",
    install_requires=[
        "PyYAML>=6.0",
        "gitpython>=3.1.0",
        "sentence-transformers>=2.2.0",
        "faiss-cpu>=1.7.0",
        "numpy>=1.21.0",
        "scipy>=1.7.0",
        "torch>=1.9.0",
        "transformers>=4.20.0",
        "chardet>=5.0.0",
        "python-magic>=0.4.27",
        "watchdog>=2.1.0",
        "requests>=2.28.0",
        "tqdm>=4.64.0",
    ],
    extras_require={
        "dev": [
            "pytest>=7.0.0",
            "pytest-cov>=4.0.0",
            "black>=22.0.0",
            "flake8>=5.0.0",
            "mypy>=0.991",
            "pre-commit>=2.20.0",
        ],
        "gpu": [
            "faiss-gpu>=1.7.0",
        ],
    },
    entry_points={
        "console_scripts": [
            "context-engine=context_engine.scripts.cli:main",
        ],
    },
    include_package_data=True,
    package_data={
        "context_engine": [
            "templates/*.yml",
            "templates/*.md",
        ],
    },
    zip_safe=False,
)