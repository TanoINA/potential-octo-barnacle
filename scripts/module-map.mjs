#!/usr/bin/env node
// Structural map of a large Rust file: top-level items, domain clusters,
// external-reference counts (public surface vs internal), and a split proposal.
// Usage: node scripts/module-map.mjs <file.rs> <out.md>
import fs from 'node:fs';
import path from 'node:path';

const target = process.argv[2];
const outPath = process.argv[3];
if (!target || !outPath) {
  console.error('usage: node scripts/module-map.mjs <file.rs> <out.md>');
  process.exit(1);
}

const ROOT = process.cwd();
const src = fs.readFileSync(target, 'utf8');
const lines = src.split('\n');

// ---- collect every other backend .rs for reference counting ----
function walk(dir, acc = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (['node_modules', 'target', '.git', 'gen'].includes(e.name)) continue;
      walk(full, acc);
    } else if (e.name.endsWith('.rs') && path.resolve(full) !== path.resolve(target)) {
      acc.push(full);
    }
  }
  return acc;
}
const otherSources = walk(path.join(ROOT, 'src-tauri', 'src'))
  .map((f) => ({ f, text: fs.readFileSync(f, 'utf8') }));

const extRef = (name) => {
  // Only count real call syntax: `name(`. Bare word or `::name` matches would
  // pick up unrelated fields, locals, and type paths sharing the same name.
  const re = new RegExp(`(?:^|[^\\w.])${name}\\s*\\(`, 'g');
  let n = 0;
  const inFiles = [];
  for (const { f, text } of otherSources) {
    const m = text.match(re);
    if (m) {
      n += m.length;
      inFiles.push(path.basename(f));
    }
  }
  return { n, inFiles };
};

// ---- parse top-level items ----
const items = [];
let currentImpl = null;
const sections = [];
const testBlocks = [];

for (let i = 0; i < lines.length; i++) {
  const line = lines[i];
  const ln = i + 1;

  const sec = line.match(/^\/\/\s*[-=*_]{4,}\s*(.*)$/);
  if (sec) sections.push({ ln, label: sec[1].trim() || '(unnamed)' });
  const sec2 = line.match(/^\/\/\s*([A-Z][A-Za-z0-9 ,&/()'-]{6,})$/) 
  if (sec2 && /^\/\/\s*[A-Z]/.test(line) && line.length > 12 && !line.trim().startsWith('///')) {
    // heuristics: standalone capitalised comment = section header
    const prev = (lines[i - 1] || '').trim();
    const next = (lines[i + 1] || '').trim();
    if (prev === '' && next !== '') sections.push({ ln, label: sec2[1].trim() });
  }

  if (/^#\[cfg\(test\)\]/.test(line)) testBlocks.push({ ln, kind: 'cfg(test)' });
  if (/^mod tests\b/.test(line)) testBlocks.push({ ln, kind: 'mod tests' });

  let m;
  if ((m = line.match(/^impl(?:<[^>]*>)?\s+(?:[\w:]+::)?(\w+)/))) {
    currentImpl = m[1];
    items.push({ ln, kind: 'impl', name: m[1], implOf: m[1] });
    continue;
  }
  if ((m = line.match(/^pub(?:\(crate\))?\s+(?:async\s+)?fn\s+(\w+)/))) {
    items.push({ ln, kind: 'pub fn', name: m[1], implOf: currentImpl });
    continue;
  }
  if ((m = line.match(/^(?:async\s+)?fn\s+(\w+)/))) {
    items.push({ ln, kind: 'fn', name: m[1], implOf: currentImpl });
    continue;
  }
  if ((m = line.match(/^pub(?:\(crate\))?\s+(struct|enum|trait|type|const)\s+(\w+)/))) {
    items.push({ ln, kind: 'pub ' + m[1], name: m[2] });
    continue;
  }
  if ((m = line.match(/^(struct|enum|trait|type|const)\s+(\w+)/))) {
    items.push({ ln, kind: m[1], name: m[2] });
  }
}

// ---- compute test-only spans: `#[cfg(test)]` on any item, plus `mod tests` to EOF ----
const braceEnd = (from) => {
  let depth = 0;
  let seen = false;
  for (let i = from; i < lines.length; i++) {
    for (const ch of lines[i]) {
      if (ch === '{') { depth++; seen = true; }
      else if (ch === '}') { depth--; if (seen && depth === 0) return i + 1; }
      // a brace-less item (static/const/type) ends at its semicolon
      else if (ch === ';' && !seen && depth === 0) return i + 1;
    }
  }
  return lines.length;
};

const testRanges = [];
for (let i = 0; i < lines.length; i++) {
  if (/^mod tests\b/.test(lines[i])) {
    testRanges.push([i + 1, braceEnd(i + 1)]);
    break;
  }
  if (/^#\[cfg\(test\)\]/.test(lines[i])) {
    let j = i + 1;
    while (j < lines.length && /^\s*(#\[|\/\/)/.test(lines[j])) j++;
    const end = braceEnd(j);
    testRanges.push([i + 1, end]);
    i = end - 1;
  }
}
const testLineSet = new Set();
for (const [a, b] of testRanges) for (let k = a; k <= b; k++) testLineSet.add(k);
const isTestLine = (ln) => testLineSet.has(ln);

const testLines = testLineSet.size;
const authoredLines = lines.length - testLines;
const testStartLine = testRanges.length ? testRanges[0][0] : lines.length + 1;

// only lines outside any test span count as authored code
const authoredItems = items.filter((x) => !isTestLine(x.ln));
items.length = 0;
items.push(...authoredItems);

// ---- LOC per item (distance to the next top-level authored item) ----
const boundaries = [...items.map((x) => x.ln), lines.length + 1].sort((a, b) => a - b);
for (let i = 0; i < items.length; i++) {
  let next = boundaries.find((b) => b > items[i].ln) ?? lines.length + 1;
  // never let an item's span swallow a test-only span
  for (const [a] of testRanges) {
    if (a > items[i].ln && a < next) next = a;
  }
  items[i].loc = Math.max(next - items[i].ln, 1);
  // sanity: a single top-level fn with >400 lines is suspicious, report it
  items[i].big = items[i].loc > 400;
}

// ---- attach external refs ----
const refCache = new Map();
const attach = (it) => {
  if (it.loc === undefined) it.loc = 1;
  if (!refCache.has(it.name)) refCache.set(it.name, extRef(it.name));
  it.refs = refCache.get(it.name);
};

// ---- domain clustering by the most specific keyword in the name ----
// ordered longest-first so `self_improvement` wins over `save`.
const KEYWORDS = [
  'self_improvement', 'checkpoint', 'artifact', 'settings', 'workspace',
  'session_page', 'session', 'journal', 'catalog', 'profile', 'image',
  'pending', 'review', 'export', 'rename', 'pin', 'index', 'cache',
  'ack', 'path', 'dir', 'file', 'json', 'lock', 'atomic', 'temp',
];
const SPECIFIC = [...KEYWORDS].sort((a, b) => b.length - a.length);

function domainOf(name) {
  const lc = name.toLowerCase();
  for (const k of SPECIFIC) if (lc.includes(k)) return k;
  return 'core';
}

const fns = items.filter((it) => it.kind === 'pub fn' || it.kind === 'fn');
fns.forEach(attach);

const clusters = new Map();
for (const f of fns) {
  const d = domainOf(f.name);
  if (!clusters.has(d)) clusters.set(d, []);
  clusters.get(d).push(f);
}

// ---- report ----
const out = [];
const rel = path.relative(ROOT, path.resolve(target)).replace(/\\/g, '/');
out.push(`# Module map: \`${rel}\``, '');
out.push(`**${lines.length} lines total = ${authoredLines} authored + ${testLines} test** (${Math.round((testLines / lines.length) * 100)}% tests)`, '');
out.push(`${items.length} authored top-level items · ${fns.length} functions · ${testBlocks.length} test block(s)`, '');

const pubCount = fns.filter((f) => f.kind === 'pub fn').length;
const extFns = fns.filter((f) => f.refs.n > 0);
out.push(`**Public surface:** ${pubCount} \`pub fn\`, of which **${extFns.length} are referenced outside this file**.`, '');
out.push(`Internal-only functions (0 external refs): **${fns.length - extFns.length}**.`, '');

const itemLoc = items.reduce((s, x) => s + x.loc, 0);
const reconcile = itemLoc <= authoredLines ? 'OK' : 'OVERCOUNT';
out.push(`**Reconciliation:** item spans sum to ${itemLoc} lines vs ${authoredLines} authored lines → \`${reconcile}\``, '');

if (sections.length) {
  out.push('## Section markers found in file', '');
  out.push('| line | label |', '| --- | --- |');
  for (const s of sections.slice(0, 40)) out.push(`| ${s.ln} | ${s.label} |`);
  out.push('');
}

if (testBlocks.length) {
  out.push('## Test-only spans', '');
  out.push(`\`#[cfg(test)]\` / \`mod tests\` spans (excluded from all clusters below):`, '');
  for (const t of testBlocks) {
    out.push(`- line **${t.ln}** — \`${t.kind}\``);
  }
  out.push('');
}

// clusters sorted by total LOC
const clusterRows = [...clusters.entries()]
  .map(([d, fs_]) => ({
    domain: d,
    fns: fs_.length,
    loc: fs_.reduce((s, f) => s + f.loc, 0),
    ext: fs_.filter((f) => f.refs.n > 0).length,
    extHits: fs_.reduce((s, f) => s + f.refs.n, 0),
    sample: fs_.slice(0, 5).map((f) => f.name),
  }))
  .sort((a, b) => b.loc - a.loc);

out.push('## Clusters by domain keyword', '');
out.push('| domain | fns | ~LOC | fns used externally | ext hits | sample |', '| --- | --- | --- | --- | --- | --- |');
for (const c of clusterRows) {
  out.push(`| \`${c.domain}\` | ${c.fns} | ${c.loc} | ${c.ext} | ${c.extHits} | ${c.sample.join(', ')} |`);
}
out.push('');

// biggest single items
const top = [...fns].sort((a, b) => b.loc - a.loc).slice(0, 15);
out.push('## Largest single functions', '');
out.push('| line | fn | ~LOC | ext refs |', '| --- | --- | --- | --- |');
for (const t of top) out.push(`| ${t.ln} | \`${t.name}\` | ${t.loc} | ${t.refs.n} |`);
out.push('');

// public surface detail
out.push('## External surface (functions referenced from other files)', '');
out.push('| line | fn | hits | referenced in |', '| --- | --- | --- | --- |');
for (const f of extFns.sort((a, b) => b.refs.n - a.refs.n).slice(0, 40)) {
  const uniqFiles = [...new Set(f.refs.inFiles)].slice(0, 4).join(', ');
  out.push(`| ${f.ln} | \`${f.name}\` | ${f.refs.n} | ${uniqFiles} |`);
}
out.push('');

// types
const types = items.filter((i) => i.kind !== 'fn' && i.kind !== 'pub fn' && i.kind !== 'impl');
if (types.length) {
  out.push('## Types declared', '');
  out.push('```');
  out.push(types.map((t) => `${t.ln}  ${t.kind} ${t.name}`).join('\n'));
  out.push('```', '');
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, out.join('\n'));

// ---- console summary ----
console.log(`${rel}: ${lines.length}L items=${items.length} fns=${fns.length} pub=${pubCount} ext=${extFns.length}`);
console.log(`clusters=${clusterRows.length} sections=${sections.length} testBlocks=${testBlocks.length}`);
for (const c of clusterRows.slice(0, 14)) {
  console.log(`  ${c.domain.padEnd(18)} fns=${String(c.fns).padStart(3)} loc=${String(c.loc).padStart(5)} ext=${String(c.ext).padStart(3)}`);
}
console.log(`wrote ${outPath}`);
