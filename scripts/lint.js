#!/usr/bin/env node

/**
 * Lint script for Context Engine
 * Lints both Node.js and Python code
 */

const { spawn } = require('cross-spawn');
const chalk = require('chalk');
const path = require('path');

class Linter {
  constructor() {
    this.rootDir = path.resolve(__dirname, '..');
  }

  async run() {
    console.log(chalk.cyan('🔍 Linting Context Engine code...'));

    try {
      // Lint Node.js frontend
      await this.lintNodejs();

      // Lint Python backend
      await this.lintPython();

      console.log(chalk.green('\n✅ Linting completed!'));

    } catch (error) {
      console.error(chalk.red('\n❌ Linting failed:'), error.message);
      process.exit(1);
    }
  }

  async lintNodejs() {
    console.log(chalk.blue('\n📋 Linting Node.js frontend...'));

    return this.execCommand('npx', ['eslint', 'ui/', 'lib/', 'bin/', 'scripts/'], {
      cwd: this.rootDir
    });
  }

  async lintPython() {
    console.log(chalk.blue('\n🐍 Linting Python backend...'));

    const pythonCmd = this.getPythonCommand();
    if (!pythonCmd) {
      console.log(chalk.yellow('⚠️  Python not available, skipping Python linting'));
      return;
    }

    // Try to use black, flake8, or pylint
    const linters = ['black', 'flake8', 'pylint'];

    for (const linter of linters) {
      try {
        await this.execCommand(pythonCmd, ['-m', linter, '--check', 'python/', 'backend/'], {
          cwd: this.rootDir
        });
        console.log(chalk.green(`✅ ${linter} checks passed`));
        return;
      } catch (error) {
        // Try next linter
      }
    }

    console.log(chalk.yellow('⚠️  No Python linter found (black, flake8, or pylint)'));
  }

  getPythonCommand() {
    const { spawnSync } = require('child_process');

    for (const cmd of ['python3', 'python', 'py']) {
      try {
        const result = spawnSync(cmd, ['--version'], {
          stdio: 'pipe'
        });
        if (result.status === 0) {
          return cmd;
        }
      } catch (error) {
        // Continue
      }
    }
    return null;
  }

  async execCommand(command, args, options = {}) {
    return new Promise((resolve, reject) => {
      const child = spawn(command, args, {
        stdio: 'inherit',
        ...options
      });

      child.on('close', (code) => {
        if (code === 0) {
          resolve();
        } else {
          reject(new Error(`Command failed with code ${code}`));
        }
      });

      child.on('error', reject);
    });
  }
}

if (require.main === module) {
  const linter = new Linter();
  linter.run().catch(error => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = Linter;