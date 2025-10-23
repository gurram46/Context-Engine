#!/usr/bin/env node

/**
 * Placeholder build script for the Context Engine CLI.
 * Ensures the dist directory exists so npm hooks do not fail.
 */

const fs = require('fs');
const path = require('path');
const buildComponents = require('./build-components');

function ensureDistDirectory() {
  const projectRoot = path.join(__dirname, '..');
  const distDir = path.join(projectRoot, 'dist');

  if (!fs.existsSync(distDir)) {
    fs.mkdirSync(distDir, { recursive: true });
    console.log('Created dist/ directory for packaging artifacts.');
  } else {
    console.log('dist/ directory already present.');
  }
}

async function main() {
  ensureDistDirectory();
  await buildComponents();
  console.log('Context Engine CLI build step completed.');
}

if (require.main === module) {
  main().catch((error) => {
    console.error('Build failed:', error);
    process.exit(1);
  });
}
