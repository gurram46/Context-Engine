#!/usr/bin/env node

const { Command } = require('commander');
const chalk = require('chalk');
const ora = require('ora');
const fs = require('fs');
const path = require('path');
const { pathToFileURL } = require('url');
const WelcomeScreen = require('./lib/welcome');
const BackendBridge = require('./lib/backend-bridge');
const BULLET = '-';

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
      .description('Context Engine v1.2 - AI-powered session intelligence for coding projects')
      .version('1.2.0');

    this.program
      .command('welcome', { isDefault: true })
      .description('Show welcome screen and interactive command palette')
      .action(async () => {
        await this.showWelcomeAndCommandPalette();
      });

    this.program
      .command('init')
      .description('Initialize Context Engine in current directory')
      .action(() => this.runSimpleCommand('init', 'Project initialized successfully'));

    this.program
      .command('start-session')
      .description('Start background logging of CLI and file activity')
      .option('--auto', 'Enable automatic logging of file and CLI actions')
      .action((options) => {
        const args = [];
        if (options.auto) args.push('--auto');
        return this.runSimpleCommand('start-session', 'Session tracking started', args);
      });

    this.program
      .command('stop-session')
      .description('Stop current session gracefully')
      .action(() => this.runSimpleCommand('stop-session', 'Session stopped'));

    this.program
      .command('session')
      .description('Session management commands')
      .argument('[action]', 'Session action (save)')
      .action((action) => {
        const args = [action || 'save'];
        return this.runSimpleCommand('session', 'Session command executed', args);
      });

    this.program
      .command('bundle')
      .description('Create context bundle')
      .option('-o, --output <file>', 'Output file', 'bundle.md')
      .option('-f, --format <format>', 'Output format', 'markdown')
      .action(({ output, format }) => this.runSimpleCommand('bundle', `Bundle created: ${output}`, ['--output', output, '--format', format]));

    this.program
      .command('compress')
      .description('Compress baseline files for AI context')
      .option('--rate <rate>', 'Compression rate (0.0 - 1.0)')
      .option('--query <query>', 'Compression guidance query')
      .option('--instruction <instruction>', 'Compression instruction prompt')
      .action((options) => {
        const args = [];
        if (options.rate) args.push('--rate', options.rate);
        if (options.query) args.push('--query', options.query);
        if (options.instruction) args.push('--instruction', options.instruction);
        return this.runSimpleCommand('compress', 'Compression completed', args);
      });

    this.program
      .command('summary')
      .description('Generate AI-powered project summary or display latest summary')
      .option('--project-root <path>', 'Project root directory')
      .action(({ projectRoot }) =>
        this.runCommandWithSpinner('summary', () => this.generateSummary(projectRoot))
      );

    this.program
      .command('config-show')
      .description('Display current configuration values')
      .action(() => this.runSimpleCommand('config show', 'Configuration displayed'));

    this.program
      .command('config-set <key> <value>')
      .description('Set configuration value (model, api_key)')
      .action((key, value) => this.runSimpleCommand('config set ' + [key, value].join(' '), 'Configuration updated'));

    this.program
      .command('palette')
      .description('Open the interactive command palette')
      .action(() => this.showCommandPalette());

    this.program
      .command('chat')
      .description('Launch the combined palette + chat interface')
      .action(() => this.launchInteractiveUI());

    this.program
      .command('help')
      .description('Show help information')
      .action(() => this.showHelp());
  }

  async showWelcomeAndCommandPalette() {
    await this.welcomeScreen.showWelcomeScreen();
    await this.launchInteractiveUI();
  }

  async showCommandPalette() {
    await this.launchInteractiveUI();
  }

  async launchInteractiveUI() {
    try {
      const React = require('react');
      const { render } = await import('ink');
      const compiledPath = path.join(__dirname, 'dist', 'components', 'chat-app.mjs');

      if (!fs.existsSync(compiledPath)) {
        console.error(chalk.red('Chat UI bundle missing. Run "npm run build" inside ui/ first.'));
        return;
      }

      const { default: ChatApp } = await import(pathToFileURL(compiledPath).href);
      const { waitUntilExit } = render(React.createElement(ChatApp));
      await waitUntilExit();
    } catch (error) {
      console.error(chalk.red('Interactive UI failed:'), error.message);
    }
  }

  async runSimpleCommand(command, successMessage, args = []) {
    return this.runCommandWithSpinner(command, async () => {
      await this.backendBridge.executeCommand(command, args);
      console.log(chalk.green(successMessage));
    });
  }

  async runCommandWithSpinner(label, executor) {
    const spinner = ora(`Executing ${label}...`).start();
    try {
      const result = await executor();
      spinner.succeed(chalk.hex('#ff3131')(`${label} completed`));
      return result;
    } catch (error) {
      spinner.fail(chalk.red(`Command failed: ${error.message}`));
      throw error;
    }
  }

  async generateSummary() {
    const result = await this.backendBridge.executeCommand('summary', []);
    const lines = result.stdout.split('\n');
    const payload = lines.find((line) => line.trim().startsWith('{'));
    if (!payload) {
      console.log(chalk.red('Summary command did not return JSON output.'));
      return null;
    }

    const parsed = JSON.parse(payload);
    if (!parsed.success) {
      throw new Error(parsed.error || 'Summary generation failed');
    }

    const red = chalk.hex('#ff3131');
    const orange = chalk.hex('#ff6b35');
    const green = chalk.hex('#4ade80');
    const rule = '-'.repeat(60);

    console.log(red.bold('\n*** PROJECT SUMMARY GENERATED\n'));
    console.log(chalk.gray(rule));

    const summaryLines = parsed.summary.split('\n');
    let displayed = 0;
    for (const line of summaryLines) {
      if (displayed >= 30) {
        console.log(chalk.gray('...'));
        break;
      }

      const trimmed = line.trimStart();
      if (line.startsWith('# ')) {
        console.log(red.bold(line));
      } else if (line.startsWith('## ')) {
        console.log(orange.bold(line));
      } else if (line.startsWith('- ') || trimmed.startsWith(BULLET) || trimmed.startsWith('-')) {
        console.log(green(line));
      } else {
        console.log(line);
      }

      displayed += 1;
    }

    console.log(chalk.gray(rule));
    console.log(red(`Summary saved to: ${parsed.summary_path}`));
    console.log(red(`Model used: ${parsed.model_used}`));
    return parsed;
  }

  showHelp() {
    console.log(chalk.bold.cyan('\nContext Engine v1.2 CLI Help - Session Intelligence Model\n'));
    console.log(chalk.bold('Core Commands (8 total):'));
    console.log(`${chalk.cyan('init')}              Initialize Context Engine in current directory`);
    console.log(`${chalk.cyan('start-session')}     Start background logging of CLI and file activity`);
    console.log(`${chalk.cyan('stop-session')}      Stop current session gracefully`);
    console.log(`${chalk.cyan('session save')}      Save session note and generate AI-powered summary`);
    console.log(`${chalk.cyan('summary')}           Generate AI-powered project summary or display latest`);
    console.log(`${chalk.cyan('compress')}          Compress project context using LongCodeZip`);
    console.log(`${chalk.cyan('bundle')}            Create context bundle for AI handoff`);
    console.log(`${chalk.cyan('config show')}       Display current configuration values`);
    console.log(`${chalk.cyan('chat')}              Launch palette + chat interface`);
    console.log(`${chalk.cyan('palette')}           Interactive command palette`);
    console.log(`${chalk.cyan('help')}              Show this help message`);

    console.log(chalk.bold('\nSession Intelligence Flow:'));
    console.log(`1. ${chalk.cyan('init')} -> Initialize project`);
    console.log(`2. ${chalk.cyan('start-session')} -> Begin logging activity`);
    console.log(`3. ${chalk.cyan('session save')} -> Generate AI summary`);
    console.log(`4. ${chalk.cyan('compress')} + ${chalk.cyan('bundle')} -> Package for AI`);

    console.log(chalk.bold('\nInteractive Tips:'));
    console.log(`- Use ${chalk.cyan('/init')}, ${chalk.cyan('/start-session')}, ${chalk.cyan('/summary')} shortcuts`);
    console.log('- Press Enter for the palette, /exit to quit');
    console.log('- Session notes: /session save "Fixed authentication bug in API"');
    console.log(`- Run ${chalk.cyan('context-engine chat')} for the chat-enabled palette`);
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

if (require.main === module) {
  const cli = new ContextEngineCLI();
  cli.run().catch((error) => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = ContextEngineCLI;
