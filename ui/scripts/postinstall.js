#!/usr/bin/env node

/**
 * Post-install script for Context Engine.
 * Installs Python backend dependencies using python -m pip.
 */

const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

console.log('dY" Setting up Context Engine backend...');

const projectRoot = path.join(__dirname, '..');
const backendDir = path.join(projectRoot, 'backend');

function findPython() {
  const candidates = ['python', 'py', 'python3'];

  return new Promise((resolve) => {
    const tryNext = (index) => {
      if (index >= candidates.length) {
        resolve(null);
        return;
      }

      const cmd = candidates[index];
      const child = spawn(cmd, ['--version']);
      let output = '';

      child.stdout.on('data', (data) => {
        output += data.toString();
      });
      child.stderr.on('data', (data) => {
        output += data.toString();
      });

      child.on('close', (code) => {
        if (code === 0) {
          console.log(`�o. Python detected via "${cmd}": ${output.trim()}`);
          resolve(cmd);
        } else {
          tryNext(index + 1);
        }
      });

      child.on('error', () => {
        tryNext(index + 1);
      });
    };

    tryNext(0);
  });
}

async function ensureRequirementsFile() {
  const requirementsPath = path.join(backendDir, 'requirements.txt');
  if (fs.existsSync(requirementsPath)) {
    return requirementsPath;
  }

  console.log('�s��,? requirements.txt not found. Creating minimal requirements list...');
  const minimalRequirements = `openrouter>=0.2.0
rich>=13.0.0
click>=8.0.0
pyyaml>=6.0
requests>=2.28.0
`;
  fs.writeFileSync(requirementsPath, minimalRequirements);
  return requirementsPath;
}

async function installBackendDependencies() {
  if (!fs.existsSync(backendDir)) {
    console.log('�s��,? Backend directory not found. Skipping Python setup.');
    return;
  }

  const requirementsPath = await ensureRequirementsFile();
  const pythonCmd = await findPython();

  if (!pythonCmd) {
    console.log('�?O Python 3.8+ not detected. Skipping backend dependency installation.');
    console.log('dY\'� Install manually with: python -m pip install -r backend/requirements.txt');
    return;
  }

  console.log('dY"� Installing Python backend dependencies...');

  await new Promise((resolve) => {
    const pip = spawn(pythonCmd, ['-m', 'pip', 'install', '-r', requirementsPath], {
      cwd: backendDir,
      stdio: 'inherit'
    });

    pip.on('close', (code) => {
      if (code === 0) {
        console.log('�o. Backend dependencies installed successfully');
      } else {
        console.log('�s��,? Backend dependency installation failed, but CLI will still work');
      }
      resolve();
    });

    pip.on('error', (error) => {
      console.log('�s��,? Could not install backend dependencies:', error.message);
      console.log('dY\'� Install manually with: python -m pip install -r backend/requirements.txt');
      resolve();
    });
  });
}

async function main() {
  try {
    await installBackendDependencies();
    console.log('dYZ% Context Engine setup complete!');
    console.log('');
    console.log('dY"- Usage:');
    console.log('  context-engine         # Launch interactive CLI');
    console.log('  context-engine help    # Show all commands');
    console.log('');
  } catch (error) {
    console.log('�s��,? Post-install setup completed with warnings');
    console.log(error.message);
  }
}

if (require.main === module) {
  main();
}
