#!/usr/bin/env node
// Writes the license texts of every third-party component compiled into the
// shipped programs, for the packages (MIT, BSD, Apache and ISC require
// their notices to go with the binaries):
//   - Rust crates linked into the app, the CLI and the helper: normal
//     dependencies only (no dev or build dependencies, no proc-macros,
//     which run in the compiler and are not part of the binaries), for the
//     release target and default features (the Docker engine is left out);
//   - npm packages bundled into the frontend (production dependencies).
// Identical texts are written once, with the list of components using them.
//
// Usage: node scripts/third-party-licenses.mjs <output file>
// The engine (strongSwan, GMP, musl) is covered by THIRD_PARTY_NOTICES.md
// and the texts in its own share/licenses/.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const ROOTS = ['ikebox-app', 'ikebox-cli', 'ikebox-helper'];
const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-._].*)?$/i;

const out = process.argv[2];
if (!out) {
  console.error('usage: third-party-licenses.mjs <output file>');
  process.exit(2);
}

function licenseTexts(dir) {
  let names;
  try {
    names = readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  return names
    .filter((e) => e.isFile() && LICENSE_FILE.test(e.name))
    .map((e) => e.name)
    .sort()
    .map((name) => readFileSync(join(dir, name), 'utf8').replace(/\r\n/g, '\n').trim());
}

// --- Rust ---------------------------------------------------------------
const meta = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--locked', '--filter-platform', 'x86_64-unknown-linux-gnu'],
    { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  ),
);
const byId = new Map(meta.packages.map((p) => [p.id, p]));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const workspace = new Set(meta.workspace_members);
const isProcMacro = (p) => p.targets.every((t) => t.kind.includes('proc-macro'));

const seen = new Set();
const queue = meta.packages.filter((p) => ROOTS.includes(p.name)).map((p) => p.id);
if (queue.length !== ROOTS.length) {
  console.error(`expected the packages ${ROOTS.join(', ')} in the workspace`);
  process.exit(1);
}
while (queue.length > 0) {
  const id = queue.pop();
  if (seen.has(id)) continue;
  seen.add(id);
  if (isProcMacro(byId.get(id))) continue;
  for (const dep of nodes.get(id).deps) {
    if (dep.dep_kinds.some((k) => k.kind === null)) queue.push(dep.pkg);
  }
}

const components = [];
for (const id of seen) {
  const p = byId.get(id);
  if (workspace.has(id) || isProcMacro(p)) continue;
  components.push({
    label: `${p.name} ${p.version} (crate)`,
    license: p.license ?? (p.license_file ? `see ${p.license_file}` : 'unknown'),
    url: p.repository ?? `https://crates.io/crates/${p.name}`,
    texts: licenseTexts(dirname(p.manifest_path)),
  });
}

// --- npm ----------------------------------------------------------------
const lock = JSON.parse(readFileSync('package-lock.json', 'utf8'));
for (const [path, p] of Object.entries(lock.packages)) {
  if (path === '' || p.dev || p.devOptional) continue;
  const name = path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length);
  components.push({
    label: `${name} ${p.version} (npm)`,
    license: p.license ?? 'unknown',
    url: `https://www.npmjs.com/package/${name}`,
    texts: licenseTexts(path),
  });
}

components.sort((a, b) => a.label.localeCompare(b.label));
const unknown = components.filter((c) => c.license === 'unknown');
if (unknown.length > 0) {
  console.error(`components without a declared license: ${unknown.map((c) => c.label).join(', ')}`);
  process.exit(1);
}

// --- Output ---------------------------------------------------------------
const groups = new Map();
for (const c of components) {
  const text = c.texts.join('\n\n----\n\n');
  const key = createHash('sha256').update(text).digest('hex');
  if (!groups.has(key)) groups.set(key, { text, users: [] });
  groups.get(key).users.push(c);
}

const rule = '='.repeat(78);
const lines = [
  'Ikebox: third-party licenses',
  '',
  'Ikebox is free software under GPL-3.0-or-later (see LICENSE). The',
  'programs it ships are built with the components below, under their own',
  'licenses. The embedded strongSwan, GMP and musl are described in',
  'THIRD_PARTY_NOTICES.md; their texts are in the engine (share/licenses/).',
  '',
  `${components.length} components:`,
  '',
  ...components.map((c) => `  ${c.label}: ${c.license} <${c.url}>`),
  '',
];
for (const { text, users } of groups.values()) {
  lines.push(rule, ...users.map((c) => `${c.label}: ${c.license}`), rule, '');
  lines.push(text || '(no license text in the package; the license above applies)', '');
}
writeFileSync(out, `${lines.join('\n')}\n`);
console.log(`third-party licenses: ${components.length} components, ${groups.size} texts → ${out}`);
