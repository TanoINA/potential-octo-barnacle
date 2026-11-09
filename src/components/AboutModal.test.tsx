import { act, render, screen, fireEvent, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AboutModal } from './AboutModal';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
type Progress = { phase: 'downloading' | 'verifying' | 'installing'; downloaded: number; total?: number };
type ProgressHandler = (event: { payload: Progress }) => void;
const events = vi.hoisted(() => ({ progress: undefined as ProgressHandler | undefined, unlisten: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
beforeEach(() => {
  vi.mocked(invoke).mockReset();
  events.progress = undefined;
  events.unlisten.mockReset();
  vi.mocked(listen).mockReset().mockImplementation(async (name, handler) => {
    if (name === 'release:update-progress') events.progress = handler as ProgressHandler;
    return events.unlisten;
  });
});
it('shows actual release metadata, disabled unconfigured updates and authorized diagnostic opener', async () => {
  vi.mocked(invoke).mockResolvedValue({ version: '0.3.0', buildId: '20261108T120000Z-abcdef0-dirty', platform: 'windows', arch: 'x86_64', diagnosticLogsPath: 'D:/logs/tasks', signingStatus: 'Unknown (not verified at runtime)', updatesConfigured: false, updateStatus: 'Automatic updates not configured' });
  render(<AboutModal onClose={vi.fn()} />);
  expect(await screen.findByText('0.3.0')).toBeTruthy();
  expect(screen.getByText('20261108T120000Z-abcdef0-dirty')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Check for updates' }).hasAttribute('disabled')).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Open diagnostic logs' }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('open_path_or_url', { pathOrUrl: 'D:/logs/tasks' }));
});
it('never installs merely by checking and requires explicit confirmation', async () => {
  vi.mocked(invoke).mockImplementation(async (command) => command === 'get_release_info' ? { version: '0.3.0', buildId: 'build', platform: 'windows', arch: 'x86_64', diagnosticLogsPath: 'D:/logs', signingStatus: 'Unknown', updatesConfigured: true, updateStatus: 'Configured' } : { version: '0.3.1', status: 'Update available' });
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install signed update 0.3.1' }));
  expect(confirm).toHaveBeenCalledOnce();
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'install_release_update')).toBe(false);
});
it('surfaces asynchronous native metadata rejection and retries without fabricated release configuration', async () => {
  let rejectLoad!: (reason: Error) => void;
  vi.mocked(invoke).mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectLoad = reject; }));
  render(<AboutModal onClose={vi.fn()} />);
  rejectLoad(new Error('Native release metadata unavailable'));
  expect((await screen.findByRole('alert')).textContent).toContain('Native release metadata unavailable');
  expect(screen.queryByRole('button', { name: 'Check for updates' })).toBeNull();
  expect(screen.queryByText('0.3.0')).toBeNull();
  vi.mocked(invoke).mockResolvedValueOnce({ version: '0.3.2', buildId: 'native-build', platform: 'windows', arch: 'x86_64', diagnosticLogsPath: 'D:/real-logs', signingStatus: 'Unknown', updatesConfigured: false, updateStatus: 'Not configured' });
  fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  expect(await screen.findByText('native-build')).toBeTruthy();
  expect(screen.queryByRole('alert')).toBeNull();
  expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual(['get_release_info', 'get_release_info']);
});


const configuredInfo = { version: '0.3.0', buildId: 'build', platform: 'windows', arch: 'x86_64', diagnosticLogsPath: 'D:/logs', signingStatus: 'Unknown', updatesConfigured: true, updateStatus: 'Configured' };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
it('waits for readiness and fails closed on a flush error without invoking the installer', async () => {
  const readiness = deferred<void>();
  const onBeforeInstall = vi.fn(() => readiness.promise);
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo).mockResolvedValueOnce({ version: '0.3.1', status: 'Available' });
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} onBeforeInstall={onBeforeInstall} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  const install = await screen.findByRole('button', { name: 'Install signed update 0.3.1' });
  fireEvent.click(install);
  fireEvent.click(install);
  expect(onBeforeInstall).toHaveBeenCalledOnce();
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'install_release_update')).toBe(false);
  await act(async () => readiness.reject(new Error('Could not flush session: disk full')));
  expect(screen.getByRole('alert').textContent).toContain('disk full');
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'install_release_update')).toBe(false);
  expect(install.hasAttribute('disabled')).toBe(false);
  onBeforeInstall.mockResolvedValueOnce();
  fireEvent.click(install);
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('install_release_update', { expectedVersion: '0.3.1', confirmed: true }));
});
it('shows native install status and requires fresh confirmation and readiness for Linux restart', async () => {
  const onBeforeInstall = vi.fn().mockResolvedValue(undefined);
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValueOnce({ ...configuredInfo, platform: 'linux' })
    .mockResolvedValueOnce({ version: '0.3.1', status: 'Available' })
    .mockResolvedValueOnce({ status: 'Signed AppImage installed', restartRequired: true });
  render(<AboutModal onClose={vi.fn()} onBeforeInstall={onBeforeInstall} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install signed update 0.3.1' }));
  expect((await screen.findByRole('status')).textContent).toContain('Signed AppImage installed');
  expect(screen.queryByRole('button', { name: /Install signed update/ })).toBeNull();
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'restart_after_update')).toBe(false);
  const restart = screen.getByRole('button', { name: 'Restart to finish update' });
  confirm.mockReturnValueOnce(false);
  fireEvent.click(restart);
  expect(onBeforeInstall).toHaveBeenCalledTimes(1);
  onBeforeInstall.mockRejectedValueOnce(new Error('Background session still saving'));
  fireEvent.click(restart);
  expect((await screen.findByRole('alert')).textContent).toContain('still saving');
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'restart_after_update')).toBe(false);
  fireEvent.click(restart);
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('restart_after_update', { confirmed: true }));
  expect(onBeforeInstall).toHaveBeenCalledTimes(3);
});
it('renders bounded release notes and native progress only during its own install', async () => {
  const pending = deferred<{ status: string; restartRequired: boolean }>();
  const notes = '<script>not executable</script>\n' + 'n'.repeat(20000);
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo)
    .mockResolvedValueOnce({ version: '0.3.1', status: 'Available'.repeat(1000), notes })
    .mockImplementationOnce(() => pending.promise);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  const install = await screen.findByRole('button', { name: 'Install signed update 0.3.1' });
  const releaseNotes = screen.getByRole('region', { name: 'Release notes' });
  expect(releaseNotes.textContent!.length).toBeLessThanOrEqual(12001);
  expect(releaseNotes.querySelector('script')).toBeNull();
  expect(releaseNotes.textContent).toBe(notes.slice(0, 12000) + '…');
  expect(screen.getByRole('status').textContent!.length).toBeLessThanOrEqual(1001);
  fireEvent.click(install);
  await waitFor(() => expect(listen).toHaveBeenCalledWith('release:update-progress', expect.any(Function)));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('install_release_update', { expectedVersion: '0.3.1', confirmed: true }));
  act(() => events.progress!({ payload: { phase: 'downloading', downloaded: 25, total: 100 } }));
  expect(screen.getByRole('status').textContent).toContain('Downloading');
  expect(screen.getByRole('progressbar').getAttribute('value')).toBe('25');
  for (const payload of [
    { phase: 'downloading' as const, downloaded: 10 },
    { phase: 'downloading' as const, downloaded: 10, total: 0 },
    { phase: 'downloading' as const, downloaded: 10, total: -100 },
    { phase: 'downloading' as const, downloaded: 10, total: Infinity },
    { phase: 'downloading' as const, downloaded: 10, total: NaN },
    { phase: 'downloading' as const, downloaded: Infinity, total: 100 },
    { phase: 'downloading' as const, downloaded: NaN, total: 100 },
  ]) {
    act(() => events.progress!({ payload }));
    expect(screen.getByRole('progressbar').hasAttribute('value')).toBe(false);
  }
  act(() => events.progress!({ payload: { phase: 'downloading', downloaded: 250, total: 100 } }));
  expect(screen.getByRole('progressbar').getAttribute('value')).toBe('100');
  act(() => events.progress!({ payload: { phase: 'downloading', downloaded: -5, total: 100 } }));
  expect(screen.getByRole('progressbar').getAttribute('value')).toBe('0');
  act(() => events.progress!({ payload: { phase: 'verifying', downloaded: 100 } }));
  expect(screen.getByRole('status').textContent).toContain('Verifying');
  expect(screen.getByRole('progressbar').hasAttribute('value')).toBe(false);
  act(() => events.progress!({ payload: { phase: 'installing', downloaded: 100 } }));
  expect(screen.getByRole('status').textContent).toContain('Installing');
  const stale = events.progress!;
  await act(async () => pending.resolve({ status: 'Signed installer launched', restartRequired: false }));
  expect(screen.getByRole('status').textContent).toContain('Signed installer launched');
  expect(events.unlisten).toHaveBeenCalledOnce();
  act(() => stale({ payload: { phase: 'downloading', downloaded: 1, total: 100 } }));
  expect(screen.getByRole('status').textContent).toContain('Signed installer launched');
  expect(screen.queryByRole('progressbar')).toBeNull();
});
it.each(['button', 'Escape', 'unmount'])('releases delayed progress registration after %s without starting the installer', async (method) => {
  const registration = deferred<() => void>();
  const onClose = vi.fn();
  vi.mocked(listen).mockImplementationOnce((_name, handler) => {
    events.progress = handler as ProgressHandler;
    return registration.promise;
  });
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo).mockResolvedValueOnce({ version: '0.3.1', status: 'Available' });
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  const modal = render(<AboutModal onClose={onClose} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install signed update 0.3.1' }));
  expect(listen).toHaveBeenCalledOnce();
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'install_release_update')).toBe(false);
  act(() => events.progress!({ payload: { phase: 'downloading', downloaded: 50, total: 100 } }));
  expect(screen.queryByRole('progressbar')).toBeNull();
  if (method === 'unmount') modal.unmount();
  else if (method === 'Escape') fireEvent.keyDown(document, { key: 'Escape' });
  else fireEvent.click(screen.getByRole('button', { name: 'Close About' }));
  await act(async () => registration.resolve(events.unlisten));
  expect(events.unlisten).toHaveBeenCalledOnce();
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'install_release_update')).toBe(false);
  act(() => events.progress!({ payload: { phase: 'installing', downloaded: 100 } }));
  expect(screen.queryByRole('progressbar')).toBeNull();
  if (method !== 'unmount') {
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.getByRole('status').textContent).toBe('Available');
    modal.unmount();
    expect(events.unlisten).toHaveBeenCalledOnce();
  }
});
it('cleans an active progress listener immediately on unmount', async () => {
  const pending = deferred<{ status: string; restartRequired: boolean }>();
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo)
    .mockResolvedValueOnce({ version: '0.3.1', status: 'Available' })
    .mockImplementationOnce(() => pending.promise);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  const modal = render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install signed update 0.3.1' }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('install_release_update', { expectedVersion: '0.3.1', confirmed: true }));
  const stale = events.progress!;
  act(() => stale({ payload: { phase: 'downloading', downloaded: 25, total: 100 } }));
  modal.unmount();
  expect(events.unlisten).toHaveBeenCalledOnce();
  act(() => stale({ payload: { phase: 'installing', downloaded: 100 } }));
  await act(async () => pending.resolve({ status: 'Late installer status', restartRequired: true }));
  expect(screen.queryByText('Late installer status')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Restart to finish update' })).toBeNull();
  expect(events.unlisten).toHaveBeenCalledOnce();
});
it.each(['button', 'Escape'])('keeps the modal barrier during native installation on %s until failure', async (method) => {
  const pending = deferred<void>();
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo)
    .mockResolvedValueOnce({ version: '0.3.1', status: 'Available' }).mockImplementationOnce(() => pending.promise);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  const onClose = vi.fn();
  render(<AboutModal onClose={onClose} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install signed update 0.3.1' }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('install_release_update', expect.anything()));
  if (method === 'button') fireEvent.click(screen.getByRole('button', { name: 'Close About' }));
  else fireEvent.keyDown(document, { key: 'Escape' });
  expect(onClose).not.toHaveBeenCalled();
  expect(events.unlisten).not.toHaveBeenCalled();
  await act(async () => pending.reject(new Error('Download cancelled by network failure')));
  expect(screen.getByRole('alert').textContent).toContain('network failure');
  fireEvent.click(screen.getByRole('button', { name: 'Close About' }));
  expect(onClose).toHaveBeenCalledOnce();
});

it('fails closed on progress registration failure and permits a fresh install retry', async () => {
  vi.mocked(listen).mockRejectedValueOnce(new Error('Progress registration failed'));
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo)
    .mockResolvedValueOnce({ version: '0.3.1', status: 'Available' })
    .mockResolvedValueOnce({ status: 'Installer launched', restartRequired: false });
  const onBeforeInstall = vi.fn().mockResolvedValue(undefined);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} onBeforeInstall={onBeforeInstall} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  const install = await screen.findByRole('button', { name: 'Install signed update 0.3.1' });
  fireEvent.click(install);
  expect((await screen.findByRole('alert')).textContent).toContain('Progress registration failed');
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'install_release_update')).toBe(false);
  expect(install.hasAttribute('disabled')).toBe(false);
  fireEvent.click(install);
  await waitFor(() => expect(screen.getByRole('status').textContent).toBe('Installer launched'));
  expect(onBeforeInstall).toHaveBeenCalledTimes(2);
  expect(listen).toHaveBeenCalledTimes(2);
  expect(events.unlisten).toHaveBeenCalledOnce();
  expect(screen.queryByRole('alert')).toBeNull();
});
it('cleans failed install progress and ignores stale events on retry', async () => {
  const first = deferred<void>();
  const retry = deferred<{ status: string; restartRequired: boolean }>();
  const firstUnlisten = vi.fn();
  vi.mocked(listen).mockImplementationOnce(async (_name, handler) => {
    events.progress = handler as ProgressHandler;
    return firstUnlisten;
  });
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo)
    .mockResolvedValueOnce({ version: '0.3.1', status: 'Available' })
    .mockImplementationOnce(() => first.promise)
    .mockImplementationOnce(() => retry.promise);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  const install = await screen.findByRole('button', { name: 'Install signed update 0.3.1' });
  fireEvent.click(install);
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('install_release_update', { expectedVersion: '0.3.1', confirmed: true }));
  const stale = events.progress!;
  act(() => stale({ payload: { phase: 'downloading', downloaded: 20, total: 100 } }));
  await act(async () => first.reject(new Error('Installer failed')));
  expect(firstUnlisten).toHaveBeenCalledOnce();
  expect(screen.queryByRole('progressbar')).toBeNull();
  expect(screen.getByRole('alert').textContent).toContain('Installer failed');
  fireEvent.click(install);
  await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'install_release_update')).toHaveLength(2));
  act(() => stale({ payload: { phase: 'installing', downloaded: 100 } }));
  expect(screen.getByRole('status').textContent).toBe('Available');
  expect(screen.queryByRole('progressbar')).toBeNull();
  act(() => events.progress!({ payload: { phase: 'downloading', downloaded: 70, total: 100 } }));
  expect(screen.getByRole('progressbar').getAttribute('value')).toBe('70');
  await act(async () => retry.resolve({ status: 'Installer launched', restartRequired: false }));
  expect(events.unlisten).toHaveBeenCalledOnce();
  expect(screen.queryByRole('alert')).toBeNull();
  expect(screen.queryByRole('progressbar')).toBeNull();
});
it('invalidates the previous update while rechecking and after a check error', async () => {
  const pending = deferred<{ version: string; status: string }>();
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo).mockResolvedValueOnce({ version: '0.3.1', status: 'Available' }).mockImplementationOnce(() => pending.promise);
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  await screen.findByRole('button', { name: 'Install signed update 0.3.1' });
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  expect(screen.getByRole('button', { name: 'Check for updates' }).hasAttribute('disabled')).toBe(true);
  expect(screen.queryByRole('button', { name: 'Install signed update 0.3.1' })).toBeNull();
  await act(async () => pending.reject(new Error('Check failed')));
  expect(screen.getByRole('alert').textContent).toContain('Check failed');
  expect(screen.queryByRole('button', { name: /Install signed update/ })).toBeNull();
  expect(screen.getByRole('button', { name: 'Check for updates' }).hasAttribute('disabled')).toBe(false);
});
it.each(['button', 'Escape'])('ignores pending check completion after closing with %s', async (method) => {
  const pending = deferred<{ version: string; status: string }>();
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo).mockImplementationOnce(() => pending.promise);
  const onClose = vi.fn();
  render(<AboutModal onClose={onClose} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  if (method === 'button') fireEvent.click(screen.getByRole('button', { name: 'Close About' }));
  else fireEvent.keyDown(document, { key: 'Escape' });
  expect(onClose).toHaveBeenCalledOnce();
  await act(async () => pending.resolve({ version: '0.3.1', status: 'Available' }));
  expect(screen.queryByRole('button', { name: /Install signed update/ })).toBeNull();
});
it.each(['resolve', 'reject'])('ignores late metadata %s after switching away and reopening', async (outcome) => {
  const pending = deferred<typeof configuredInfo>();
  vi.mocked(invoke).mockImplementationOnce(() => pending.promise).mockResolvedValueOnce({ ...configuredInfo, buildId: 'new-build' });
  const old = render(<AboutModal onClose={vi.fn()} />);
  old.unmount();
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('new-build');
  await act(async () => {
    if (outcome === 'resolve') pending.resolve({ ...configuredInfo, buildId: 'stale-build' });
    else pending.reject(new Error('Stale load failed'));
  });
  expect(screen.getByText('new-build')).toBeTruthy();
  expect(screen.queryByText('stale-build')).toBeNull();
  expect(screen.queryByRole('alert')).toBeNull();
});
it('surfaces confirmed install failure and permits a fresh update check', async () => {
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo).mockResolvedValueOnce({ version: '0.3.1', status: 'Available' }).mockRejectedValueOnce(new Error('Installer failed'));
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install signed update 0.3.1' }));
  expect((await screen.findByRole('alert')).textContent).toContain('Installer failed');
  expect(invoke).toHaveBeenCalledWith('install_release_update', { expectedVersion: '0.3.1', confirmed: true });
  expect(screen.getByRole('button', { name: 'Check for updates' }).hasAttribute('disabled')).toBe(false);
});
it('starts only the explicitly confirmed installer and clears its install action on success', async () => {
  const pending = deferred<void>();
  vi.mocked(invoke).mockResolvedValueOnce(configuredInfo).mockResolvedValueOnce({ version: '0.3.1', status: 'Available' }).mockImplementationOnce(() => pending.promise);
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} />);
  await screen.findByText('0.3.0');
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  const install = await screen.findByRole('button', { name: 'Install signed update 0.3.1' });
  fireEvent.click(install);
  fireEvent.click(install);
  expect(confirm).toHaveBeenCalledExactlyOnceWith('Install signed update 0.3.1? AetherAgent may close. Save your work first.');
  await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'install_release_update')).toHaveLength(1));
  expect(screen.getByRole('button', { name: 'Check for updates' }).hasAttribute('disabled')).toBe(true);
  await act(async () => pending.resolve());
  expect(screen.getByRole('status').textContent).toContain('Installer started');
  expect(screen.queryByRole('button', { name: /Install signed update/ })).toBeNull();
});
it.each(['get_release_info', 'check_release_update'])('consumes late %s rejection after Close without changing the closed view', async (command) => {
  const pending = deferred<unknown>();
  vi.mocked(invoke).mockImplementation(async (name) => {
    if (name === command) return pending.promise;
    if (name === 'get_release_info') return configuredInfo;
    return { version: '0.3.1', status: 'Available' };
  });
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  render(<AboutModal onClose={vi.fn()} />);
  if (command !== 'get_release_info') {
    await screen.findByText('0.3.0');
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  }
  fireEvent.click(screen.getByRole('button', { name: 'Close About' }));
  await act(async () => pending.reject(new Error('Closed operation failed')));
  expect(screen.queryByRole('alert')).toBeNull();
});