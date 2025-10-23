#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const esbuild = require('esbuild');

async function buildComponents() {
  const projectRoot = path.join(__dirname, '..');
  const entryFile = path.join(projectRoot, 'components', 'ChatApp.tsx');
  const outDir = path.join(projectRoot, 'dist', 'components');

  if (!fs.existsSync(entryFile)) {
    console.warn('Chat UI source not found, skipping build.');
    return;
  }

  fs.mkdirSync(outDir, { recursive: true });

  await esbuild.build({
    entryPoints: [entryFile],
    bundle: true,
    platform: 'node',
    format: 'esm',
    target: ['node18'],
    outfile: path.join(outDir, 'chat-app.mjs'),
    sourcemap: false,
    external: ['ink', 'ink-select-input', 'ink-text-input', 'react'],
    banner: {
      js: "import {createRequire as __createRequire} from 'module'; const require = __createRequire(import.meta.url);"
    }
  });

  console.log('Built chat interface bundle at dist/components/chat-app.mjs');
}

module.exports = buildComponents;

if (require.main === module) {
  buildComponents().catch((error) => {
    console.error('Failed to build chat components:', error);
    process.exit(1);
  });
}
