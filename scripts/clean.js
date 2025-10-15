#!/usr/bin/env node

/**
 * Clean script for Context Engine
 * Removes build artifacts and cache files
 */

const fs = require('fs-extra');
const path = require('path');
const chalk = require('chalk');

class Cleaner {
  constructor() {
    this.rootDir = path.resolve(__dirname, '..');
  }

  async run() {
    console.log(chalk.cyan('🧹 Cleaning Context Engine...'));

    try {
      const cleanedDirs = [];

      // Clean build directories
      const buildDirs = [
        'lib',
        'bin',
        'dist',
        'build'
      ];

      for (const dir of buildDirs) {
        const dirPath = path.join(this.rootDir, dir);
        if (await fs.pathExists(dirPath)) {
          await fs.remove(dirPath);
          cleanedDirs.push(dir);
        }
      }

      // Clean cache directories
      const cacheDirs = [
        '.cache',
        '.parcel-cache',
        '.eslintcache',
        '.nyc_output',
        'coverage',
        'node_modules/.cache'
      ];

      for (const dir of cacheDirs) {
        const dirPath = path.join(this.rootDir, dir);
        if (await fs.pathExists(dirPath)) {
          await fs.remove(dirPath);
          cleanedDirs.push(dir);
        }
      }

      // Clean Python virtual environments
      const venvDirs = [
        'python/.venv',
        'backend/venv',
        'venv',
        '.venv'
      ];

      for (const dir of venvDirs) {
        const dirPath = path.join(this.rootDir, dir);
        if (await fs.pathExists(dirPath)) {
          await fs.remove(dirPath);
          cleanedDirs.push(dir);
        }
      }

      // Clean Python cache files
      await this.cleanPythonCache();

      // Clean log files
      const logFiles = await this.findFiles(this.rootDir, ['.log', '*.tmp']);

      for (const file of logFiles) {
        await fs.remove(file);
        cleanedDirs.push(path.relative(this.rootDir, file));
      }

      console.log(chalk.green(`\n✅ Cleaned ${cleanedDirs.length} directories/files`));

      if (cleanedDirs.length > 0) {
        console.log(chalk.blue('Cleaned:'));
        cleanedDirs.forEach(dir => console.log(chalk.gray(`  - ${dir}`)));
      } else {
        console.log(chalk.blue('Nothing to clean'));
      }

    } catch (error) {
      console.error(chalk.red('❌ Cleaning failed:'), error.message);
      process.exit(1);
    }
  }

  async cleanPythonCache() {
    console.log(chalk.blue('🐍 Cleaning Python cache...'));

    const cachePatterns = [
      '**/__pycache__',
      '**/*.pyc',
      '**/*.pyo',
      '**/*.pyd',
      '**/.pytest_cache',
      '**/.mypy_cache'
    ];

    for (const pattern of cachePatterns) {
      const files = await this.findFiles(this.rootDir, [pattern]);
      for (const file of files) {
        await fs.remove(file);
      }
    }
  }

  async findFiles(dir, patterns) {
    const files = [];

    for (const pattern of patterns) {
      const glob = require('glob');
      const matches = glob.sync(pattern, {
        cwd: dir,
        absolute: true
      });
      files.push(...matches);
    }

    return files;
  }
}

if (require.main === module) {
  const cleaner = new Cleaner();
  cleaner.run().catch(error => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = Cleaner;