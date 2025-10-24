#!/usr/bin/env node

const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

console.log('[context-engine-cli] Checking Python backend dependencies...');

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
          console.log(`[context-engine-cli] Python detected via "${cmd}": ${output.trim()}`);
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
    console.log('[context-engine-cli] Backend directory not found. Skipping Python setup.');
    return;
  }

  const requirementsPath = await ensureRequirementsFile();
  const pythonCmd = await findPython();

  if (!pythonCmd) {
    console.log('[context-engine-cli] Python 3.8+ not detected. Install dependencies manually:');
    console.log('  python -m pip install -r backend/requirements.txt');
    return;
  }

  console.log('[context-engine-cli] Installing Python backend dependencies...');

  await new Promise((resolve) => {
    const pip = spawn(pythonCmd, ['-m', 'pip', 'install', '-r', requirementsPath], {
      cwd: backendDir,
      stdio: 'inherit'
    });

    pip.on('close', (code) => {
      if (code === 0) {
        console.log('[context-engine-cli] Backend dependencies installed successfully.');
      } else {
        console.log('[context-engine-cli] pip install exited with a non-zero code. Install manually if needed.');
      }
      resolve();
    });

    pip.on('error', (error) => {
      console.log('[context-engine-cli] Could not install backend dependencies automatically:', error.message);
      console.log('Run manually: python -m pip install -r backend/requirements.txt');
      resolve();
    });
  });
}

async function main() {
  try {
    await installBackendDependencies();
    console.log('[context-engine-cli] Setup complete.');
    console.log('');
    console.log('Usage:');
    console.log('  context-engine         # Launch interactive CLI');
    console.log('  context-engine help    # Show all commands');
    console.log('');
  } catch (error) {
    console.log('[context-engine-cli] Post-install setup completed with warnings');
    console.log(error.message);
  }
}

if (require.main === module) {
  main();
}
