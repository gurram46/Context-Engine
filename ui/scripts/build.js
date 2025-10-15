#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

// Create lib directory structure
const libDir = path.join(__dirname, '..', 'lib');
if (!fs.existsSync(libDir)) {
  fs.mkdirSync(libDir, { recursive: true });
}

// Copy source files to lib directory for distribution
const sourceFiles = [
  'index.js',
  'lib/welcome.js',
  'lib/backend-bridge.js'
];

const sourceDir = path.join(__dirname, '..');
const targetDir = path.join(__dirname, '..', 'lib');

for (const file of sourceFiles) {
  const sourcePath = path.join(sourceDir, file);
  const targetPath = path.join(targetDir, file);

  // Create directory if it doesn't exist
  const targetFileDir = path.dirname(targetPath);
  if (!fs.existsSync(targetFileDir)) {
    fs.mkdirSync(targetFileDir, { recursive: true });
  }

  if (fs.existsSync(sourcePath)) {
    fs.copyFileSync(sourcePath, targetPath);
    console.log(`✓ Copied ${file}`);
  } else {
    console.warn(`⚠ Warning: ${file} not found`);
  }
}

console.log('✓ Build completed successfully');