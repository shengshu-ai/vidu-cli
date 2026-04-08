#!/usr/bin/env node
const fs = require('fs');
const https = require('https');
const path = require('path');

const pkg = require('./package.json');
const version = pkg.version;

const PLATFORM_MAP = {
  'darwin-arm64': 'vidu-cli-darwin-arm64',
  'darwin-x64': 'vidu-cli-darwin-x64',
  'linux-x64': 'vidu-cli-linux-x64',
  'linux-arm64': 'vidu-cli-linux-arm64',
  'win32-x64': 'vidu-cli-win32-x64.exe',
};

const platform = process.platform;
const arch = process.arch;
const key = `${platform}-${arch}`;
const binaryName = PLATFORM_MAP[key];

if (!binaryName) {
  console.error(`Unsupported platform: ${key}`);
  process.exit(1);
}

const binDir = path.join(__dirname, 'bin');
const ext = platform === 'win32' ? '.exe' : '';
const dest = path.join(binDir, `vidu-cli${ext}`);

if (!fs.existsSync(binDir)) fs.mkdirSync(binDir, { recursive: true });

const url = `https://github.com/shengshu-ai/vidu-cli/releases/download/v${version}/${binaryName}`;

console.log(`Downloading vidu-cli for ${key}...`);

function download(url, dest, cb) {
  https.get(url, (res) => {
    if (res.statusCode === 301 || res.statusCode === 302) {
      res.resume();
      return download(res.headers.location, dest, cb);
    }
    if (res.statusCode !== 200) {
      res.resume();
      return cb(new Error(`Download failed: HTTP ${res.statusCode}`));
    }
    const file = fs.createWriteStream(dest);
    file.on('error', (err) => { file.destroy(); cb(err); });
    res.pipe(file);
    file.on('finish', () => file.close(cb));
  }).on('error', cb);
}

download(url, dest, (err) => {
  if (err) {
    fs.unlink(dest, () => {});
    console.error(`Failed to download vidu-cli: ${err.message}`);
    process.exit(1);
  }
  if (platform !== 'win32') {
    fs.chmodSync(dest, 0o755);
  }
  console.log('vidu-cli installed successfully.');
});
