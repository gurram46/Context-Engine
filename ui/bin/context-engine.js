#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');

// Get the directory of this script
const scriptDir = path.dirname(__filename);
const mainScript = path.join(scriptDir, '..', 'index.js');

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