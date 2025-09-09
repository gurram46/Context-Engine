#!/usr/bin/env python3
"""Test runner for Context Engine test suite."""

import unittest
import sys
import os
from pathlib import Path
import argparse
from io import StringIO

# Add the project root to Python path
project_root = Path(__file__).parent.parent
sys.path.insert(0, str(project_root))

def discover_tests(test_dir=None, pattern='test_*.py'):
    """Discover and return test suite."""
    if test_dir is None:
        test_dir = Path(__file__).parent
    
    loader = unittest.TestLoader()
    suite = loader.discover(str(test_dir), pattern=pattern)
    return suite

def run_tests(verbosity=2, failfast=False, pattern='test_*.py', specific_test=None):
    """Run the test suite with specified options."""
    if specific_test:
        # Run specific test
        loader = unittest.TestLoader()
        suite = loader.loadTestsFromName(specific_test)
    else:
        # Discover all tests
        suite = discover_tests(pattern=pattern)
    
    # Create test runner
    stream = StringIO()
    runner = unittest.TextTestRunner(
        stream=stream,
        verbosity=verbosity,
        failfast=failfast,
        buffer=True
    )
    
    # Run tests
    result = runner.run(suite)
    
    # Print results
    output = stream.getvalue()
    print(output)
    
    # Print summary
    print("\n" + "="*70)
    print(f"Tests run: {result.testsRun}")
    print(f"Failures: {len(result.failures)}")
    print(f"Errors: {len(result.errors)}")
    print(f"Skipped: {len(result.skipped)}")
    
    if result.failures:
        print("\nFAILURES:")
        for test, traceback in result.failures:
            print(f"- {test}: {traceback.split('AssertionError:')[-1].strip()}")
    
    if result.errors:
        print("\nERRORS:")
        for test, traceback in result.errors:
            print(f"- {test}: {traceback.split('Error:')[-1].strip()}")
    
    success = len(result.failures) == 0 and len(result.errors) == 0
    
    if success:
        print("\n✅ All tests passed!")
    else:
        print("\n❌ Some tests failed.")
    
    return success

def run_coverage_analysis():
    """Run tests with coverage analysis if coverage.py is available."""
    try:
        import coverage
        
        # Start coverage
        cov = coverage.Coverage()
        cov.start()
        
        # Run tests
        success = run_tests(verbosity=1)
        
        # Stop coverage and generate report
        cov.stop()
        cov.save()
        
        print("\n" + "="*70)
        print("COVERAGE REPORT:")
        print("="*70)
        
        # Generate coverage report
        cov.report(show_missing=True)
        
        # Generate HTML report
        html_dir = Path(__file__).parent / 'coverage_html'
        cov.html_report(directory=str(html_dir))
        print(f"\nHTML coverage report generated in: {html_dir}")
        
        return success
        
    except ImportError:
        print("Coverage.py not installed. Running tests without coverage analysis.")
        print("Install with: pip install coverage")
        return run_tests()

def run_specific_module_tests():
    """Run tests for specific modules."""
    modules = {
        'checklist': 'test_checklist.py',
        'auto_capture': 'test_auto_capture.py',
        'handoff_notes': 'test_handoff_notes.py'
    }
    
    print("Available test modules:")
    for name, filename in modules.items():
        print(f"  {name}: {filename}")
    
    choice = input("\nEnter module name to test (or 'all' for all modules): ").strip().lower()
    
    if choice == 'all':
        return run_tests()
    elif choice in modules:
        pattern = modules[choice]
        return run_tests(pattern=pattern)
    else:
        print(f"Unknown module: {choice}")
        return False

def setup_test_environment():
    """Set up test environment variables and paths."""
    # Set test environment variable
    os.environ['CONTEXT_ENGINE_TEST'] = '1'
    
    # Ensure test directories exist
    test_dir = Path(__file__).parent
    (test_dir / 'temp').mkdir(exist_ok=True)
    (test_dir / 'fixtures').mkdir(exist_ok=True)
    
    print("Test environment set up successfully.")

def cleanup_test_environment():
    """Clean up test environment."""
    import shutil
    
    test_dir = Path(__file__).parent
    
    # Clean up temporary directories
    temp_dirs = [
        test_dir / 'temp',
        test_dir / 'coverage_html'
    ]
    
    for temp_dir in temp_dirs:
        if temp_dir.exists():
            shutil.rmtree(temp_dir)
    
    # Remove coverage files
    coverage_files = [
        test_dir / '.coverage',
        test_dir.parent / '.coverage'
    ]
    
    for cov_file in coverage_files:
        if cov_file.exists():
            cov_file.unlink()
    
    print("Test environment cleaned up.")

def main():
    """Main test runner function."""
    parser = argparse.ArgumentParser(description='Context Engine Test Runner')
    parser.add_argument(
        '--coverage', '-c',
        action='store_true',
        help='Run tests with coverage analysis'
    )
    parser.add_argument(
        '--failfast', '-f',
        action='store_true',
        help='Stop on first failure'
    )
    parser.add_argument(
        '--verbose', '-v',
        action='count',
        default=2,
        help='Increase verbosity (use -vv for more verbose)'
    )
    parser.add_argument(
        '--pattern', '-p',
        default='test_*.py',
        help='Test file pattern (default: test_*.py)'
    )
    parser.add_argument(
        '--test', '-t',
        help='Run specific test (e.g., test_checklist.TestChecklist.test_all_docs_present)'
    )
    parser.add_argument(
        '--interactive', '-i',
        action='store_true',
        help='Interactive mode - choose which tests to run'
    )
    parser.add_argument(
        '--setup-only',
        action='store_true',
        help='Only set up test environment'
    )
    parser.add_argument(
        '--cleanup',
        action='store_true',
        help='Clean up test environment'
    )
    
    args = parser.parse_args()
    
    # Handle cleanup
    if args.cleanup:
        cleanup_test_environment()
        return
    
    # Set up test environment
    setup_test_environment()
    
    # Handle setup-only
    if args.setup_only:
        return
    
    try:
        # Run tests based on arguments
        if args.interactive:
            success = run_specific_module_tests()
        elif args.coverage:
            success = run_coverage_analysis()
        else:
            success = run_tests(
                verbosity=args.verbose,
                failfast=args.failfast,
                pattern=args.pattern,
                specific_test=args.test
            )
        
        # Exit with appropriate code
        sys.exit(0 if success else 1)
        
    except KeyboardInterrupt:
        print("\n\nTests interrupted by user.")
        sys.exit(1)
    except Exception as e:
        print(f"\nError running tests: {e}")
        sys.exit(1)
    finally:
        # Clean up environment variables
        if 'CONTEXT_ENGINE_TEST' in os.environ:
            del os.environ['CONTEXT_ENGINE_TEST']

if __name__ == '__main__':
    main()