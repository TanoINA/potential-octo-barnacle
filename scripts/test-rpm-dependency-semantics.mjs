import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { inspectLinux } from './release-pipeline.mjs';

assert.equal(process.platform, 'linux', 'Run this isolated RPM test on Linux (or WSL) with cargo and rpm');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rpm-dependency-semantics-'));
console.log(`Isolated fixtures: ${root}`);
const config = JSON.parse(fs.readFileSync(new URL('../src-tauri/tauri.linux.conf.json', import.meta.url), 'utf8'));
const dependencies = config.bundle.linux.rpm.depends;
fs.writeFileSync(path.join(root, 'Cargo.toml'), `[package]
name = "rpm-dependency-semantics"
version = "0.0.0"
edition = "2021"
[dependencies]
rpm = { version = "=0.30.2", default-features = false, features = ["payload"] }
[workspace]
`);
fs.mkdirSync(path.join(root, 'src'));
fs.writeFileSync(path.join(root, 'src/main.rs'), `use rpm::{Dependency, FileOptions, PackageBuilder};
use std::{env, path::Path};
fn package(root: &Path, name: &str, version: &str, requirements: Vec<Dependency>, provides: Vec<Dependency>) -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = PackageBuilder::new(name, version, "MIT", "x86_64", "Isolated RPM dependency fixture");
    builder.release("1");
    for dep in requirements { builder.requires(dep); }
    for dep in provides { builder.provides(dep); }
    builder.with_file_contents(b"fixture\\n", FileOptions::new(format!("/usr/share/{name}/fixture")))?;
    builder.build()?.write_file(root.join(format!("{name}.rpm")))?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let root = Path::new(&args[1]);
    let webkit = "libwebkit2gtk-4.1.so.0()(64bit)";
    let gtk = "libgtk-3.so.0()(64bit)";
    for (name, dep) in [("literal", Dependency::any("glibc >= 2.39")), ("rich", Dependency::any("(glibc >= 2.39)")), ("structured", Dependency::greater_eq("glibc", "2.39"))] {
        package(root, name, "0.3.0", vec![dep, Dependency::any(webkit), Dependency::any(gtk)], vec![])?;
    }
    package(root, "configured", "0.3.0", args[2..].iter().map(Dependency::any).collect(), vec![])?;
    for version in ["2.38", "2.39", "2.40"] {
        package(root, &format!("provider-{version}"), version, vec![], vec![Dependency::eq("glibc", version), Dependency::any(webkit), Dependency::any(gtk)])?;
    }
    Ok(())
}
`);
function command(tool, args) {
  const result = spawnSync(tool, args, { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
  assert.ifError(result.error);
  return result;
}
function success(tool, args) {
  const result = command(tool, args);
  assert.equal(result.status, 0, `${tool} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}
console.log(success('rpm', ['--version']).trim());
console.log(success('cargo', ['--version']).trim());
success('cargo', ['generate-lockfile', '--manifest-path', path.join(root, 'Cargo.toml')]);
assert.match(fs.readFileSync(path.join(root, 'Cargo.lock'), 'utf8'), /name = "rpm"\nversion = "0\.30\.2"/);
success('cargo', ['run', '--locked', '--quiet', '--manifest-path', path.join(root, 'Cargo.toml'), '--', root, ...dependencies]);
const rpm = name => path.join(root, `${name}.rpm`);
const tags = name => success('rpm', ['-qp', '--qf', '[%{REQUIRENAME}|%{REQUIREFLAGS}|%{REQUIREVERSION}\\n]', rpm(name)]).trim();
for (const name of ['literal', 'rich', 'structured', 'configured']) console.log(`${name} REQUIRENAME|REQUIREFLAGS|REQUIREVERSION:\n${tags(name)}`);
assert.ok(tags('literal').split('\n').includes('glibc >= 2.39|0|'));
assert.ok(tags('rich').split('\n').includes('(glibc >= 2.39)|0|'));
assert.ok(tags('structured').split('\n').includes('glibc|12|2.39'));
const db = path.join(root, 'rpmdb');
fs.mkdirSync(db);
success('rpm', ['--dbpath', db, '--initdb']);
// Transaksi hanya diuji dengan basis data terisolasi; tidak ada paket yang dipasang.
for (const name of ['literal', 'rich', 'structured']) {
  for (const version of ['2.38', '2.39', '2.40']) {
    const result = command('rpm', ['--dbpath', db, '--define', '_pkgverify_level digest', '--test', '--install', '--noscripts', rpm(`provider-${version}`), rpm(name)]);
    const expected = name !== 'literal' && version !== '2.38';
    console.log(`${name} with glibc ${version}: exit=${result.status}, expected=${expected ? 'satisfied' : 'unsatisfied'}`);
    assert.equal(result.status === 0, expected, `${result.stdout}\n${result.stderr}`);
    if (!expected) assert.match(result.stderr, /glibc.*2\.39/);
  }
}
assert.equal(success('rpm', ['--dbpath', db, '-qa']).trim(), '', 'Test transactions must not install packages');
assert.throws(() => inspectLinux(rpm('literal'), 'rpm', '0.3.0'), /RPM.*(glibc|dependency)/i);
for (const name of ['rich', 'structured', 'configured']) {
  const metadata = inspectLinux(rpm(name), 'rpm', '0.3.0');
  assert.equal(metadata.version, '0.3.0');
}
assert.ok(tags('configured').split('\n').includes('(glibc >= 2.39)|0|'));
console.log('PASS: pinned rpm 0.30.2 metadata, isolated RPM resolver transactions, config and candidate semantic gate');
