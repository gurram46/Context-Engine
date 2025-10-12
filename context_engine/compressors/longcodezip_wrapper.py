"""Wrapper for LongCodeZip compression library."""

import signal
import threading
import time
import os
import sys
from typing import Tuple, Optional, Any
from contextlib import contextmanager

# Add the temp_longcodezip directory to Python path at module level to ensure import works
_LONGCODEZIP_PATH = os.path.join(os.path.dirname(__file__), '..', '..', 'temp_longcodezip')
if os.path.exists(_LONGCODEZIP_PATH):
    _LONGCODEZIP_PATH = os.path.abspath(_LONGCODEZIP_PATH)
    if _LONGCODEZIP_PATH not in sys.path:
        sys.path.insert(0, _LONGCODEZIP_PATH)

class LongcodezipWrapper:
    """Wrapper for LongCodeZip's CodeCompressor with lazy initialization and error handling."""
    
    def __init__(self, model_name: str = "Qwen/Qwen2.5-Coder-0.5B-Instruct", rate: float = 0.5):
        """
        Initialize the LongCodeZip wrapper.
        
        Args:
            model_name: Name of the model to use for compression
            rate: Compression rate (0.0 to 1.0, where 0.5 means 50% compression)
        """
        self._model_name = model_name
        self._rate = rate
        self._compressor = None
        self._compressor_lock = threading.Lock()

    def _initialize_compressor(self) -> Any:
        """Lazily initialize the CodeCompressor."""
        if self._compressor is not None:
            return self._compressor
            
        with self._compressor_lock:
            if self._compressor is not None:
                return self._compressor
            
            try:
                # Import LongCodeZip from the local copy
                import longcodezip
                self._compressor = longcodezip.CodeCompressor(model_name=self._model_name)
            except ImportError:
                try:
                    # Try to import from globally installed version (if available)
                    from LongCodeZip import CodeCompressor
                    self._compressor = CodeCompressor(model_name=self._model_name)
                except ImportError:
                    raise ImportError(
                        "LongCodeZip is not available. The library needs to be properly installed or "
                        "the temp_longcodezip directory must be present in the project root."
                    )
            except Exception as e:
                raise RuntimeError(f"Failed to initialize LongCodeZip compressor: {e}")
        
        return self._compressor

    @contextmanager
    def _timeout_context(self, seconds: int):
        """Context manager for timeout handling."""
        if hasattr(signal, 'SIGALRM') and os.name != 'nt':  # Not on Windows
            def timeout_handler(signum, frame):
                raise TimeoutError(f"Compression timed out after {seconds} seconds")
            
            # Set the signal handler
            old_handler = signal.signal(signal.SIGALRM, timeout_handler)
            signal.alarm(seconds)
            
            try:
                yield
            finally:
                signal.alarm(0)  # Cancel the alarm
                signal.signal(signal.SIGALRM, old_handler)
        else:
            # Fallback for Windows which doesn't support SIGALRM
            class TimeoutContainer:
                def __init__(self):
                    self.timed_out = False
                    
            container = TimeoutContainer()
            
            def timeout_func():
                time.sleep(seconds)
                container.timed_out = True
                
            timer = threading.Thread(target=timeout_func, daemon=True)
            timer.start()
            
            try:
                yield
                if container.timed_out:
                    raise TimeoutError(f"Compression timed out after {seconds} seconds")
            finally:
                # Timer is a daemon thread, so it will be cleaned up automatically
                pass

    def compress(self, code: str, query: str, instruction: str = "Summarize the code based on the query.") -> Tuple[Optional[str], Optional[str], Optional[float]]:
        """
        Compress code using LongCodeZip.
        
        Args:
            code: The code to compress
            query: Query to guide compression
            instruction: Instruction for the compression model
            
        Returns:
            Tuple of (compressed_code, compressed_prompt, compression_ratio) or (None, None, None) if failed
        """
        try:
            # Initialize compressor lazily
            compressor = self._initialize_compressor()
            
            # Use timeout to prevent hanging on large inputs
            with self._timeout_context(120):  # 120 second timeout for large models
                result = compressor.compress_code_file(
                    code=code,
                    query=query,
                    instruction=instruction,
                    rate=self._rate
                )
                
                compressed_code = result.get('compressed_code')
                compressed_prompt = result.get('compressed_prompt')
                compression_ratio = result.get('compression_ratio')
                
                return compressed_code, compressed_prompt, compression_ratio
                
        except ImportError as e:
            print(f"Import Error: {e}")
            return None, None, None
        except TimeoutError as e:
            print(f"Compression timed out: {e}")
            return None, None, None
        except Exception as e:
            print(f"Compression failed: {e}")
            return None, None, None