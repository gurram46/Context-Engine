#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const sourceDir = path.resolve(__dirname, '..', '..', 'backend');
const targetDir = path.resolve(__dirname, '..', 'backend');

const ignoredNames = new Set([
  '__pycache__',
  '.pytest_cache',
  '.context',
  'dist',
  'build',
  'node_modules'
]);

if (!fs.existsSync(sourceDir)) {
  console.error('[context-engine-dev] Unable to bundle backend: source directory not found', sourceDir);
  process.exit(1);
}

fs.rmSync(targetDir, { recursive: true, force: true });

const shouldInclude = (srcPath) => {
  const base = path.basename(srcPath);
  if (ignoredNames.has(base)) {
    return false;
  }
  if (base.endsWith('.egg-info')) {
    return false;
  }
  const ext = path.extname(base);
  if (ext === '.pyc' || ext === '.pyo' || ext === '.whl') {
    return false;
  }
  return true;
};

fs.cpSync(sourceDir, targetDir, {
  recursive: true,
  filter: (src) => shouldInclude(src)
});

console.log('[context-engine-dev] Bundled Python backend for npm package.');