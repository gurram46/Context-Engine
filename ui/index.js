#!/usr/bin/env node

const { Command } = require('commander');
const chalk = require('chalk');
const inquirer = require('inquirer');
const WelcomeScreen = require('./lib/welcome');
const BackendBridge = require('./lib/backend-bridge');

class ContextEngineCLI {
  constructor() {
    this.program = new Command();
    this.welcomeScreen = new WelcomeScreen();
    this.backendBridge = new BackendBridge();
    this.setupCommands();
  }

  setupCommands() {
    this.program
      .name('context-engine')
      .description('Context Engine CLI - Compress the Chaos')
      .version('1.0.0');

    // Interactive mode (default)
    this.program
      .command('welcome', { isDefault: true })
      .description('Show welcome screen and interactive command palette')
      .action(async () => {
        await this.showWelcomeAndCommandPalette();
      });

    // Direct commands
    this.program
      .command('init')
      .description('Initialize Context Engine in current directory')
      .action(async () => {
        try {
          await this.backendBridge.init();
          console.log(chalk.green('✓ Project initialized successfully'));
        } catch (error) {
          console.error(chalk.red('✗ Initialization failed:'), error.message);
          process.exit(1);
        }
      });

    this.program
      .command('baseline')
      .description('Generate project baseline')
      .option('-o, --output <file>', 'Output file', 'BASELINE.md')
      .action(async (options) => {
        try {
          await this.backendBridge.baseline(['--output', options.output]);
          console.log(chalk.green(`✓ Baseline generated: ${options.output}`));
        } catch (error) {
          console.error(chalk.red('✗ Baseline generation failed:'), error.message);
          process.exit(1);
        }
      });

    this.program
      .command('bundle')
      .description('Create context bundle')
      .option('-o, --output <file>', 'Output file', 'bundle.md')
      .option('-f, --format <format>', 'Output format', 'markdown')
      .action(async (options) => {
        try {
          await this.backendBridge.bundle(['--output', options.output, '--format', options.format]);
          console.log(chalk.green(`✓ Bundle created: ${options.output}`));
        } catch (error) {
          console.error(chalk.red('✗ Bundle creation failed:'), error.message);
          process.exit(1);
        }
      });

    this.program
      .command('compress')
      .description('Compress source files')
      .option('-o, --output <file>', 'Output file', 'compressed.md')
      .option('-r, --recursive', 'Recursive compression')
      .action(async (options) => {
        try {
          const args = ['--output', options.output];
          if (options.recursive) args.push('--recursive');
          await this.backendBridge.compress(args);
          console.log(chalk.green(`✓ Compression complete: ${options.output}`));
        } catch (error) {
          console.error(chalk.red('✗ Compression failed:'), error.message);
          process.exit(1);
        }
      });

    // Interactive command palette
    this.program
      .command('palette')
      .alias('p')
      .description('Show interactive command palette')
      .action(async () => {
        await this.showCommandPalette();
      });

    // Help command
    this.program
      .command('help')
      .description('Show help information')
      .action(() => {
        this.showHelp();
      });
  }

  async showWelcomeAndCommandPalette() {
    await this.welcomeScreen.showWelcomeScreen();

    // Show command palette after welcome screen
    console.log(chalk.cyan('\nPress Enter to open command palette or Ctrl+C to exit...'));

    // Wait for user input
    process.stdin.setRawMode(true);
    process.stdin.resume();
    process.stdin.setEncoding('utf8');

    await new Promise((resolve) => {
      process.stdin.on('data', async (key) => {
        if (key === '\r' || key === '\n') {
          process.stdin.setRawMode(false);
          process.stdin.pause();
          await this.showCommandPalette();
          resolve();
        } else if (key === '\u0003') { // Ctrl+C
          console.log(chalk.yellow('\nGoodbye!'));
          process.exit(0);
        }
      });
    });
  }

  async showCommandPalette() {
    try {
      const command = await this.welcomeScreen.showCommandPalette();

      if (command === 'exit') {
        console.log(chalk.yellow('Goodbye!'));
        process.exit(0);
      } else if (command === 'help') {
        this.showHelp();
        await this.showCommandPalette(); // Show palette again after help
      } else {
        // Execute the selected command
        await this.executeCommand(command);
      }
    } catch (error) {
      console.error(chalk.red('Error executing command:'), error.message);
      process.exit(1);
    }
  }

  async executeCommand(command) {
    const chalk = require('chalk');
    const ora = require('ora');

    const spinner = ora(`Executing ${command}...`).start();

    try {
      switch (command) {
        case 'init':
          await this.backendBridge.init();
          spinner.succeed(chalk.green('Project initialized successfully'));
          break;
        case 'baseline':
          await this.backendBridge.baseline();
          spinner.succeed(chalk.green('Baseline generated successfully'));
          break;
        case 'bundle':
          await this.backendBridge.bundle();
          spinner.succeed(chalk.green('Bundle created successfully'));
          break;
        case 'compress':
          await this.backendBridge.compress();
          spinner.succeed(chalk.green('Compression completed successfully'));
          break;
        default:
          spinner.fail(chalk.red(`Unknown command: ${command}`));
          return;
      }

      // Show command palette again after successful execution
      console.log(chalk.cyan('\nPress Enter to continue or Ctrl+C to exit...'));
      await this.waitForUserInput();
      await this.showCommandPalette();

    } catch (error) {
      spinner.fail(chalk.red(`Command failed: ${error.message}`));
      process.exit(1);
    }
  }

  async waitForUserInput() {
    return new Promise((resolve) => {
      process.stdin.setRawMode(true);
      process.stdin.resume();
      process.stdin.setEncoding('utf8');

      process.stdin.on('data', (key) => {
        if (key === '\r' || key === '\n') {
          process.stdin.setRawMode(false);
          process.stdin.pause();
          resolve();
        } else if (key === '\u0003') { // Ctrl+C
          console.log(chalk.yellow('\nGoodbye!'));
          process.exit(0);
        }
      });
    });
  }

  showHelp() {
    console.log(chalk.bold.cyan('\nContext Engine CLI Help\n'));
    console.log(chalk.bold('Commands:'));
    console.log(chalk.cyan('  init') + '       Initialize Context Engine in current directory');
    console.log(chalk.cyan('  baseline') + '  Generate project baseline');
    console.log(chalk.cyan('  bundle') + '    Create context bundle');
    console.log(chalk.cyan('  compress') + '  Compress source files');
    console.log(chalk.cyan('  palette') + '    Show interactive command palette');
    console.log(chalk.cyan('  help') + '       Show this help message');
    console.log(chalk.cyan('  welcome') + '   Show welcome screen and command palette');

    console.log(chalk.bold('\nInteractive Mode:'));
    console.log('Run ' + chalk.cyan('context-engine') + ' without arguments to start interactive mode');

    console.log(chalk.bold('\nExamples:'));
    console.log('  ' + chalk.cyan('context-engine init') + '                    # Initialize project');
    console.log('  ' + chalk.cyan('context-engine bundle -o docs.md') + '       # Create bundle with custom output');
    console.log('  ' + chalk.cyan('context-engine compress --recursive') + '    # Compress recursively');

    console.log(chalk.bold('\nQuick Tips:'));
    console.log('• Use ' + chalk.cyan('/init') + ', ' + chalk.cyan('/bundle') + ', ' + chalk.cyan('/compress') + ' shortcuts in interactive mode');
    console.log('• Press Ctrl+C anytime to exit');
    console.log('• Check generated files in your project directory');
    console.log(chalk.gray('\nContext Engine — Compress the Chaos.'));
  }

  async run() {
    try {
      await this.program.parseAsync(process.argv);
    } catch (error) {
      console.error(chalk.red('CLI Error:'), error.message);
      process.exit(1);
    }
  }
}

// Run the CLI
if (require.main === module) {
  const cli = new ContextEngineCLI();
  cli.run().catch(error => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = ContextEngineCLI;