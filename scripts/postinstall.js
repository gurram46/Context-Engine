#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const os = require('os');

/**
 * Post-install script for Context Engine
 * Handles setup tasks after npm installation
 */

class PostInstaller {
    constructor() {
        this.projectRoot = process.cwd();
        this.isGlobalInstall = this.checkGlobalInstall();
    }

    /**
     * Check if this is a global installation
     */
    checkGlobalInstall() {
        const npmConfigPrefix = process.env.npm_config_prefix;
        const npmConfigGlobal = process.env.npm_config_global;
        
        return npmConfigGlobal === 'true' || 
               (npmConfigPrefix && this.projectRoot.startsWith(npmConfigPrefix));
    }

    /**
     * Make CLI script executable on Unix-like systems
     */
    makeExecutable() {
        if (os.platform() === 'win32') {
            return; // Windows doesn't need chmod
        }

        const cliScript = path.join(this.projectRoot, 'bin', 'context-engine.js');
        
        try {
            if (fs.existsSync(cliScript)) {
                fs.chmodSync(cliScript, '755');
                console.log('✓ Made CLI script executable');
            }
        } catch (error) {
            console.warn('Warning: Could not make CLI script executable:', error.message);
        }
    }

    /**
     * Create necessary directories
     */
    createDirectories() {
        const dirs = [
            path.join(this.projectRoot, 'context_engine', 'data'),
            path.join(this.projectRoot, 'context_engine', 'logs'),
            path.join(this.projectRoot, 'context_engine', 'sessions')
        ];

        dirs.forEach(dir => {
            try {
                if (!fs.existsSync(dir)) {
                    fs.mkdirSync(dir, { recursive: true });
                }
            } catch (error) {
                console.warn(`Warning: Could not create directory ${dir}:`, error.message);
            }
        });
    }

    /**
     * Check Python availability
     */
    checkPython() {
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
                        console.log(`✓ Found Python: ${version.trim()}`);
                        return true;
                    }
                }
            } catch (error) {
                // Continue to next candidate
            }
        }
        
        console.warn('⚠ Python 3.8+ not found. Please install Python 3.8 or higher.');
        console.warn('  Download from: https://www.python.org/downloads/');
        return false;
    }

    /**
     * Show installation success message
     */
    showSuccessMessage() {
        console.log('\n🎉 Context Engine installed successfully!');
        console.log('\nQuick start:');
        
        if (this.isGlobalInstall) {
            console.log('  context-engine init          # Initialize in current directory');
            console.log('  context-engine start-session # Start a new session');
            console.log('  ce --help                    # Show help (short alias)');
        } else {
            console.log('  npx context-engine init          # Initialize in current directory');
            console.log('  npx context-engine start-session # Start a new session');
            console.log('  npx ce --help                    # Show help (short alias)');
        }
        
        console.log('\nFor more information, visit: https://github.com/your-org/context-engine');
    }

    /**
     * Run post-installation tasks
     */
    run() {
        console.log('Setting up Context Engine...');
        
        try {
            this.makeExecutable();
            this.createDirectories();
            
            const pythonAvailable = this.checkPython();
            
            if (!pythonAvailable) {
                console.log('\n⚠ Python setup required:');
                console.log('  1. Install Python 3.8+');
                console.log('  2. Run: pip install -r requirements.txt');
                console.log('  3. Then use: context-engine --help');
            } else {
                this.showSuccessMessage();
            }
            
        } catch (error) {
            console.error('Post-install setup failed:', error.message);
            process.exit(1);
        }
    }
}

// Run post-install setup
if (require.main === module) {
    const installer = new PostInstaller();
    installer.run();
}

module.exports = PostInstaller;