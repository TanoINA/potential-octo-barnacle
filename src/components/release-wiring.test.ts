import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
it('wires release IPC and compile-time build identity without mutating process environment', () => {
  const native = readFileSync('src-tauri/src/lib.rs', 'utf8');
  expect(native).toContain('agent::release::get_release_info,');
  expect(native).toContain('agent::release::install_release_update,');
  const build = readFileSync('src-tauri/build.rs', 'utf8');
  expect(build).toContain('cargo:rustc-env=AETHER_BUILD_ID=');
  expect(build).not.toContain('set_var');
});
it('validates public updater values before embedding them', () => {
  const build = readFileSync('src-tauri/build.rs', 'utf8');
  expect(build).toContain('release_policy::validate_url');
  expect(build).toContain('release_policy::validate_key');
});
