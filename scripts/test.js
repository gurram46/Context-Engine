#!/usr/bin/env node

/**
 * Test script for Context Engine
 * Runs both Node.js and Python tests
 */

const { spawn } = require('cross-spawn');
const chalk = require('chalk');
const path = require('path');

class Tester {
  constructor() {
    this.rootDir = path.resolve(__dirname, '..');
  }

  async run() {
    console.log(chalk.cyan('🧪 Running Context Engine tests...'));

    try {
      // Test Node.js frontend
      await this.testNodejs();

      // Test Python backend
      await this.testPython();

      // Test integration
      await this.testIntegration();

      console.log(chalk.green('\n✅ All tests passed!'));

    } catch (error) {
      console.error(chalk.red('\n❌ Tests failed:'), error.message);
      process.exit(1);
    }
  }

  async testNodejs() {
    console.log(chalk.blue('\n📋 Testing Node.js frontend...'));

    return this.execCommand('npm', ['test'], {
      cwd: path.join(this.rootDir, 'ui')
    });
  }

  async testPython() {
    console.log(chalk.blue('\n🐍 Testing Python backend...'));

    const pythonCmd = this.getPythonCommand();
    if (!pythonCmd) {
      console.log(chalk.yellow('⚠️  Python not available, skipping Python tests'));
      return;
    }

    return this.execCommand(pythonCmd, ['-m', 'pytest', 'tests/', '-v'], {
      cwd: path.join(this.rootDir, 'backend')
    });
  }

  async testIntegration() {
    console.log(chalk.blue('\n🔗 Testing integration...'));

    // Test that CLI commands work
    const contextEngineCmd = path.join(this.rootDir, 'bin', 'context-engine.js');

    return this.execCommand('node', [contextEngineCmd, '--help'], {
      cwd: this.rootDir
    });
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
  const tester = new Tester();
  tester.run().catch(error => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = Tester;