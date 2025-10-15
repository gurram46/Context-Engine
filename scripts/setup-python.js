#!/usr/bin/env node

/**
 * Python Dependencies Setup Script
 * Automatically installs Python dependencies for Context Engine
 */

const fs = require('fs-extra');
const path = require('path');
const { spawn } = require('cross-spawn');
const chalk = require('chalk');

class PythonSetup {
  constructor() {
    this.rootDir = path.resolve(__dirname, '..');
    this.pythonDir = path.join(this.rootDir, 'python');
    this.venvDir = path.join(this.pythonDir, '.venv');
    this.requirementsFile = path.join(this.rootDir, 'requirements.txt');
    this.setupLockFile = path.join(this.pythonDir, '.setup-complete');
  }

  async run() {
    console.log(chalk.cyan('🐍 Setting up Python dependencies for Context Engine...'));

    try {
      // Check if setup is already complete
      if (await fs.pathExists(this.setupLockFile)) {
        console.log(chalk.green('✓ Python dependencies already set up'));
        return;
      }

      // Check Python availability
      const pythonCmd = await this.findPythonCommand();
      if (!pythonCmd) {
        console.error(chalk.red('❌ Python 3.7+ is required but not found'));
        console.log(chalk.yellow('Please install Python from https://python.org'));
        process.exit(1);
      }

      console.log(chalk.blue(`✓ Found Python: ${pythonCmd}`));

      // Create virtual environment
      await this.createVirtualEnvironment(pythonCmd);

      // Install dependencies
      await this.installDependencies();

      // Create setup lock file
      await fs.writeFile(this.setupLockFile, new Date().toISOString());

      console.log(chalk.green('✅ Python dependencies setup complete!'));

    } catch (error) {
      console.error(chalk.red('❌ Failed to setup Python dependencies:'), error.message);
      process.exit(1);
    }
  }

  async findPythonCommand() {
    const possibleCommands = [
      'python3',
      'python',
      'py',
      'python3.9',
      'python3.8',
      'python3.7',
      'python3.10',
      'python3.11',
      'python3.12'
    ];

    for (const cmd of possibleCommands) {
      try {
        const result = await this.execCommand(cmd, ['--version']);
        if (result.success) {
          // Check if version is 3.7+
          const versionMatch = result.stdout.match(/Python (\d+)\.(\d+)/);
          if (versionMatch) {
            const major = parseInt(versionMatch[1]);
            const minor = parseInt(versionMatch[2]);
            if (major >= 3 && minor >= 7) {
              return cmd;
            }
          }
        }
      } catch (error) {
        // Continue to next command
      }
    }

    return null;
  }

  async createVirtualEnvironment(pythonCmd) {
    console.log(chalk.blue('📦 Creating Python virtual environment...'));

    // Remove existing venv if it exists
    if (await fs.pathExists(this.venvDir)) {
      await fs.remove(this.venvDir);
    }

    // Create new virtual environment
    const result = await this.execCommand(pythonCmd, ['-m', 'venv', '.venv'], {
      cwd: this.pythonDir
    });

    if (!result.success) {
      throw new Error(`Failed to create virtual environment: ${result.stderr}`);
    }
  }

  async installDependencies() {
    console.log(chalk.blue('📥 Installing Python dependencies...'));

    const pipCmd = this.getPipCommand();

    // Ensure pip is up to date
    await this.execCommand(pipCmd, ['install', '--upgrade', 'pip'], {
      cwd: this.pythonDir
    });

    // Install requirements
    const result = await this.execCommand(pipCmd, ['install', '-r', this.requirementsFile], {
      cwd: this.rootDir
    });

    if (!result.success) {
      throw new Error(`Failed to install dependencies: ${result.stderr}`);
    }
  }

  getPipCommand() {
    const platform = process.platform;

    if (platform === 'win32') {
      return path.join(this.venvDir, 'Scripts', 'pip.exe');
    } else {
      return path.join(this.venvDir, 'bin', 'pip');
    }
  }

  getPythonCommand() {
    const platform = process.platform;

    if (platform === 'win32') {
      return path.join(this.venvDir, 'Scripts', 'python.exe');
    } else {
      return path.join(this.venvDir, 'bin', 'python');
    }
  }

  async execCommand(command, args, options = {}) {
    return new Promise((resolve) => {
      const child = spawn(command, args, {
        stdio: 'pipe',
        ...options
      });

      let stdout = '';
      let stderr = '';

      child.stdout?.on('data', (data) => {
        stdout += data.toString();
      });

      child.stderr?.on('data', (data) => {
        stderr += data.toString();
      });

      child.on('close', (code) => {
        resolve({
          success: code === 0,
          code,
          stdout: stdout.trim(),
          stderr: stderr.trim()
        });
      });

      child.on('error', (error) => {
        resolve({
          success: false,
          code: -1,
          stdout: '',
          stderr: error.message
        });
      });
    });
  }
}

// Run setup if called directly
if (require.main === module) {
  const setup = new PythonSetup();
  setup.run().catch(error => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = PythonSetup;