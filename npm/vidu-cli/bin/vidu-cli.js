#!/usr/bin/env node
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const ext = process.platform === 'win32' ? '.exe' : '';
const bin = path.join(__dirname, `vidu-cli${ext}`);

if (!fs.existsSync(bin)) {
  console.error(`vidu-cli binary not found. Try reinstalling: npm install -g vidu-cli`);
  process.exit(1);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  console.error(`Failed to run vidu-cli: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
