import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const pipeline = await import('./release-pipeline.mjs');
const key = Buffer.from(`untrusted comment: fixture\n${Buffer.from([69, 100, ...Array.from({length: 40}, (_, i) => i + 1)]).toString('base64')}\n`).toString('base64');
const signature = Buffer.from('public test signature contents, not a filename').toString('base64');
const sha = 'a'.repeat(40);
const baseUrl = 'https://updates.example.com/aether';
function fixture(platform) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'aether-pipeline-node-'));
  const source = path.join(root, 'bundle');
  const names = platform === 'windows-x86_64' ? [ ['nsis', 'AetherAgent_0.3.0_x64-setup.exe'], ['msi', 'AetherAgent_0.3.0_x64_en-US.msi'] ] : [ ['appimage', 'AetherAgent_0.3.0_amd64.AppImage'], ['deb', 'AetherAgent_0.3.0_amd64.deb'], ['rpm', 'AetherAgent-0.3.0-1.x86_64.rpm'] ];
  for (const [kind, name] of names) {
    fs.mkdirSync(path.join(source, kind), { recursive: true });
    fs.writeFileSync(path.join(source, kind, name), `fixture ${kind}`);
    fs.writeFileSync(path.join(source, kind, `${name}.sig`), signature);
  }
  return { root, source, names, out: path.join(root, 'candidate'), platform, version: '0.3.0', sha, baseUrl, startedAt: new Date(Date.now() - 60_000).toISOString(), key, verifier: () => {}, inspectLinux: () => ({ fixture: true }) };
}
test('candidate requires a complete fresh exact artifact set; manifest contains signature contents and immutable URLs', () => {
  assert.equal(typeof pipeline.createCandidate, 'function', 'Missing candidate pipeline');
  const win = fixture('windows-x86_64');
  const linux = fixture('linux-x86_64');
  pipeline.createCandidate(win);
  pipeline.createCandidate(linux);
  const keys = { 'windows-x86_64': key, 'linux-x86_64': key };
  const out = path.join(win.root, 'prepared');
  pipeline.prepareRelease({ candidates: [win.out, linux.out], out, baseUrl, sha, version: '0.3.0', keys, verifier: () => {} });
  const manifest = JSON.parse(fs.readFileSync(path.join(out, 'latest.json')));
  assert.equal(manifest.platforms['windows-x86_64'].signature, signature);
  assert.equal(manifest.platforms['linux-x86_64'].signature, signature);
  assert.match(manifest.platforms['linux-x86_64-deb'].url, /\/releases\/0\.3\.0\/a{40}\/AetherAgent_0\.3\.0_amd64\.deb$/);
  assert.equal(Object.keys(manifest.platforms).length, 7);
  pipeline.verifyPrepared({ source: out, baseUrl, sha, version: '0.3.0', keys, verifier: () => {} });
  const deployment = path.join(win.root, 'public');
  pipeline.promoteLocal({ source: out, destination: deployment, baseUrl, sha, version: '0.3.0', keys, verifier: () => {} });
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(deployment, 'latest.json'))), manifest);
  assert.throws(() => pipeline.promoteLocal({ source: out, destination: deployment, baseUrl, sha, version: '0.3.0', keys, verifier: () => {} }), /exist/);
  const record = JSON.parse(fs.readFileSync(path.join(out, 'release-record.json')));
  assert.equal(record.sha, sha);
  assert.equal(record.candidates.flatMap(c => c.artifacts).length, 5);
  fs.appendFileSync(path.join(out, win.names[0][1]), 'tamper');
  assert.throws(() => pipeline.verifyPrepared({ source: out, baseUrl, sha, version: '0.3.0', keys, verifier: () => {} }), /hash|size/i);
  const untouched = path.join(win.root, 'not-published');
  assert.throws(() => pipeline.promoteLocal({ source: out, destination: untouched, baseUrl, sha, version: '0.3.0', keys, verifier: () => {} }));
  assert.equal(fs.existsSync(path.join(untouched, 'latest.json')), false);
});
test('missing, stale, symlink, wrong key, bad URL, signature failure and unexpected exact names fail closed', () => {
  assert.equal(typeof pipeline.createCandidate, 'function', 'Missing candidate pipeline');
  for (const mode of ['missing', 'stale', 'symlink', 'signature', 'key', 'url', 'name']) {
    const f = fixture('windows-x86_64');
    const file = path.join(f.source, ...f.names[0]);
    if (mode === 'missing') fs.renameSync(`${file}.sig`, `${file}.missing`);
    if (mode === 'stale') fs.utimesSync(file, new Date(0), new Date(0));
    if (mode === 'symlink') { const dir = path.dirname(file); fs.renameSync(dir, dir + '.real'); fs.symlinkSync(dir + '.real', dir, 'junction'); }
    if (mode === 'signature') f.verifier = () => { throw new Error('Invalid real signature'); };
    if (mode === 'key') f.key = 'not-a-key';
    if (mode === 'url') f.baseUrl = 'https://updates.example.com/a?token=secret';
    if (mode === 'name') fs.renameSync(file, `${file}.old`);
    assert.throws(() => pipeline.createCandidate(f), undefined, mode);
    assert.equal(fs.existsSync(path.join(f.out, 'candidate-record.json')), false, mode);
  }
});
test('public endpoint policy rejects local, credentialed, redirected-token style and ambiguous URLs', () => {
  for (const url of ['http://updates.example.com', 'https://localhost/x', 'https://127.0.0.1/x', 'https://[::1]/x', 'https://u:p@updates.example.com/x', 'https://updates.example.com/x?q=1', 'https://updates.example.com/x#x', 'https://updates.internal/x', 'https://updates.example.com/x%2fy']) assert.throws(() => pipeline.publicUrl(url));
});
test('prepared release rejects platform-key swaps and manifest/signature/identity tampering before pointer replacement', () => {
  const win = fixture('windows-x86_64'), linux = fixture('linux-x86_64');
  const otherKey = Buffer.from(`untrusted comment: other\n${Buffer.from([69,100,...Array(40).fill(2)]).toString('base64')}\n`).toString('base64');
  linux.key = otherKey;
  pipeline.createCandidate(win); pipeline.createCandidate(linux);
  const options = { baseUrl, sha, version: '0.3.0', keys: { 'windows-x86_64': key, 'linux-x86_64': otherKey }, verifier: () => {} };
  const out = path.join(win.root, 'prepared');
  pipeline.prepareRelease({ candidates: [win.out, linux.out], out, ...options });
  const verify = opts => pipeline.verifyPrepared({ source: out, ...options, ...opts });
  assert.throws(() => verify({ keys: { 'windows-x86_64': otherKey, 'linux-x86_64': key } }), /key/i);
  assert.throws(() => verify({ sha: 'b'.repeat(40) }), /identity/i);
  assert.throws(() => verify({ verifier: () => { throw new Error('Crypto failure'); } }), /Crypto/);
  const manifestFile = path.join(out, 'latest.json');
  const original = fs.readFileSync(manifestFile);
  const manifest = JSON.parse(original);
  manifest.platforms['windows-x86_64'].signature = win.names[0][1] + '.sig';
  fs.writeFileSync(manifestFile, JSON.stringify(manifest));
  const dest = path.join(win.root, 'public');
  fs.mkdirSync(dest);
  fs.writeFileSync(path.join(dest, 'latest.json'), 'old pointer');
  assert.throws(() => pipeline.promoteLocal({ source: out, destination: dest, ...options }), /Manifest/);
  assert.equal(fs.readFileSync(path.join(dest, 'latest.json'), 'utf8'), 'old pointer');
  fs.writeFileSync(manifestFile, original);
  fs.appendFileSync(path.join(out, linux.names[0][1] + '.sig'), 'x');
  assert.throws(() => verify({}), /hash/);
});
function preparedFixture(shaValue = sha) {
  const win = fixture('windows-x86_64'), linux = fixture('linux-x86_64');
  win.sha = linux.sha = shaValue;
  pipeline.createCandidate(win); pipeline.createCandidate(linux);
  const options = { baseUrl, sha: shaValue, version: '0.3.0', keys: { 'windows-x86_64': key, 'linux-x86_64': key }, verifier: () => {} };
  const source = path.join(win.root, 'prepared');
  pipeline.prepareRelease({ candidates: [win.out, linux.out], out: source, ...options });
  return { root: win.root, source, destination: path.join(win.root, 'public'), options };
}
function snapshot(root) {
  const result = {};
  function visit(file) {
    const stat = fs.lstatSync(file);
    const relative = path.relative(root, file) || '.';
    if (stat.isSymbolicLink()) result[relative] = ['link', fs.readlinkSync(file)];
    else if (stat.isDirectory()) {
      result[relative] = ['directory'];
      for (const name of fs.readdirSync(file).sort()) visit(path.join(file, name));
    } else result[relative] = ['file', fs.readFileSync(file).toString('base64')];
  }
  visit(root);
  return result;
}
test('promotion replaces only latest atomically and preserves prior immutable release bytes', () => {
  const first = preparedFixture();
  const second = preparedFixture('b'.repeat(40));
  second.destination = first.destination;
  pipeline.promoteLocal({ ...first, ...first.options });
  const oldRelease = path.join(first.destination, 'releases', first.options.version, first.options.sha);
  const before = snapshot(oldRelease);
  const rename = fs.renameSync;
  let observedRename = false;
  fs.renameSync = (from, to) => {
    observedRename = true;
    assert.equal(path.dirname(from), first.destination);
    assert.match(path.basename(from), /^\.latest-[a-f0-9-]+\.json$/);
    assert.equal(to, path.join(first.destination, 'latest.json'));
    assert.equal(JSON.parse(fs.readFileSync(to)).platforms['windows-x86_64'].url.includes(sha), true, 'old pointer stays active until rename');
    assert.equal(JSON.parse(fs.readFileSync(from)).platforms['windows-x86_64'].url.includes(second.options.sha), true);
    return rename(from, to);
  };
  try { pipeline.promoteLocal({ ...second, ...second.options }); } finally { fs.renameSync = rename; }
  assert.equal(observedRename, true);
  assert.deepEqual(snapshot(oldRelease), before);
  assert.equal(JSON.parse(fs.readFileSync(path.join(first.destination, 'latest.json'))).platforms['linux-x86_64'].url.includes(second.options.sha), true);
  assert.deepEqual(fs.readdirSync(first.destination).sort(), ['latest.json', 'releases']);
});
test('destination verification failure leaves the active pointer unchanged and never overwrites partial immutable output', () => {
  const f = preparedFixture();
  fs.mkdirSync(f.destination);
  fs.writeFileSync(path.join(f.destination, 'latest.json'), 'previous active manifest');
  const verifier = artifact => {
    if (artifact.startsWith(f.destination + path.sep)) throw new Error('Destination verification fixture failure');
  };
  assert.throws(() => pipeline.promoteLocal({ ...f, ...f.options, verifier }), /Destination verification/);
  assert.equal(fs.readFileSync(path.join(f.destination, 'latest.json'), 'utf8'), 'previous active manifest');
  assert.throws(() => pipeline.promoteLocal({ ...f, ...f.options }), /already exists/);
});
test('promotion rejects ambiguous Windows aliases before writes', { skip: process.platform !== 'win32' }, () => {
  const f = preparedFixture();
  for (const suffix of ['public.', 'public ', 'NUL', 'public:stream', 'missing/../public']) {
    const before = snapshot(f.root);
    assert.throws(() => pipeline.promoteLocal({ ...f, ...f.options, destination: `${f.root}/${suffix}` }), /collision|Ambiguous/);
    assert.deepEqual(snapshot(f.root), before);
  }
});
test('promotion rejects destination collisions and junctions before writing any bytes', async t => {
  const modes = ['parent-file', 'destination-file', 'destination-junction', 'releases-file', 'releases-junction', 'version-file', 'version-junction', 'final-directory', 'final-file', 'final-dangling-junction', 'latest-directory', 'latest-junction', 'latest-hardlink'];
  for (const mode of modes) await t.test(mode, () => {
    const f = preparedFixture();
    const outside = path.join(f.root, 'outside');
    fs.mkdirSync(outside);
    fs.writeFileSync(path.join(outside, 'keep'), 'must remain untouched');
    if (mode === 'parent-file') {
      fs.writeFileSync(f.destination, 'parent collision');
      f.destination = path.join(f.destination, 'public');
    } else if (mode === 'destination-file') fs.writeFileSync(f.destination, 'collision');
    else if (mode === 'destination-junction') fs.symlinkSync(outside, f.destination, 'junction');
    else {
      fs.mkdirSync(f.destination);
      const releases = path.join(f.destination, 'releases');
      const version = path.join(releases, f.options.version);
      const final = path.join(version, f.options.sha);
      const latest = path.join(f.destination, 'latest.json');
      if (!mode.startsWith('latest-')) fs.writeFileSync(latest, 'old pointer');
      if (mode === 'releases-file') fs.writeFileSync(releases, 'collision');
      else if (mode === 'releases-junction') fs.symlinkSync(outside, releases, 'junction');
      else if (mode.startsWith('version-')) {
        fs.mkdirSync(releases);
        if (mode === 'version-file') fs.writeFileSync(version, 'collision');
        else fs.symlinkSync(outside, version, 'junction');
      } else if (mode.startsWith('final-')) {
        fs.mkdirSync(version, { recursive: true });
        if (mode === 'final-directory') fs.mkdirSync(final);
        else if (mode === 'final-file') fs.writeFileSync(final, 'collision');
        else fs.symlinkSync(path.join(f.root, 'absent'), final, 'junction');
      } else if (mode === 'latest-directory') fs.mkdirSync(latest);
      else if (mode === 'latest-junction') fs.symlinkSync(outside, latest, 'junction');
      else fs.linkSync(path.join(outside, 'keep'), latest);
    }
    const before = snapshot(f.root);
    assert.throws(() => pipeline.promoteLocal({ ...f, ...f.options }), /symlink|reparse|collision|exist|directory|regular|link/i);
    assert.deepEqual(snapshot(f.root), before, 'rejection must precede all destination writes');
  });
});
test('promotion rejects source/destination overlap and linked source ancestry without writes', async t => {
  for (const mode of ['same', 'destination-inside-source', 'source-inside-destination', 'source-junction', 'source-ancestor-junction', 'source-record-junction', 'source-latest-junction']) await t.test(mode, () => {
    const f = preparedFixture();
    if (mode === 'same') f.destination = f.source;
    if (mode === 'destination-inside-source') f.destination = path.join(f.source, 'public');
    if (mode === 'source-inside-destination') f.destination = f.root;
    if (mode === 'source-junction') {
      const linked = path.join(f.root, 'source-link');
      fs.symlinkSync(f.source, linked, 'junction');
      f.source = linked;
    }
    if (mode === 'source-ancestor-junction') {
      const linked = path.join(f.root, 'root-link');
      fs.symlinkSync(f.root, linked, 'junction');
      f.source = path.join(linked, 'prepared');
    }
    if (mode === 'source-record-junction' || mode === 'source-latest-junction') {
      const file = path.join(f.source, mode === 'source-record-junction' ? 'release-record.json' : 'latest.json');
      fs.renameSync(file, file + '.original');
      fs.symlinkSync(f.root, file, 'junction');
    }
    const before = snapshot(f.root);
    assert.throws(() => pipeline.promoteLocal({ ...f, ...f.options }), /overlap|symlink|reparse/i);
    assert.deepEqual(snapshot(f.root), before);
  });
});
test('Linux config uses a rich glibc requirement for Tauri Dependency::any', () => {
  const config = JSON.parse(fs.readFileSync(new URL('../src-tauri/tauri.linux.conf.json', import.meta.url), 'utf8'));
  assert.deepEqual(config.bundle.linux.rpm.depends, ['(glibc >= 2.39)', 'libwebkit2gtk-4.1.so.0()(64bit)', 'libgtk-3.so.0()(64bit)']);
  assert.deepEqual(config.bundle.linux.deb.depends, ['libc6 (>= 2.39)', 'libwebkit2gtk-4.1-0', 'libgtk-3-0']);
});
function rpmQueryFixture(requirements, { version = '0.3.0', release = '1', arch = 'x86_64', display = 'glibc >= 2.39\nlibwebkit2gtk-4.1.so.0()(64bit)' } = {}) {
  const calls = [];
  const query = (tool, args) => {
    assert.equal(tool, 'rpm');
    assert.equal(args.at(-1), 'fixture.rpm');
    calls.push(args);
    if (args.includes('--requires')) return display;
    const format = args[args.indexOf('--queryformat') + 1];
    if (format === '%{VERSION}\n%{RELEASE}\n%{ARCH}') return `${version}\n${release}\n${arch}`;
    assert.equal(format, '[%{REQUIRENAME}|%{REQUIREFLAGS}|%{REQUIREVERSION}\\n]');
    return requirements;
  };
  return { calls, inspect: () => pipeline.inspectLinux('fixture.rpm', 'rpm', '0.3.0', query) };
}
test('RPM inspection rejects a literal ANY glibc capability despite convincing --requires text', () => {
  const f = rpmQueryFixture('glibc >= 2.39|0|\nlibwebkit2gtk-4.1.so.0()(64bit)|0|');
  assert.throws(f.inspect, /RPM.*(glibc|dependency)/i);
});
test('RPM inspection accepts rich and structured glibc tags, never display text', () => {
  for (const glibc of ['(glibc >= 2.39)|0|', 'glibc|12|2.39']) {
    const f = rpmQueryFixture(`${glibc}\nlibwebkit2gtk-4.1.so.0()(64bit)|0|`, { display: 'untrusted human-readable output' });
    const metadata = f.inspect();
    assert.equal(metadata.version, '0.3.0');
    assert.equal(metadata.depends, 'untrusted human-readable output');
    assert.deepEqual(metadata.requirements[0], { name: glibc.split('|')[0], flags: Number(glibc.split('|')[1]), version: glibc.split('|')[2] });
    assert.ok(f.calls.some(args => args.includes('[%{REQUIRENAME}|%{REQUIREFLAGS}|%{REQUIREVERSION}\\n]')));
  }
});
function workflowText(name) {
  return fs.readFileSync(new URL(`../.github/workflows/${name}.yml`, import.meta.url), 'utf8');
}
function runBlock(text, marker) {
  const section = text.slice(text.indexOf(marker));
  assert.ok(text.includes(marker), `Missing workflow step: ${marker}`);
  const lines = section.split(/\r?\n/);
  const start = lines.findIndex(line => line.trim() === 'run: |') + 1;
  assert.ok(start > 0, 'Missing shell block');
  const indent = lines[start].match(/^ */)[0].length;
  const end = lines.findIndex((line, i) => i >= start && line.trim() && line.match(/^ */)[0].length < indent);
  return lines.slice(start, end < 0 ? undefined : end).map(line => line.slice(indent)).join('\n');
}
test('promotion workflow binds authenticated candidate head SHA and branch to the requested tag', async () => {
  const block = runBlock(workflowText('promote-release'), 'name: Check trusted successful build provenance');
  const script = block.match(/node --input-type=module <<'JS'\n([\s\S]*?)\nJS/)?.[1];
  assert.ok(script, 'Missing provenance verifier');
  const check = new (Object.getPrototypeOf(async function () {}).constructor)('fetch', 'process', script);
  const good = { conclusion: 'success', status: 'completed', path: '.github/workflows/signed-release.yml', repository: { full_name: 'TanoINA/potential-octo-barnacle' }, head_repository: { full_name: 'TanoINA/potential-octo-barnacle' }, event: 'push', head_sha: sha, head_branch: 'v0.3.0' };
  const env = { CANDIDATE_RUN_ID: '123', GH_TOKEN: 'fixture-token', RELEASE_SHA: sha, RELEASE_TAG: 'v0.3.0' };
  async function verify(run, ok = true) {
    return check(async (url, options) => {
      assert.equal(url, 'https://api.github.com/repos/TanoINA/potential-octo-barnacle/actions/runs/123');
      assert.equal(options.headers.Authorization, 'Bearer fixture-token');
      return { ok, json: async () => run };
    }, { env });
  }
  await verify(good);
  await verify({ ...good, event: 'workflow_dispatch', head_branch: 'main' });
  for (const patch of [{ head_sha: 'b'.repeat(40) }, { head_sha: undefined }, { head_branch: 'v0.2.0' }, { head_branch: undefined }, { event: 'push', head_branch: 'main' }, { event: 'workflow_dispatch', head_branch: 'feature' }, { event: 'pull_request' }, { conclusion: 'failure' }, { status: 'in_progress' }, { path: '.github/workflows/other.yml' }, { repository: { full_name: 'attacker/repo' } }, { head_repository: { full_name: 'attacker/repo' } }]) await assert.rejects(verify({ ...good, ...patch }), undefined, JSON.stringify(patch));
  await assert.rejects(verify(good, false));
  assert.match(block, /export RELEASE_SHA="\$sha"/, 'Resolved SHA must reach Node in this step, not only future GITHUB_ENV steps');
});
test('signed workflow shell binds manual tag to workflow SHA and checked-out HEAD', t => {
  const block = runBlock(workflowText('signed-release'), '- id: identity');
  const bash = process.platform === 'win32' ? path.join(process.env.ProgramFiles || 'C:\\Program Files', 'Git/bin/bash.exe') : '/bin/bash';
  if (!fs.existsSync(bash)) return t.skip('Bash unavailable; identity shell fixture cannot execute');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'aether-pipeline-identity-'));
  // Only git read queries are stubbed: no repository commits, credentials or network.
  const git = `git() {
    if [[ "$1 $2" == 'rev-parse --verify' && "$3" == refs/tags/* ]]; then printf '%s\\n' "$FIXTURE_TAG_SHA";
    elif [[ "$1 $2" == 'rev-parse --verify' && "$3" == 'HEAD^{commit}' ]]; then printf '%s\\n' "$FIXTURE_HEAD_SHA";
    elif [[ "$1 $2" == 'merge-base --is-ancestor' ]]; then return 0;
    else echo 'Unexpected git invocation' >&2; return 2; fi
  }`;
  function check(overrides = {}) {
    const output = path.join(root, `output-${Math.random()}`);
    const result = spawnSync(bash, ['--noprofile', '--norc', '-c', `${git}\n${block}`], { encoding: 'utf8', env: { ...process.env, RELEASE_TAG: 'v0.3.0', WORKFLOW_SHA: sha, FIXTURE_TAG_SHA: sha, FIXTURE_HEAD_SHA: sha, GITHUB_OUTPUT: output, ...overrides } });
    assert.ifError(result.error);
    return { ...result, output: fs.existsSync(output) ? fs.readFileSync(output, 'utf8') : '' };
  }
  const good = check();
  assert.equal(good.status, 0, good.stderr);
  assert.match(good.output, new RegExp(`sha=${sha}\\ntag=v0.3.0\\nversion=0.3.0`));
  for (const overrides of [{ WORKFLOW_SHA: 'b'.repeat(40) }, { FIXTURE_HEAD_SHA: 'b'.repeat(40) }, { RELEASE_TAG: 'v0.3.0;echo-injection' }]) {
    const bad = check(overrides);
    assert.notEqual(bad.status, 0, JSON.stringify(overrides));
    assert.equal(bad.output, '', 'Rejected identity must not authorize downstream jobs');
  }
  assert.match(workflowText('signed-release'), /WORKFLOW_SHA: \$\{\{ github.sha \}\}/);
});
test('promotion refuses destination symlink ancestors', () => {
  const win = fixture('windows-x86_64'), linux = fixture('linux-x86_64');
  pipeline.createCandidate(win); pipeline.createCandidate(linux);
  const options = { baseUrl, sha, version: '0.3.0', keys: { 'windows-x86_64': key, 'linux-x86_64': key }, verifier: () => {} };
  const out = path.join(win.root, 'prepared');
  pipeline.prepareRelease({ candidates: [win.out, linux.out], out, ...options });
  const real = path.join(win.root, 'real'); fs.mkdirSync(real);
  const linked = path.join(win.root, 'linked'); fs.symlinkSync(real, linked, 'junction');
  assert.throws(() => pipeline.promoteLocal({ source: out, destination: path.join(linked, 'public'), ...options }), /symlink/i);
  assert.equal(fs.readdirSync(real).length, 0);
});