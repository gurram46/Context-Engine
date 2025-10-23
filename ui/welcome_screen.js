#!/usr/bin/env node

const chalk = require('chalk');
const boxenModule = require('boxen');
const boxen = typeof boxenModule === 'function' ? boxenModule : boxenModule.default;
const figlet = require('figlet');
const fs = require('fs');
const os = require('os');
const path = require('path');
const inquirerModule = require('inquirer');

const inquirer = inquirerModule.default || inquirerModule;
const prompt =
  typeof inquirer.createPromptModule === 'function'
    ? inquirer.createPromptModule()
    : (...args) => inquirer.prompt(...args);

const COLORS = {
  red: '#ff3131',
  orange: '#ff6b35',
  indigo: '#1800ad',
  dim: '#cc2a2a'
};

const STAR = '✱';
const BULLET = '•';
const RECENT_PATH_FILE = path.join(os.homedir(), '.context_engine_recent.json');

const ANSI_REGEX =
  // eslint-disable-next-line no-control-regex
  /[\u001B\u009B][[\]()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]/g;

const stripAnsi = (value) => value.replace(ANSI_REGEX, '');
const visibleLength = (value) => stripAnsi(value).length;

function padCenter(value, width) {
  const len = visibleLength(value);
  if (len >= width) return value;
  const total = width - len;
  const left = Math.floor(total / 2);
  const right = total - left;
  return `${' '.repeat(left)}${value}${' '.repeat(right)}`;
}

function createSeparator(label = '────────────') {
  const Separator = inquirer.Separator;
  if (typeof Separator === 'function') {
    try {
      return new Separator(label);
    } catch {
      return Separator(label);
    }
  }
  return { type: 'separator', line: label };
}

class WelcomeScreen {
  constructor() {
    this.version = '1.2.0';
    this.greetings = [
      'Welcome back!',
      'Hello there!',
      'Ready to compress the chaos?',
      'Great to see you!',
      'Let’s build some context!'
    ];
    this.maxRecent = 5;
  }

  getGreeting() {
    const index = Math.floor(Math.random() * this.greetings.length);
    return chalk.hex(COLORS.red).bold(this.greetings[index]);
  }

  createLogo() {
    try {
      const art = figlet.textSync('CNTXT ENGINE', { font: 'Small' }).split('\n');
      return art.map((line, rowIndex) => {
        if (!line.trim()) return '';
        if (rowIndex === 0) {
          return chalk.hex(COLORS.red)(
            line.replace('C', `C${chalk.hex(COLORS.indigo)(STAR)}`)
          );
        }
        return chalk.hex(COLORS.red)(line);
      });
    } catch {
      return [chalk.hex(COLORS.red)(`C${chalk.hex(COLORS.indigo)(STAR)}NTXT ENGINE`)];
    }
  }

  async getRecentPaths() {
    try {
      const raw = await fs.promises.readFile(RECENT_PATH_FILE, 'utf8');
      const data = JSON.parse(raw);
      const paths = Array.isArray(data.paths) ? data.paths : [];
      return paths.slice(0, this.maxRecent);
    } catch {
      return [];
    }
  }

  async recordCurrentPath() {
    try {
      const cwd = process.cwd();
      let paths = [];

      if (fs.existsSync(RECENT_PATH_FILE)) {
        const raw = await fs.promises.readFile(RECENT_PATH_FILE, 'utf8');
        const data = JSON.parse(raw);
        paths = Array.isArray(data.paths) ? data.paths : [];
      }

      paths = [cwd, ...paths.filter((entry) => entry !== cwd)].slice(0, this.maxRecent);
      await fs.promises.writeFile(
        RECENT_PATH_FILE,
        JSON.stringify({ paths }, null, 2),
        'utf8'
      );
    } catch {
      // ignore persistence issues
    }
  }

  buildCard(recentPaths) {
    const logoLines = this.createLogo();
    const tips = [
      `${BULLET} Use /help to list commands`,
      `${BULLET} Try /summary for a quick overview`,
      `${BULLET} Run /bundle after updating baseline`
    ];

    const recent = recentPaths.length
      ? recentPaths.map((entry) => `${BULLET} ${entry}`)
      : [`${BULLET} No recent projects recorded`];

    const content = [
      this.getGreeting(),
      '',
      ...logoLines,
      '',
      chalk.hex(COLORS.orange)(`Version ${this.version}`),
      '',
      chalk.hex(COLORS.red).bold('Tips for getting started'),
      ...tips,
      '',
      chalk.hex(COLORS.red).bold('Recent paths'),
      ...recent,
      '',
      chalk.hex(COLORS.orange).bold('Current project'),
      process.cwd()
    ];

    const width = Math.min(
      96,
      Math.max(...content.map((line) => visibleLength(line))) + 4
    );

    return {
      width,
      content: content.map((line) => padCenter(line, width - 4))
    };
  }

  async showWelcomeScreen() {
    const { width, content } = this.buildCard(await this.getRecentPaths());
    const boxed = boxen(content.join('\n'), {
      padding: { top: 1, bottom: 1, left: 2, right: 2 },
      borderColor: COLORS.red,
      borderStyle: 'round'
    });

    console.clear();
    console.log('\n' + boxed);
    console.log(
      chalk.hex(COLORS.dim)(
        padCenter('Context Engine — Compress the Chaos.', width)
      )
    );
    console.log('');

    await this.recordCurrentPath();
  }

  async showCommandPalette() {
    const choices = [
      { name: 'Initialize project (/init)', value: 'init' },
      { name: 'Start session (/start-session)', value: 'start-session' },
      { name: 'Stop session (/stop-session)', value: 'stop-session' },
      { name: 'Save session & generate summary (/session save)', value: 'session-save' },
      { name: 'Generate AI summary (/summary)', value: 'summary' },
      { name: 'Compress context (/compress)', value: 'compress' },
      { name: 'Create bundle (/bundle)', value: 'bundle' },
      { name: 'Show configuration (/config show)', value: 'config show' },
      createSeparator(),
      { name: 'Help', value: 'help' },
      { name: 'Exit', value: 'exit' }
    ];

    const { command } = await prompt([
      {
        type: 'list',
        name: 'command',
        message: 'Select a command',
        choices
      }
    ]);

    if (command === 'session-save') {
      const { note } = await prompt([
        {
          type: 'input',
          name: 'note',
          message: 'Session note:',
          validate: (value) =>
            value && value.trim().length > 0 ? true : 'Please enter a session note.'
        }
      ]);
      return `session save ${note.trim()}`;
    }

    return command;
  }
}

module.exports = WelcomeScreen;
