// Runs during `npm version`: copies the new version from package.json into
// the Rust crate, so Cargo.toml and Cargo.lock match the app version.

import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

const { version } = JSON.parse(readFileSync('package.json', 'utf8'));

function setVersion(file, pattern, replacement) {
  const text = readFileSync(file, 'utf8');
  if (!pattern.test(text)) throw new Error(`No version found in ${file}`);
  writeFileSync(file, text.replace(pattern, replacement));
}

setVersion('src-tauri/Cargo.toml', /^version = ".*"$/m, `version = "${version}"`);
setVersion('src-tauri/Cargo.lock', /(name = "sweep"\nversion = )".*"/, `$1"${version}"`);

// npm only commits files that are staged.
execFileSync('git', ['add', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']);
console.log(`Version set to ${version}`);
