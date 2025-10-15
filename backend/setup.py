"""Setup configuration for Context Engine"""

from setuptools import setup, find_packages
from pathlib import Path

# Read README for long description
readme_file = Path(__file__).parent / "README.md"
long_description = readme_file.read_text(encoding="utf-8") if readme_file.exists() else ""

setup(
    name="context-engine",
    version="1.0.0",
    author="Context Engine Team",
    description="CLI-based Context Engine for AI coding tools",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/gurram46/Context-Engine",
    packages=find_packages(),
    classifiers=[
        "Development Status :: 4 - Beta",
        "Intended Audience :: Developers",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
    ],
    python_requires=">=3.8",
    install_requires=[
        "click>=8.0.0",
        "tiktoken>=0.5.0",
        "requests>=2.28.0",
    ],
    entry_points={
        "console_scripts": [
            "context=context_engine.cli:main",
        ],
    },
    include_package_data=True,
    zip_safe=False,
)
