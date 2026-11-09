import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { isIP } from 'node:net';
const here = path.dirname(fileURLToPath(import.meta.url));
export const sha256 = data => createHash('sha256').update(data).digest('hex');
const json = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
function requireThat(ok, message) { if (!ok) throw new Error(message); }
export function validateKey(value) {
  requireThat(typeof value === 'string' && /^[A-Za-z0-9+/]+={0,2}$/.test(value), 'Invalid public key encoding');
  const text = Buffer.from(value, 'base64').toString('utf8');
  const packet = Buffer.from(text.split(/\r?\n/).find(line => line && !line.startsWith('untrusted comment:')) ?? '', 'base64');
  requireThat(packet.length === 42 && packet.subarray(0, 2).toString() === 'Ed' && packet.subarray(10).some(v => v !== 0), 'Expected Ed25519 minisign public key');
  return value;
}
export function publicUrl(value) {
  const u = new URL(value);
  requireThat(u.protocol === 'https:' && !u.username && !u.password && !u.search && !u.hash && !isIP(u.hostname) && u.hostname.includes('.') && !u.hostname.endsWith('.') && !/(^|\.)(localhost|local|internal|test|invalid)$/.test(u.hostname), 'Require public static HTTPS URL without credentials, query or fragment');
  requireThat(!/%2f|%5c|\\/i.test(value), 'Ambiguous URL path');
  return u.href.replace(/\/$/, '');
}
export function preflight({ root, tag, publicKey, platform, thumbprint, timestampUrl }) {
  const pkg = json(path.join(root, 'package.json'));
  const tauri = json(path.join(root, 'src-tauri/tauri.conf.json'));
  const cargo = fs.readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8');
  const section = cargo.match(/^\[package\]\s*\r?\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const version = section?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  requireThat(/^\d+\.\d+\.\d+$/.test(pkg.version), 'Stable numeric release version required');
  requireThat(pkg.version === tauri.version && version === pkg.version && tag === `v${pkg.version}`, 'npm/Cargo/Tauri/tag versions must agree exactly');
  requireThat(/^[A-Za-z0-9._-]+$/.test(tauri.productName), 'Unsafe product name');
  const cli = json(path.join(root, 'node_modules/@tauri-apps/cli/package.json')).version;
  requireThat(/^2\.\d+\.\d+$/.test(cli) && Number(cli.split('.')[1]) >= 12, 'Tauri CLI >=2.12.0 stable v2 required');
  validateKey(publicKey);
  requireThat(['windows-x86_64', 'linux-x86_64'].includes(platform), 'Unsupported release target');
  const config = { plugins: { updater: { pubkey: publicKey, requireSignedVersion: true } }, bundle: { createUpdaterArtifacts: true } };
  if (thumbprint) {
    requireThat(/^[a-fA-F0-9]{40}$/.test(thumbprint), 'Invalid certificate thumbprint');
    publicUrl(timestampUrl);
    config.bundle.windows = { certificateThumbprint: thumbprint, digestAlgorithm: 'sha256', timestampUrl, tsp: true };
  }
  return config;
}
function run(file, args, env = process.env) {
  const result = spawnSync(file, args, { env, encoding: 'utf8', windowsHide: true, maxBuffer: 8 * 1024 * 1024 });
  requireThat(!result.error && result.status === 0, `Required verifier/tool failed: ${path.basename(file)}`);
  return result.stdout.trim();
}
export function verifySignature(artifact, signature, version, key) {
  // Verifier Rust memeriksa tanda tangan global serta versi dalam trusted comment.
  validateKey(key);
  run('cargo', ['run', '--offline', '--locked', '--quiet', '--manifest-path', path.join(here, 'release-policy-tests/Cargo.toml'), '--bin', 'aether-release-policy-tests', '--', 'verify', artifact, signature, version], { ...process.env, AETHER_UPDATE_PUBLIC_KEY: key });
}
export function expectedArtifacts(product, version, platform) {
  requireThat(/^[A-Za-z0-9._-]+$/.test(product) && /^\d+\.\d+\.\d+$/.test(version), 'Unsafe product/version');
  if (platform === 'windows-x86_64') return [ { kind: 'nsis', file: `${product}_${version}_x64-setup.exe` }, { kind: 'msi', file: `${product}_${version}_x64_en-US.msi` } ];
  requireThat(platform === 'linux-x86_64', 'Unsupported platform');
  return [ { kind: 'appimage', file: `${product}_${version}_amd64.AppImage` }, { kind: 'deb', file: `${product}_${version}_amd64.deb` }, { kind: 'rpm', file: `${product}-${version}-1.x86_64.rpm` } ];
}
function regular(file, startedAt) {
  const stat = fs.lstatSync(file);
  requireThat(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0, 'Artifact must be a nonempty regular file');
  if (startedAt) requireThat(stat.mtimeMs >= Date.parse(startedAt), 'Stale release artifact');
  return stat;
}
function signatureText(file) {
  const text = fs.readFileSync(file, 'utf8').trim();
  requireThat(text.length > 0 && text.length <= 16384 && /^[A-Za-z0-9+/]+={0,2}$/.test(text), 'Expected .sig file contents, not filename');
  return text;
}
function parseRpmRequirements(text) {
  requireThat(typeof text === 'string' && text.trim().length > 0, 'RPM dependency tags missing');
  return text.trim().split(/\r?\n/).map(line => {
    const parts = line.split('|');
    requireThat(parts.length === 3 && parts[0].length > 0 && !/[\u0000-\u001f\u007f]/.test(line) && /^(0|[1-9]\d*)$/.test(parts[1]) && Number(parts[1]) <= 0xffffffff && !/\s/.test(parts[2]), 'RPM dependency tags malformed');
    return { name: parts[0], flags: Number(parts[1]), version: parts[2] };
  });
}
export function inspectLinux(file, kind, version, query = run) {
  if (kind === 'appimage') {
    const fd = fs.openSync(file, 'r');
    const header = Buffer.alloc(64);
    try { requireThat(fs.readSync(fd, header, 0, 64, 0) === 64, 'Short AppImage'); } finally { fs.closeSync(fd); }
    requireThat(header.subarray(0, 4).equals(Buffer.from([127, 69, 76, 70])) && header[4] === 2 && header[5] === 1 && header.readUInt16LE(18) === 62 && header.subarray(8, 11).equals(Buffer.from([65, 73, 2])), 'Expected x86_64 type-2 ELF AppImage');
    requireThat((fs.statSync(file).mode & 0o111) !== 0, 'AppImage must be executable');
    return { format: 'AppImage type 2', arch: 'x86_64', versionBinding: 'authenticated trusted comment' };
  }
  if (kind === 'deb') {
    const field = name => run('dpkg-deb', ['--field', file, name]);
    const metadata = { package: field('Package'), version: field('Version'), arch: field('Architecture'), depends: field('Depends') };
    requireThat(metadata.version === version && metadata.arch === 'amd64' && /libc6\s*\(>=\s*2\.39\)/.test(metadata.depends) && /libwebkit2gtk-4\.1-0/.test(metadata.depends), 'DEB version/architecture/dependency mismatch');
    return metadata;
  }
  const fields = query('rpm', ['--query', '--package', '--queryformat', '%{VERSION}\n%{RELEASE}\n%{ARCH}', file]).split(/\r?\n/);
  const depends = query('rpm', ['--query', '--package', '--requires', file]);
  const requirements = parseRpmRequirements(query('rpm', ['--query', '--package', '--queryformat', '[%{REQUIRENAME}|%{REQUIREFLAGS}|%{REQUIREVERSION}\\n]', file]));
  // Tauri memakai Dependency::any: nama literal bukan pembanding versi RPM.
  // Terima hanya batas tepat 2.39 yang wajib, berupa rich tunggal atau tag >= terstruktur.
  const glibcRequirement = dep => (dep.name === '(glibc >= 2.39)' && dep.flags === 0 && dep.version === '') || (dep.name === 'glibc' && dep.flags === 12 && dep.version === '2.39');
  requireThat(!requirements.some(dep => /^glibc\s*[<>=]/.test(dep.name)), 'RPM literal glibc dependency is not a version requirement');
  requireThat(fields.length === 3 && fields[0] === version && fields[1] === '1' && fields[2] === 'x86_64' && requirements.some(glibcRequirement) && requirements.some(dep => dep.name === 'libwebkit2gtk-4.1.so.0()(64bit)' && dep.flags === 0 && dep.version === ''), 'RPM version/architecture/dependency mismatch');
  return { version: fields[0], release: fields[1], arch: fields[2], depends, requirements };
}
function identity({ version, sha, baseUrl }) {
  requireThat(/^\d+\.\d+\.\d+$/.test(version) && /^[a-f0-9]{40}$/.test(sha), 'Stable version and full source SHA required');
  return `${publicUrl(baseUrl)}/releases/${version}/${sha}`;
}
function exclusiveJson(file, value) { fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' }); }
export function createCandidate({ source, out, platform, version, sha, baseUrl, startedAt, key, product = 'AetherAgent', verifier = verifySignature, inspectLinux: inspect = inspectLinux }) {
  const prefix = identity({ version, sha, baseUrl });
  validateKey(key);
  requireThat(Number.isFinite(Date.parse(startedAt)), 'Build start time required');
  requireThat(!fs.existsSync(out), 'Candidate output already exists');
  const expected = expectedArtifacts(product, version, platform);
  const entries = expected.map(e => {
    const dir = path.join(source, e.kind);
    requireThat(!fs.lstatSync(dir).isSymbolicLink(), 'Bundle directory may not be symlink');
    const unexpected = fs.readdirSync(dir).filter(f => /\.(sig|exe|msi|deb|rpm|AppImage)$/i.test(f) && f !== e.file && f !== `${e.file}.sig`);
    requireThat(unexpected.length === 0, 'Unexpected installer/signature in bundle directory');
    const artifact = path.join(dir, e.file);
    const signature = `${artifact}.sig`;
    const stat = regular(artifact, startedAt);
    regular(signature, startedAt);
    verifier(artifact, signature, version, key);
    const metadata = platform === 'linux-x86_64' ? inspect(artifact, e.kind, version) : { authenticode: 'separate Windows build gate required' };
    return { ...e, artifact, signature, bytes: stat.size, sha256: sha256(fs.readFileSync(artifact)), signatureSha256: sha256(fs.readFileSync(signature)), metadata, url: `${prefix}/${e.file}`, signatureUrl: `${prefix}/${e.file}.sig`, signatureContents: signatureText(signature) };
  });
  fs.mkdirSync(out, { recursive: true });
  const artifacts = entries.map(({ artifact, signature, signatureContents, ...entry }) => {
    fs.copyFileSync(artifact, path.join(out, entry.file), fs.constants.COPYFILE_EXCL);
    fs.copyFileSync(signature, path.join(out, `${entry.file}.sig`), fs.constants.COPYFILE_EXCL);
    requireThat(sha256(fs.readFileSync(path.join(out, entry.file))) === entry.sha256 && sha256(fs.readFileSync(path.join(out, `${entry.file}.sig`))) === entry.signatureSha256, 'Artifact changed while copying');
    verifier(path.join(out, entry.file), path.join(out, `${entry.file}.sig`), version, key);
    return entry;
  });
  const record = { schema: 1, product, platform, version, sha, baseUrl: publicUrl(baseUrl), startedAt, builtAt: new Date().toISOString(), publicKeySha256: sha256(key), artifacts };
  exclusiveJson(path.join(out, 'candidate-record.json'), record);
  return record;
}
function verifyCandidate(source, record, options) {
  const { version, sha, baseUrl, keys, verifier = verifySignature } = options;
  const prefix = identity(options);
  const key = validateKey(keys[record.platform]);
  requireThat(record.schema === 1 && record.product === 'AetherAgent' && record.version === version && record.sha === sha && record.baseUrl === publicUrl(baseUrl) && record.publicKeySha256 === sha256(key), 'Candidate identity/public key mismatch');
  const expected = expectedArtifacts(record.product, version, record.platform);
  requireThat(Array.isArray(record.artifacts) && record.artifacts.length === expected.length, 'Incomplete candidate artifact set');
  for (const e of expected) {
    const entry = record.artifacts.find(a => a.kind === e.kind && a.file === e.file);
    requireThat(entry && entry.url === `${prefix}/${e.file}` && entry.signatureUrl === `${prefix}/${e.file}.sig`, 'Invalid immutable artifact URL or name');
    const artifact = path.join(source, e.file);
    const signature = `${artifact}.sig`;
    const stat = regular(artifact);
    regular(signature);
    requireThat(stat.size === entry.bytes && sha256(fs.readFileSync(artifact)) === entry.sha256 && sha256(fs.readFileSync(signature)) === entry.signatureSha256, 'Artifact hash/size mismatch');
    signatureText(signature);
    verifier(artifact, signature, version, key);
  }
  return record;
}
function manifestFor(sourceFor, records, version) {
  const platforms = {};
  for (const record of records) {
    for (const e of record.artifacts) {
      const entry = { signature: signatureText(path.join(sourceFor(record), `${e.file}.sig`)), url: e.url, sha256: e.sha256 };
      platforms[`${record.platform}-${e.kind}`] = entry;
      if (e.kind === 'nsis' || e.kind === 'appimage') platforms[record.platform] = entry;
    }
  }
  return { version, notes: `AetherAgent ${version}`, pub_date: new Date(Math.max(...records.map(r => Date.parse(r.builtAt)))).toISOString(), platforms };
}
export function prepareRelease({ candidates, out, ...options }) {
  identity(options);
  requireThat(!fs.existsSync(out), 'Prepared output already exists');
  const records = candidates.map(source => verifyCandidate(source, json(path.join(source, 'candidate-record.json')), options));
  requireThat(records.length === 2 && new Set(records.map(r => r.platform)).size === 2, 'Both Windows and Linux candidates required');
  fs.mkdirSync(out, { recursive: true });
  for (let i = 0; i < records.length; i++) for (const entry of records[i].artifacts) for (const file of [entry.file, `${entry.file}.sig`]) fs.copyFileSync(path.join(candidates[i], file), path.join(out, file), fs.constants.COPYFILE_EXCL);
  const record = { schema: 1, version: options.version, sha: options.sha, baseUrl: publicUrl(options.baseUrl), candidates: records };
  exclusiveJson(path.join(out, 'release-record.json'), record);
  // Manifest ditulis sesudah semua salinan lolos pemeriksaan ulang.
  for (const candidate of records) verifyCandidate(out, candidate, options);
  exclusiveJson(path.join(out, 'latest.json'), manifestFor(() => out, records, options.version));
  return verifyPrepared({ source: out, ...options });
}
export function verifyPrepared({ source, ...options }) {
  identity(options);
  const record = json(path.join(source, 'release-record.json'));
  requireThat(record.schema === 1 && record.version === options.version && record.sha === options.sha && record.baseUrl === publicUrl(options.baseUrl), 'Release record identity mismatch');
  requireThat(record.candidates?.length === 2 && new Set(record.candidates.map(r => r.platform)).size === 2, 'Incomplete/duplicate release platforms');
  for (const candidate of record.candidates) verifyCandidate(source, candidate, options);
  const expected = manifestFor(() => source, record.candidates, options.version);
  requireThat(JSON.stringify(json(path.join(source, 'latest.json'))) === JSON.stringify(expected), 'Manifest differs from verified artifacts/signature contents');
  return record;
}
function promotionPath(value) {
  requireThat(typeof value === 'string' && value.length > 0 && !value.includes('\0'), 'Explicit promotion path required');
  requireThat(!value.split(/[\\/]/).includes('..'), 'Ambiguous promotion path collision');
  if (process.platform === 'win32') {
    // Refuse device/UNC namespaces, ADS, DOS names and Win32 normalization aliases.
    requireThat(!/^[\\/]{2}/.test(value), 'Reparse/device/network promotion paths forbidden');
    const tail = value.replace(/^[A-Za-z]:[\\/]/, '');
    requireThat(!tail.includes(':') && tail.split(/[\\/]/).every(p => !/[. ]$/.test(p) && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p)), 'Ambiguous Windows path collision');
  }
  return path.resolve(value);
}
function ancestry(file, { kind = 'directory', missing = false, absent = false } = {}) {
  const root = path.parse(file).root;
  const parts = file.slice(root.length).split(path.sep).filter(Boolean);
  const entries = [root];
  for (const part of parts) entries.push(path.join(entries.at(-1), part));
  const existing = [], create = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    // lstat, not existsSync: a dangling symlink is a collision, not an absent path.
    const stat = fs.lstatSync(entry, { throwIfNoEntry: false });
    const leaf = i === entries.length - 1;
    if (!stat) {
      requireThat(missing || (leaf && absent), `Required promotion path does not exist: ${entry}`);
      create.push(entry);
      continue;
    }
    requireThat(!stat.isSymbolicLink(), `Promotion symlink/reparse path forbidden: ${entry}`);
    const canonical = fs.realpathSync.native(entry);
    const normalize = p => process.platform === 'win32' ? p.toLowerCase() : p;
    requireThat(normalize(canonical) === normalize(entry), `Promotion reparse/alias path forbidden: ${entry}`);
    requireThat(!(leaf && absent), `Immutable/temporary path already exists: ${entry}`);
    requireThat(leaf && kind === 'file' ? stat.isFile() && stat.nlink === 1 : stat.isDirectory(), `Promotion path collision: expected unlinked regular file or directory: ${entry}`);
    existing.push(entry);
  }
  return { existing, create };
}
function noReparse(paths) {
  if (process.platform !== 'win32' || paths.length === 0) return;
  // Node exposes junctions as symlinks, but not every Windows reparse tag.
  // Check the raw attribute too; missing PowerShell/access errors fail closed.
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', "$ErrorActionPreference='Stop'; foreach ($p in (ConvertFrom-Json $env:AETHER_PROMOTION_PATHS)) { if (([System.IO.File]::GetAttributes($p) -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Promotion reparse path forbidden' } }"], { ...process.env, AETHER_PROMOTION_PATHS: JSON.stringify([...new Set(paths)]) });
}
function overlaps(a, b) {
  const within = (parent, child) => {
    const relative = path.relative(parent, child);
    return relative === '' || (relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative));
  };
  return within(a, b) || within(b, a);
}
export function promoteLocal({ source, destination, ...options }) {
  identity(options);
  source = promotionPath(source);
  destination = promotionPath(destination);
  requireThat(!overlaps(source, destination), 'Promotion source/destination overlap forbidden');
  const final = path.join(destination, 'releases', options.version, options.sha);
  const latest = path.join(destination, 'latest.json');
  const temp = path.join(destination, `.latest-${randomUUID()}.json`);
  // Validate the whole ancestry, immutable subtree and pointer BEFORE any writes.
  const sourcePaths = [ancestry(source), ...['release-record.json', 'latest.json'].map(file => ancestry(path.join(source, file), { kind: 'file' }))];
  const plan = ancestry(final, { missing: true, absent: true });
  const pointerPaths = [ancestry(latest, { kind: 'file', missing: true }), ancestry(temp, { missing: true, absent: true })];
  noReparse([...sourcePaths, plan, ...pointerPaths].flatMap(p => p.existing));
  const record = verifyPrepared({ source, ...options });
  const files = [...record.candidates.flatMap(c => c.artifacts.flatMap(e => [e.file, `${e.file}.sig`])), 'release-record.json', 'latest.json'];
  noReparse(files.flatMap(file => ancestry(path.join(source, file), { kind: 'file' }).existing));
  // No recursive mkdir: every previously missing component must still be absent.
  // A trusted, single-writer filesystem is required; path checks are not openat locks.
  for (const dir of plan.create) {
    noReparse(ancestry(path.dirname(dir)).existing);
    fs.mkdirSync(dir);
  }
  for (const file of files) fs.copyFileSync(path.join(source, file), path.join(final, file), fs.constants.COPYFILE_EXCL);
  noReparse(ancestry(final).existing);
  verifyPrepared({ source: final, ...options });
  noReparse(ancestry(latest, { kind: 'file', missing: true }).existing);
  fs.copyFileSync(path.join(final, 'latest.json'), temp, fs.constants.COPYFILE_EXCL);
  // Only the channel pointer may be replaced, atomically and on the same filesystem.
  fs.renameSync(temp, latest);
  return record;
}
function main() {
  const command = process.argv[2];
  const strings = ['root', 'tag', 'public-key', 'platform', 'thumbprint', 'timestamp-url', 'source', 'out', 'version', 'sha', 'base-url', 'started-at', 'windows', 'linux', 'destination'];
  const { values: v } = parseArgs({ args: process.argv.slice(3), options: Object.fromEntries(strings.map(k => [k, { type: 'string' }])) });
  const key = v['public-key'] || process.env.AETHER_UPDATE_PUBLIC_KEY;
  const options = { version: v.version, sha: v.sha, baseUrl: v['base-url'] || process.env.AETHER_UPDATE_BASE_URL, keys: { 'windows-x86_64': process.env.AETHER_WINDOWS_UPDATE_PUBLIC_KEY, 'linux-x86_64': process.env.AETHER_LINUX_UPDATE_PUBLIC_KEY } };
  if (command === 'preflight') {
    console.log(JSON.stringify(preflight({ root: v.root, tag: v.tag, publicKey: key, platform: v.platform, thumbprint: v.thumbprint, timestampUrl: v['timestamp-url'] })));
    return;
  }
  if (command === 'candidate') createCandidate({ source: v.source, out: v.out, platform: v.platform, startedAt: v['started-at'], key, ...options });
  else if (command === 'prepare') prepareRelease({ candidates: [v.windows, v.linux], out: v.out, ...options });
  else if (command === 'verify') verifyPrepared({ source: v.source, ...options });
  else if (command === 'promote-local') promoteLocal({ source: v.source, destination: v.destination, ...options });
  else throw new Error('Expected preflight, candidate, prepare, verify or promote-local');
  console.log(`PASS: ${command}; artifacts, hashes, signatures and exact versions verified`);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(`Release pipeline rejected: ${error.message}`); process.exitCode = 1; }
}