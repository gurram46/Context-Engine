#!/usr/bin/env node

/**
 * Build script for Context Engine unified distribution
 * Prepares the package for npm distribution with bundled Python backend
 */

const fs = require('fs-extra');
const path = require('path');
const chalk = require('chalk');

class Builder {
  constructor() {
    this.rootDir = path.resolve(__dirname, '..');
    this.libDir = path.join(this.rootDir, 'lib');
    this.binDir = path.join(this.rootDir, 'bin');
    this.pythonDir = path.join(this.rootDir, 'python');
    this.scriptsDir = path.join(this.rootDir, 'scripts');
  }

  async run() {
    console.log(chalk.cyan('🏗️  Building Context Engine for distribution...'));

    try {
      await this.clean();
      await this.copyNodejsFiles();
      await this.copyPythonFiles();
      await this.copyScripts();
      await this.updateBinaries();
      await this.createDistributionFiles();

      console.log(chalk.green('✅ Build completed successfully!'));
      console.log(chalk.cyan('📦 Package is ready for npm distribution'));

    } catch (error) {
      console.error(chalk.red('❌ Build failed:'), error.message);
      process.exit(1);
    }
  }

  async clean() {
    console.log(chalk.blue('🧹 Cleaning build directories...'));

    const dirsToClean = [this.libDir, this.binDir, this.pythonDir];

    for (const dir of dirsToClean) {
      if (await fs.pathExists(dir)) {
        await fs.remove(dir);
      }
      await fs.ensureDir(dir);
    }
  }

  async copyNodejsFiles() {
    console.log(chalk.blue('📋 Copying Node.js files...'));

    // Copy main index.js
    const indexPath = path.join(this.rootDir, 'ui', 'index.js');
    await fs.copy(indexPath, path.join(this.libDir, 'index.js'));

    // Copy lib files
    const uiLibDir = path.join(this.rootDir, 'ui', 'lib');
    if (await fs.pathExists(uiLibDir)) {
      await fs.copy(uiLibDir, this.libDir);
    }

    // Update require paths in copied files
    await this.updateRequirePaths();
  }

  async copyPythonFiles() {
    console.log(chalk.blue('🐍 Copying Python files...'));

    const backendDir = path.join(this.rootDir, 'backend');
    if (await fs.pathExists(backendDir)) {
      // Copy Python modules
      await fs.copy(
        path.join(backendDir, 'context_engine'),
        path.join(this.pythonDir, 'context_engine')
      );

      // Copy main files
      const filesToCopy = ['main.py', 'welcome_screen.py'];
      for (const file of filesToCopy) {
        const srcPath = path.join(backendDir, file);
        if (await fs.pathExists(srcPath)) {
          await fs.copy(srcPath, path.join(this.pythonDir, file));
        }
      }
    }
  }

  async copyScripts() {
    console.log(chalk.blue('📜 Copying build scripts...'));

    // Copy necessary scripts for distribution
    const scriptsToCopy = [
      'setup-python.js',
      'postinstall.js'
    ];

    for (const script of scriptsToCopy) {
      const srcPath = path.join(this.scriptsDir, script);
      if (await fs.pathExists(srcPath)) {
        await fs.copy(srcPath, path.join(this.scriptsDir, script));
      }
    }
  }

  async updateBinaries() {
    console.log(chalk.blue('🔧 Updating CLI binaries...'));

    // Update context-engine binary
    const contextEngineBinary = `#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');

// Get the directory of this script
const scriptDir = path.dirname(__filename);
const mainScript = path.join(scriptDir, '..', 'lib', 'index.js');

// Spawn the main process
const child = spawn('node', [mainScript, ...process.argv.slice(2)], {
  stdio: 'inherit',
  cwd: process.cwd(),
  env: process.env
});

child.on('exit', (code) => {
  process.exit(code);
});

child.on('error', (err) => {
  console.error('Failed to start subprocess:', err);
  process.exit(1);
});
`;

    // Update ce binary (short command)
    const ceBinary = contextEngineBinary; // Same content

    await fs.writeFile(path.join(this.binDir, 'context-engine.js'), contextEngineBinary);
    await fs.writeFile(path.join(this.binDir, 'ce.js'), ceBinary);
  }

  async updateRequirePaths() {
    console.log(chalk.blue('🔄 Updating require paths...'));

    // Update backend-bridge.js require path for setup-python
    const backendBridgePath = path.join(this.libDir, 'backend-bridge.js');
    if (await fs.pathExists(backendBridgePath)) {
      let content = await fs.readFile(backendBridgePath, 'utf8');

      // Update require path for setup-python
      content = content.replace(
        "require('../scripts/setup-python')",
        "require('../../scripts/setup-python')"
      );

      await fs.writeFile(backendBridgePath, content);
    }
  }

  async createDistributionFiles() {
    console.log(chalk.blue('📄 Creating distribution files...'));

    // Create .npmignore
    const npmIgnore = `
# Development files
ui/
backend/
.git/
.github/
.gitignore
.vscode/
.idea/
*.log
*.tmp
.DS_Store
Thumbs.db

# Test files
tests/
coverage/
.nyc_output/

# Development dependencies
node_modules/
venv/
.venv/

# Build artifacts
dist/
build/

# Documentation (keep main README)
PROJECT_CODE_DOCUMENTATION.md
PROJECT_DOCUMENTATION.txt
docs/

# Cache
.cache/
.parcel-cache/
.eslintcache
`;

    await fs.writeFile(path.join(this.rootDir, '.npmignore'), npmIgnore.trim());

    // Create a simple package.json for distribution (without devDependencies)
    const packageJson = await fs.readJson(path.join(this.rootDir, 'package.json'));
    const distPackageJson = {
      ...packageJson,
      scripts: {
        start: packageJson.scripts.start,
        postinstall: packageJson.scripts.postinstall,
        'setup-python': packageJson.scripts['setup-python']
      },
      devDependencies: undefined
    };

    await fs.writeJson(
      path.join(this.rootDir, 'package.json'),
      distPackageJson,
      { spaces: 2 }
    );
  }
}

// Run build if called directly
if (require.main === module) {
  const builder = new Builder();
  builder.run().catch(error => {
    console.error(chalk.red('Fatal error:'), error);
    process.exit(1);
  });
}

module.exports = Builder;