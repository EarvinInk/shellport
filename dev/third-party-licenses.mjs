#!/usr/bin/env node
// Generates THIRD_PARTY_LICENSES.md: the licenses of every Rust crate and npm
// package that ends up inside the Shellport app (the .deb and AppImage).
//
//   node dev/third-party-licenses.mjs            # writes THIRD_PARTY_LICENSES.md
//   node dev/third-party-licenses.mjs --check    # exits 1 if the file is out of date
//
// Needs `cargo` and `pnpm` on PATH and `pnpm install` to have run.
// Rust: crates reachable from shellport through normal and build dependencies
// on x86_64 Linux (dev-dependencies are excluded). npm: production
// dependencies of the UI (type-only @types/* packages are excluded).

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outFile = join(root, 'THIRD_PARTY_LICENSES.md');
const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense|ofl)([-_.].*)?$/i;

function licenseFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f) && statSync(join(dir, f)).isFile())
    .sort()
    .map((f) => ({ name: f, text: readFileSync(join(dir, f), 'utf8').replace(/\r\n/g, '\n').trim() }));
}

// ---------- Rust ----------
const meta = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--locked', '--filter-platform', 'x86_64-unknown-linux-gnu',
      '--manifest-path', join(root, 'src-tauri/Cargo.toml')],
    { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  ),
);
const pkgById = new Map(meta.packages.map((p) => [p.id, p]));
const nodeById = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const rootId = meta.resolve.root;
const seen = new Set([rootId]);
const queue = [rootId];
while (queue.length) {
  const node = nodeById.get(queue.shift());
  for (const dep of node.deps) {
    const shipped = dep.dep_kinds.some((k) => k.kind === null || k.kind === 'build');
    if (shipped && !seen.has(dep.pkg)) { seen.add(dep.pkg); queue.push(dep.pkg); }
  }
}
seen.delete(rootId);
const crates = [...seen].map((id) => pkgById.get(id)).map((p) => ({
  name: p.name,
  version: p.version,
  license: p.license || (p.license_file ? 'see license file' : 'unspecified'),
  url: `https://crates.io/crates/${p.name}/${p.version}`,
  files: licenseFiles(dirname(p.manifest_path)),
}));

// ---------- npm ----------
const npmJson = JSON.parse(
  execFileSync('pnpm', ['licenses', 'list', '--prod', '--json'], { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }),
);
const npm = Object.entries(npmJson).flatMap(([license, pkgs]) =>
  pkgs.filter((p) => !p.name.startsWith('@types/')).flatMap((p) =>
    p.versions.map((version, i) => ({
      name: p.name,
      version,
      license,
      url: `https://www.npmjs.com/package/${p.name}/v/${version}`,
      files: licenseFiles(p.paths?.[i] ?? p.paths?.[0]),
    })),
  ),
);

// ---------- Render ----------
const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
crates.sort(byName);
npm.sort(byName);

function section(title, pkgs) {
  const out = [`## ${title}`, ''];
  out.push('| Package | Version | License |', '|---|---|---|');
  for (const p of pkgs) out.push(`| [${p.name}](${p.url}) | ${p.version} | ${p.license.replace(/\|/g, '\\|')} |`);
  out.push('');

  // Group packages that ship byte-identical license texts.
  const groups = new Map();
  const missing = [];
  for (const p of pkgs) {
    if (!p.files.length) { missing.push(p); continue; }
    for (const f of p.files) {
      const key = createHash('sha256').update(f.text).digest('hex');
      if (!groups.has(key)) groups.set(key, { text: f.text, file: f.name, users: [] });
      groups.get(key).users.push(`${p.name} ${p.version}`);
    }
  }
  out.push(`### ${title}: license texts`, '');
  for (const g of [...groups.values()].sort((a, b) => a.users[0].localeCompare(b.users[0]))) {
    out.push(`**${g.file}**, used by: ${g.users.join(', ')}`, '', '```text', g.text.replace(/```/g, '` ` `'), '```', '');
  }
  if (missing.length) {
    out.push(
      `The following packages do not include a license file in their published source. They are used under the license stated in their package metadata (listed in the table above):`,
      '',
      missing.map((p) => `${p.name} ${p.version}`).join(', '),
      '',
    );
  }
  return out.join('\n');
}

const mpl = crates.filter((c) => /MPL/.test(c.license) && !/ OR /.test(c.license));
const counts = {};
for (const p of [...crates, ...npm]) counts[p.license] = (counts[p.license] ?? 0) + 1;

const doc = [
  '# Third-party licenses',
  '',
  'Shellport is released under the [MIT license](LICENSE). The Shellport app includes the third-party',
  'software listed below, each under its own license. This file is generated by',
  '`node dev/third-party-licenses.mjs` and is included in every release package.',
  '',
  `It covers ${crates.length} Rust crates and ${npm.length} npm packages.`,
  '',
  '| License | Packages |',
  '|---|---|',
  ...Object.entries(counts).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([l, n]) => `| ${l.replace(/\|/g, '\\|')} | ${n} |`),
  '',
  ...(mpl.length
    ? [
        '**MPL-2.0 components.** The following are used unmodified under the Mozilla Public License 2.0.',
        'Their source code is available from crates.io:',
        '',
        ...mpl.map((c) => `- [${c.name} ${c.version}](${c.url})`),
        '',
      ]
    : []),
  section('Rust crates', crates),
  section('npm packages', npm),
].join('\n') + '\n';

if (process.argv.includes('--check')) {
  const current = existsSync(outFile) ? readFileSync(outFile, 'utf8') : '';
  if (current !== doc) {
    console.error('THIRD_PARTY_LICENSES.md is out of date. Run: node dev/third-party-licenses.mjs');
    process.exit(1);
  }
  console.log('THIRD_PARTY_LICENSES.md is up to date.');
} else {
  writeFileSync(outFile, doc);
  console.log(`Wrote ${outFile} (${crates.length} crates, ${npm.length} npm packages, ${(doc.length / 1024).toFixed(0)} KB)`);
}
