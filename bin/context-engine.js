#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

/**
 * Cross-platform Context Engine CLI wrapper
 * This Node.js script provides a unified interface to the Python CLI
 */

class ContextEngineCLI {
    constructor() {
        this.projectRoot = path.resolve(__dirname, '..');
        this.pythonScript = path.join(this.projectRoot, 'context_engine', 'scripts', 'cli.py');
        this.pythonExecutable = this.findPythonExecutable();
    }

    /**
     * Find the appropriate Python executable
     */
    findPythonExecutable() {
        const candidates = ['python3', 'python', 'py'];
        
        for (const candidate of candidates) {
            try {
                const result = spawn.sync(candidate, ['--version'], { 
                    stdio: 'pipe',
                    encoding: 'utf8'
                });
                
                if (result.status === 0) {
                    const version = result.stdout || result.stderr;
                    if (version.includes('Python 3.')) {
                        return candidate;
                    }
                }
            } catch (error) {
                // Continue to next candidate
            }
        }
        
        throw new Error('Python 3.8+ is required but not found. Please install Python 3.8 or higher.');
    }

    /**
     * Check if Python dependencies are installed
     */
    checkDependencies() {
        const requirementsFile = path.join(this.projectRoot, 'requirements.txt');
        
        if (!fs.existsSync(requirementsFile)) {
            console.warn('Warning: requirements.txt not found. Some features may not work.');
            return true;
        }

        // Try to import a key dependency to check if installed
        try {
            const result = spawn.sync(this.pythonExecutable, ['-c', 'import psutil, pathlib'], {
                stdio: 'pipe'
            });
            
            return result.status === 0;
        } catch (error) {
            return false;
        }
    }

    /**
     * Install Python dependencies
     */
    installDependencies() {
        console.log('Installing Python dependencies...');
        const requirementsFile = path.join(this.projectRoot, 'requirements.txt');
        
        try {
            const result = spawn.sync(this.pythonExecutable, ['-m', 'pip', 'install', '-r', requirementsFile], {
                stdio: 'inherit'
            });
            
            if (result.status !== 0) {
                throw new Error('Failed to install Python dependencies');
            }
            
            console.log('Dependencies installed successfully!');
        } catch (error) {
            console.error('Error installing dependencies:', error.message);
            console.error('Please run: pip install -r requirements.txt');
            process.exit(1);
        }
    }

    /**
     * Execute the Python CLI with provided arguments
     */
    run(args) {
        // Check if Python script exists
        if (!fs.existsSync(this.pythonScript)) {
            console.error(`Error: Python CLI script not found at ${this.pythonScript}`);
            process.exit(1);
        }

        // Check dependencies
        if (!this.checkDependencies()) {
            console.log('Python dependencies not found or incomplete.');
            this.installDependencies();
        }

        // Execute Python CLI
        const pythonArgs = [this.pythonScript, ...args];
        
        try {
            const child = spawn(this.pythonExecutable, pythonArgs, {
                stdio: 'inherit',
                cwd: this.projectRoot
            });

            child.on('error', (error) => {
                console.error('Error executing Context Engine:', error.message);
                process.exit(1);
            });

            child.on('exit', (code) => {
                process.exit(code || 0);
            });

        } catch (error) {
            console.error('Failed to start Context Engine:', error.message);
            process.exit(1);
        }
    }

    /**
     * Show help information
     */
    showHelp() {
        console.log(`
Context Engine CLI Wrapper
`);
        console.log('A powerful context management tool for development workflows');
        console.log('with automatic log capture and intelligent parsing.\n');
        console.log('Usage:');
        console.log('  context-engine <command> [options]');
        console.log('  ce <command> [options]\n');
        console.log('Available commands:');
        console.log('  init           Initialize context engine in current directory');
        console.log('  start-session  Start a new development session');
        console.log('  stop-session   Stop current session and save context');
        console.log('  set-scope      Set project scope for context tracking');
        console.log('  add-docs       Add documentation files to context');
        console.log('  inject         Inject context into current session');
        console.log('  checklist      Run project readiness checklist');
        console.log('  --help, -h     Show this help message\n');
        console.log('For detailed help on each command, use:');
        console.log('  context-engine <command> --help\n');
        console.log('System Requirements:');
        console.log('  - Python 3.8+');
        console.log('  - Node.js 14+\n');
    }
}

// Main execution
function main() {
    const args = process.argv.slice(2);
    
    // Show help if no arguments or help requested
    if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
        const cli = new ContextEngineCLI();
        cli.showHelp();
        return;
    }

    // Show version
    if (args.includes('--version') || args.includes('-v')) {
        const packageJson = require('../package.json');
        console.log(`Context Engine v${packageJson.version}`);
        return;
    }

    try {
        const cli = new ContextEngineCLI();
        cli.run(args);
    } catch (error) {
        console.error('Error:', error.message);
        process.exit(1);
    }
}

if (require.main === module) {
    main();
}

module.exports = ContextEngineCLI;