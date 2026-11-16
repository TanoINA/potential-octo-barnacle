#!/usr/bin/env node
// Generates a compact, machine-readable index of the repo.
// One pass over the filesystem -> docs/index/files-<area>.md (one file per area).
import fs from 'node:fs';
import path from 'node:path';

const ROOT = process.argv[2] ? path.resolve(process.argv[2]) : process.cwd();
const OUT = path.join(ROOT, 'docs', 'index');

// Vendored third-party trees, QA evidence dumps, duplicated snapshots and
// generated code. None of these are authored source for this project.
const SKIP_DIRS = new Set([
  'node_modules', '.git', 'target', 'dist', 'build', 'coverage', '.vite',
  '.local',            // QA evidence + vendored rune-upstream + working-files snapshots
  'gen',               // src-tauri/gen/schemas (generated)
  'archive',           // docs/archive (historical)
  'investigation.cany', 'inbox', 'capture',
  'screenshots',
]);
const SKIP_FILES = new Set(['package-lock.json', 'Cargo.lock']);
const EXTS = new Set([
  '.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.css', '.html', '.toml',
  '.json', '.md', '.yml', '.yaml', '.ps1', '.sh', '.bat', '.sql', '.conf',
]);

// area -> matcher
const AREAS = [
  ['backend', (r) => r.startsWith('src-tauri/src/')],
  ['frontend', (r) => r.startsWith('src/')],
  ['rune', (r) => r.startsWith('rune/')],
  ['release', (r) => r.startsWith('scripts/') || r.startsWith('.github/')],
  ['docs', (r) => r.startsWith('docs/')],
  ['config', (r) => !r.includes('/')],
];
const areaOf = (rel) => {
  for (const [name, test] of AREAS) if (test(rel)) return name;
  return 'other';
};

function walk(dir, acc = []) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return acc;
  }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    const rel = path.relative(ROOT, full).replace(/\\/g, '/');
    if (e.isDirectory()) {
      if (SKIP_DIRS.has(e.name)) continue;
      if (rel === 'docs/index') continue; // own output
      walk(full, acc);
    } else if (e.isFile()) {
      if (SKIP_FILES.has(e.name)) continue;
      if (!EXTS.has(path.extname(e.name).toLowerCase())) continue;
      let size = 0;
      try { size = fs.statSync(full).size; } catch { continue; }
      acc.push({ rel, full, size });
    }
  }
  return acc;
}

const uniq = (a, cap) => [...new Set(a)].slice(0, cap);

const files = walk(ROOT).sort((a, b) => a.rel.localeCompare(b.rel));
const allCmds = new Set();
const allInvokes = new Set();
const allEvents = new Set();
const recs = [];

for (const f of files) {
  const ext = path.extname(f.rel).toLowerCase();
  let src = '';
  try { src = fs.readFileSync(f.full, 'utf8'); } catch { continue; }
  const loc = src.split('\n').length;
  const rec = { rel: f.rel, ext, loc, tests: 0, syms: [] };

  if (ext === '.rs') {
    const cmds = [...src.matchAll(/#\[tauri::command\][\s\S]{0,300}?\bfn\s+([a-z0-9_]+)/g)].map((m) => m[1]);
    const pubs = [...src.matchAll(/^\s*pub (?:async )?fn\s+([a-z0-9_]+)/gm)].map((m) => 'f:' + m[1]);
    const types = [...src.matchAll(/^\s*pub (?:struct|enum|trait)\s+([A-Za-z0-9_]+)/gm)].map((m) => 'T:' + m[1]);
    const emits = [...src.matchAll(/(?:emit|broadcast)\w*\(\s*"([A-Za-z0-9:_\-]+)"/g)].map((m) => m[1]);
    rec.tests = (src.match(/#\{(?:tokio::)?test\]|#\[(?:tokio::)?test\]/g) || []).length;
    rec.syms = uniq([...cmds.map((c) => '@' + c), ...types, ...pubs], 16);
    cmds.forEach((c) => allCmds.add(c));
    emits.forEach((e) => allEvents.add(e));
  } else if (['.ts', '.tsx', '.js', '.mjs', '.cjs'].includes(ext)) {
    const exp = [...src.matchAll(/^\s*export (?:default )?(?:async )?(?:function|const|class|interface|type|enum)\s+([A-Za-z0-9_]+)/gm)].map((m) => m[1]);
    const inv = [...src.matchAll(/\binvoke(?:<[^>]{0,150}>)?\(\s*['"`]([A-Za-z0-9_]+)['"`]/g)].map((m) => m[1]);
    const lis = [...src.matchAll(/\blisten(?:<[^>]{0,100}>)?\(\s*['"`]([A-Za-z0-9:_\-]+)['"`]/g)].map((m) => m[1]);
    rec.tests = (src.match(/\b(?:it|test)\(\s*['"`]/g) || []).length;
    rec.syms = uniq(exp, 16);
    inv.forEach((i) => allInvokes.add(i));
    lis.forEach((l) => allEvents.add(l));
  }
  recs.push(rec);
}

fs.mkdirSync(OUT, { recursive: true });
for (const f of fs.readdirSync(OUT)) fs.unlinkSync(path.join(OUT, f));

const groups = new Map();
for (const r of recs) {
  const a = areaOf(r.rel);
  if (!groups.has(a)) groups.set(a, []);
  groups.get(a).push(r);
}

const totalLoc = recs.reduce((s, r) => s + r.loc, 0);
const totalTests = recs.reduce((s, r) => s + r.tests, 0);

// ---- per-area files ----
const summary = [];
for (const [area, rs] of [...groups.entries()].sort()) {
  const loc = rs.reduce((s, r) => s + r.loc, 0);
  const tests = rs.reduce((s, r) => s + r.tests, 0);
  const body = [`# ${area} (${rs.length} files, ${loc} lines, ${tests} tests)`, ''];
  for (const r of rs) {
    const bits = [`${r.loc}L`];
    if (r.tests) bits.push(`${r.tests}T`);
    if (r.syms.length) bits.push(r.syms.join(' '));
    body.push(`${r.rel} | ${bits.join(' | ')}`);
  }
  const name = `files-${area}.md`;
  fs.writeFileSync(path.join(OUT, name), body.join('\n') + '\n');
  summary.push({ area, files: rs.length, loc, tests, name, bytes: fs.statSync(path.join(OUT, name)).size });
}

// ---- directory rollup (authored code only) ----
const dirs = new Map();
for (const r of recs) {
  const d = path.posix.dirname(r.rel);
  if (!dirs.has(d)) dirs.set(d, { files: 0, loc: 0 });
  const v = dirs.get(d);
  v.files += 1;
  v.loc += r.loc;
}

const md = ['# Project Index (generated)', ''];
md.push(`root: \`${ROOT.replace(/\\/g, '/')}\``, '');
md.push(`**${recs.length} authored files · ${totalLoc} lines · ${totalTests} test cases**`, '');
md.push(`tauri commands: ${allCmds.size} · frontend invokes: ${allInvokes.size} · event topics: ${allEvents.size}`, '');
md.push('Excluded from indexing: `.local/` (QA evidence, vendored `rune-upstream`, snapshot `working-files`), `src-tauri/gen/`, `docs/archive/`, `node_modules/`, `target/`, `dist/`, lockfiles.', '');

md.push('## Areas', '', '| area | files | lines | tests | file |', '| --- | --- | --- | --- | --- |');
for (const s of summary) md.push(`| ${s.area} | ${s.files} | ${s.loc} | ${s.tests} | \`docs/index/${s.name}\` (${s.bytes}B) |`);
md.push('');

md.push('## Directory rollup', '', '```');
[...dirs.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  .forEach(([d, v]) => md.push(`${String(v.loc).padStart(7)} L  ${String(v.files).padStart(3)} files  ${d}`));
md.push('```', '');

md.push('## Event topics', '', '```', [...allEvents].sort().join('\n'), '```', '');

const dead = [...allCmds].filter((c) => !allInvokes.has(c)).sort();
const orphan = [...allInvokes].filter((c) => !allCmds.has(c)).sort();
md.push('## IPC cross-check', '');
md.push(`**Rust commands never invoked by frontend (${dead.length})**`, '```', dead.join('\n') || '(none)', '```', '');
md.push(`**Frontend invokes with no Rust command (${orphan.length})**`, '```', orphan.join('\n') || '(none)', '```', '');

fs.writeFileSync(path.join(ROOT, 'docs', 'PROJECT_INDEX.md'), md.join('\n'));

const pad = (s, n) => String(s).padEnd(n);
console.log(`files=${recs.length} loc=${totalLoc} tests=${totalTests} areas=${summary.length}`);
console.log(`commands=${allCmds.size} invokes=${allInvokes.size} events=${allEvents.size} dead=${dead.length} orphan=${orphan.length}`);
for (const s of summary) console.log(`  ${pad(s.area, 10)} ${pad(s.files + ' files', 12)} ${pad(s.loc + ' L', 10)} ${pad(s.tests + ' T', 9)} ${s.name} ${s.bytes}B`);
