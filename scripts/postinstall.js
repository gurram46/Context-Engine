#!/usr/bin/env node

/**
 * Post-install script for Context Engine
 * Sets up Python dependencies automatically after npm install
 */

const chalk = require('chalk');
const PythonSetup = require('./setup-python');

class PostInstall {
  constructor() {
    this.pythonSetup = new PythonSetup();
  }

  async run() {
    console.log(chalk.cyan('\n🚀 Context Engine - Post Installation Setup\n'));

    try {
      // Skip Python setup in CI/CD environments
      if (process.env.CI || process.env.CONTINUOUS_INTEGRATION) {
        console.log(chalk.yellow('⚠️  Skipping Python setup in CI environment'));
        console.log(chalk.gray('   Run "npm run setup-python" manually to set up Python dependencies'));
        return;
      }

      // Run Python setup
      await this.pythonSetup.run();

      console.log(chalk.green('\n🎉 Context Engine is ready to use!'));
      console.log(chalk.cyan('\nTry running:'));
      console.log(chalk.white('  context-engine          # Start interactive mode'));
      console.log(chalk.white('  context-engine --help   # Show available commands'));
      console.log(chalk.white('  context-engine init     # Initialize a project'));

    } catch (error) {
      console.log(chalk.yellow('\n⚠️  Python setup failed, but Node.js installation is complete'));
      console.log(chalk.gray('   You can manually set up Python later by running:'));
      console.log(chalk.white('      npm run setup-python'));
      console.log(chalk.gray('\n   Or use Context Engine without Python features:'));
      console.log(chalk.white('      context-engine help'));

      // Don't exit with error code to allow npm install to succeed
      process.exit(0);
    }
  }
}

// Run postinstall if called directly
if (require.main === module) {
  const postInstall = new PostInstall();
  postInstall.run().catch(error => {
    console.error(chalk.red('Post-install error:'), error.message);
    process.exit(0); // Don't fail npm install
  });
}

module.exports = PostInstall;