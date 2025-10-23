#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const targetDir = path.resolve(__dirname, '..', 'backend');

fs.rmSync(targetDir, { recursive: true, force: true });
console.log('[context-engine-dev] Cleaned bundled backend after publish.');