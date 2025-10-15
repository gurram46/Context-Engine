"""LangChain methods for Context Engine integration.

Provides LangChainProcessor and ProcessingResult classes for enhanced
content processing using LangChain capabilities.
"""

import json
import os
from typing import List, Dict, Any, Optional, Union
from pathlib import Path
from dataclasses import dataclass
from datetime import datetime

from context_engine.core.logger import setup_logger

logger = setup_logger(__name__)

@dataclass
class ProcessingResult:
    """Result of LangChain processing operations."""
    success: bool
    content: str
    metadata: Dict[str, Any]
    error: Optional[str] = None
    processing_time: Optional[float] = None

class LangChainProcessor:
    """Main LangChain processor for Context Engine integration.

    Provides enhanced content processing capabilities using LangChain methods
    for summarization, analysis, and knowledge extraction.
    """

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        """Initialize LangChain processor with configuration.

        Args:
            config: Configuration dictionary for LangChain settings
        """
        self.config = config or {}
        self.openai_api_key = os.getenv('OPENAI_API_KEY')
        self.anthropic_api_key = os.getenv('ANTHROPIC_API_KEY')

        # Initialize LangChain components
        self._initialize_components()

    def _initialize_components(self):
        """Initialize LangChain components based on configuration."""
        try:
            # Import LangChain components here to avoid import errors if not installed
            from langchain.llms import OpenAI, Anthropic
            from langchain.embeddings import OpenAIEmbeddings
            from langchain.text_splitter import RecursiveCharacterTextSplitter
            from langchain.chains import LLMChain
            from langchain.prompts import PromptTemplate

            # Initialize LLM based on available API keys
            if self.openai_api_key:
                self.llm = OpenAI(openai_api_key=self.openai_api_key)
                self.embeddings = OpenAIEmbeddings(openai_api_key=self.openai_api_key)
            elif self.anthropic_api_key:
                self.llm = Anthropic(anthropic_api_key=self.anthropic_api_key)
                self.embeddings = None  # Use OpenAI embeddings separately if needed
            else:
                logger.warning("No API keys found for LLM providers")
                self.llm = None
                self.embeddings = None

            # Initialize text splitter
            self.text_splitter = RecursiveCharacterTextSplitter(
                chunk_size=1000,
                chunk_overlap=200
            )

            # Initialize chains
            self._initialize_chains()

        except ImportError as e:
            logger.error(f"LangChain not installed: {e}")
            self.llm = None
            self.embeddings = None
            self.text_splitter = None
        except Exception as e:
            logger.error(f"Error initializing LangChain components: {e}")
            self.llm = None
            self.embeddings = None
            self.text_splitter = None

    def _initialize_chains(self):
        """Initialize LangChain chains for different processing tasks."""
        try:
            from langchain.chains import LLMChain
            from langchain.prompts import PromptTemplate

            # Summarization chain
            self.summarize_prompt = PromptTemplate(
                input_variables=["text"],
                template="Summarize the following text concisely:\n\n{text}\n\nSummary:"
            )

            # Analysis chain
            self.analyze_prompt = PromptTemplate(
                input_variables=["text"],
                template="Analyze the following code/text and provide insights:\n\n{text}\n\nAnalysis:"
            )

            # Question answering chain
            self.qa_prompt = PromptTemplate(
                input_variables=["context", "question"],
                template="Based on the following context, answer the question:\n\nContext: {context}\n\nQuestion: {question}\n\nAnswer:"
            )

            if self.llm:
                self.summarize_chain = LLMChain(llm=self.llm, prompt=self.summarize_prompt)
                self.analyze_chain = LLMChain(llm=self.llm, prompt=self.analyze_prompt)
                self.qa_chain = LLMChain(llm=self.llm, prompt=self.qa_prompt)

        except Exception as e:
            logger.error(f"Error initializing chains: {e}")
            self.summarize_chain = None
            self.analyze_chain = None
            self.qa_chain = None

    def summarize_text(self, text: str, max_length: int = 500) -> ProcessingResult:
        """Summarize text using LangChain.

        Args:
            text: Text to summarize
            max_length: Maximum length of summary

        Returns:
            ProcessingResult with summary
        """
        start_time = datetime.now()

        try:
            if not self.llm or not self.summarize_chain:
                return ProcessingResult(
                    success=False,
                    content="",
                    metadata={"error": "LLM not available"},
                    error="LangChain LLM not initialized"
                )

            # Split text if too long
            if len(text) > 8000:
                chunks = self.text_splitter.split_text(text)
                summaries = []

                for chunk in chunks:
                    result = self.summarize_chain.run(text=chunk)
                    summaries.append(result)

                # Combine summaries
                combined_summary = " ".join(summaries)

                # Final summary if still too long
                if len(combined_summary) > max_length * 2:
                    final_result = self.summarize_chain.run(text=combined_summary)
                    summary = final_result[:max_length]
                else:
                    summary = combined_summary[:max_length]
            else:
                result = self.summarize_chain.run(text=text)
                summary = result[:max_length]

            processing_time = (datetime.now() - start_time).total_seconds()

            return ProcessingResult(
                success=True,
                content=summary,
                metadata={
                    "original_length": len(text),
                    "summary_length": len(summary),
                    "processing_time": processing_time
                },
                processing_time=processing_time
            )

        except Exception as e:
            logger.error(f"Error in summarize_text: {e}")
            return ProcessingResult(
                success=False,
                content="",
                metadata={"error": str(e)},
                error=str(e)
            )

    def analyze_text(self, text: str) -> ProcessingResult:
        """Analyze text using LangChain.

        Args:
            text: Text to analyze

        Returns:
            ProcessingResult with analysis
        """
        start_time = datetime.now()

        try:
            if not self.llm or not self.analyze_chain:
                return ProcessingResult(
                    success=False,
                    content="",
                    metadata={"error": "LLM not available"},
                    error="LangChain LLM not initialized"
                )

            result = self.analyze_chain.run(text=text)
            processing_time = (datetime.now() - start_time).total_seconds()

            return ProcessingResult(
                success=True,
                content=result,
                metadata={
                    "text_length": len(text),
                    "analysis_length": len(result),
                    "processing_time": processing_time
                },
                processing_time=processing_time
            )

        except Exception as e:
            logger.error(f"Error in analyze_text: {e}")
            return ProcessingResult(
                success=False,
                content="",
                metadata={"error": str(e)},
                error=str(e)
            )

    def answer_question(self, context: str, question: str) -> ProcessingResult:
        """Answer a question based on context using LangChain.

        Args:
            context: Context information
            question: Question to answer

        Returns:
            ProcessingResult with answer
        """
        start_time = datetime.now()

        try:
            if not self.llm or not self.qa_chain:
                return ProcessingResult(
                    success=False,
                    content="",
                    metadata={"error": "LLM not available"},
                    error="LangChain LLM not initialized"
                )

            result = self.qa_chain.run(context=context, question=question)
            processing_time = (datetime.now() - start_time).total_seconds()

            return ProcessingResult(
                success=True,
                content=result,
                metadata={
                    "context_length": len(context),
                    "question_length": len(question),
                    "answer_length": len(result),
                    "processing_time": processing_time
                },
                processing_time=processing_time
            )

        except Exception as e:
            logger.error(f"Error in answer_question: {e}")
            return ProcessingResult(
                success=False,
                content="",
                metadata={"error": str(e)},
                error=str(e)
            )

    def create_embeddings(self, texts: List[str]) -> ProcessingResult:
        """Create embeddings for texts using LangChain.

        Args:
            texts: List of texts to embed

        Returns:
            ProcessingResult with embeddings
        """
        start_time = datetime.now()

        try:
            if not self.embeddings:
                return ProcessingResult(
                    success=False,
                    content="",
                    metadata={"error": "Embeddings not available"},
                    error="LangChain embeddings not initialized"
                )

            embeddings = self.embeddings.embed_documents(texts)
            processing_time = (datetime.now() - start_time).total_seconds()

            return ProcessingResult(
                success=True,
                content=json.dumps(embeddings),
                metadata={
                    "text_count": len(texts),
                    "embedding_dimension": len(embeddings[0]) if embeddings else 0,
                    "processing_time": processing_time
                },
                processing_time=processing_time
            )

        except Exception as e:
            logger.error(f"Error in create_embeddings: {e}")
            return ProcessingResult(
                success=False,
                content="",
                metadata={"error": str(e)},
                error=str(e)
            )

    def process_files(self, file_paths: List[str], operation: str = "summarize") -> ProcessingResult:
        """Process multiple files with LangChain.

        Args:
            file_paths: List of file paths to process
            operation: Operation to perform (summarize, analyze, etc.)

        Returns:
            ProcessingResult with combined results
        """
        start_time = datetime.now()
        results = []

        try:
            for file_path in file_paths:
                path = Path(file_path)
                if not path.exists():
                    results.append({
                        "file": str(path),
                        "success": False,
                        "error": "File not found"
                    })
                    continue

                try:
                    with open(path, 'r', encoding='utf-8') as f:
                        content = f.read()

                    if operation == "summarize":
                        result = self.summarize_text(content)
                    elif operation == "analyze":
                        result = self.analyze_text(content)
                    else:
                        result = ProcessingResult(
                            success=False,
                            content="",
                            metadata={"error": f"Unknown operation: {operation}"},
                            error=f"Unknown operation: {operation}"
                        )

                    results.append({
                        "file": str(path),
                        "success": result.success,
                        "content": result.content,
                        "metadata": result.metadata,
                        "error": result.error
                    })

                except Exception as e:
                    logger.error(f"Error processing file {file_path}: {e}")
                    results.append({
                        "file": str(path),
                        "success": False,
                        "error": str(e)
                    })

            processing_time = (datetime.now() - start_time).total_seconds()

            return ProcessingResult(
                success=True,
                content=json.dumps(results, indent=2),
                metadata={
                    "file_count": len(file_paths),
                    "successful_files": len([r for r in results if r["success"]]),
                    "failed_files": len([r for r in results if not r["success"]]),
                    "operation": operation,
                    "processing_time": processing_time
                },
                processing_time=processing_time
            )

        except Exception as e:
            logger.error(f"Error in process_files: {e}")
            return ProcessingResult(
                success=False,
                content="",
                metadata={"error": str(e)},
                error=str(e)
            )