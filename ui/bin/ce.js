#!/usr/bin/env node

/**
 * Short alias for the Context Engine CLI.
 * Mirrors the behaviour of the primary context-engine binary.
 */

const { spawn } = require('child_process');
const path = require('path');

const scriptDir = path.dirname(__filename);
const mainScript = path.join(scriptDir, '..', 'index.js');

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
