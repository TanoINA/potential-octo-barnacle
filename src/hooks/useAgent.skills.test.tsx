import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { pagingBackend } from '../test/sessionPaging';
import type { AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

const provider = { id: 'provider', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  reviewConfig: { createDir: 'custom-created-skills' },
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'skill-session', title: 'Skills', createdAt: 1, updatedAt: 1, messageCount: 0 },
  messages: [], artifacts: [],
};
let stored: SessionData;
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name);
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}
beforeEach(() => {
  stored = structuredClone(initial);
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (command: string, args: { data: SessionData }) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [stored.session];
    if (command === 'load_session') return structuredClone(stored);
    if (command === 'prepare_image_attachments') return [{ sourcePath: '/work/image.png', mediaType: 'image/png', data: 'cG5n' }];
    if (command === 'save_session' || command === 'checkpoint_session') stored = structuredClone(args.data);
    return undefined;
  }));
});
afterEach(cleanup);

describe('selected skill turn metadata', () => {
  it.each([false, true])('saves normalized skills and sends clean, exact text (Council: %s)', async (council) => {
    const hook = await mounted();
    const content = '  Keep **my text** exactly.\n\nDo not wrap it.  \n';
    const selectedSkills = [' Systematic-Debugging ', 'TEST-DRIVEN-DEVELOPMENT', 'systematic-debugging'];
    await act(async () => {
      expect(await hook.result.current.sendMessage(content, undefined, {
        council, candidateA: { provider }, candidateB: { provider }, selectedSkills,
      })).toBe(true);
    });
    const expectedSkills = ['systematic-debugging', 'test-driven-development'];
    expect(hook.result.current.messages[0].selectedSkills).toEqual(expectedSkills);
    expect(hook.result.current.messages[0].content).toBe(content);
    expect(stored.messages[0]).toMatchObject({ content, selectedSkills: expectedSkills });
    const requests = council
      ? [calls('send_council_message')[0][1].requestA, calls('send_council_message')[0][1].requestB]
      : [calls('send_message')[0][1].request];
    for (const request of requests) {
      expect(request.messages[0]).toMatchObject({ content, selectedSkills: expectedSkills });
      expect(request.systemPrompt).toBeUndefined();
      expect(request.reviewConfig).toEqual(settings.reviewConfig);
    }
    expect(hook.result.current.messages[1]).not.toHaveProperty('selectedSkills');
    expect(selectedSkills).toEqual([' Systematic-Debugging ', 'TEST-DRIVEN-DEVELOPMENT', 'systematic-debugging']);
  });
});

it('does not turn a skill-scoped review instruction into a local review operation', async () => {
  const hook = await mounted();
  await act(async () => { expect(await hook.result.current.sendMessage('/review', undefined, { selectedSkills: ['ponytail'] })).toBe(true); });
  expect(calls('review_session')).toHaveLength(0);
  expect(calls('send_message')[0][1].request.messages.at(-1)).toMatchObject({ content: '/review', selectedSkills: ['ponytail'] });
});

it('transforms /goal into specialized directive prompt', async () => {
  const hook = await mounted();
  await act(async () => {
    expect(await hook.result.current.sendMessage('/goal build passing tests')).toBe(true);
  });
  const sentGoal = calls('send_message')[0][1].request.messages.at(-1).content;
  expect(sentGoal).toContain('autonomous /goal mode');
  expect(sentGoal).toContain('build passing tests');
});

it('transforms /grill-me into specialized directive prompt', async () => {
  const hook = await mounted();
  await act(async () => {
    expect(await hook.result.current.sendMessage('/grill-me auth flow')).toBe(true);
  });
  const sentGrill = calls('send_message')[0][1].request.messages.at(-1).content;
  expect(sentGrill).toContain('reverse-interview mode (/grill-me)');
  expect(sentGrill).toContain('auth flow');
});

describe('selected skill replay', () => {
  it.each([
    ['retry user', true], ['retry assistant', true], ['edit', true], ['undo', true],
    ['retry user', false], ['retry assistant', false], ['edit', false], ['undo', false],
  ] as const)('preserves turn metadata for %s (selected: %s)', async (operation, selected) => {
    const content = '  Original prompt.\n\n';
    const edited = '  Edited prompt.\n';
    const selectedSkills = ['systematic-debugging', 'test-driven-development'];
    const attachment = { id: 'image', path: '/work/image.png', name: 'image.png', isDir: false };
    const imageInputs = [{ sourcePath: attachment.path, mediaType: 'image/png', data: 'cG5n' }];
    stored.messages = [
      { id: 'legacy', role: 'user', content: 'Legacy history', timestamp: 1 },
      { id: 'legacy-answer', role: 'assistant', content: 'Old answer', timestamp: 2 },
      {
        id: 'user', role: 'user', content, timestamp: 3, attachments: [attachment], imageInputs,
        ...(selected ? { selectedSkills } : {}),
      },
      { id: 'answer', role: 'assistant', content: 'Answer', timestamp: 4 },
    ];
    const hook = await mounted();
    let legacy = hook.result.current.messages[0];
    await act(async () => {
      if (operation === 'undo') {
        const undone = await hook.result.current.undoTurn();
        expect(undone).toMatchObject({ content, attachments: [attachment], imageInputs });
        if (selected) expect(undone?.selectedSkills).toEqual(selectedSkills);
        else expect(undone).not.toHaveProperty('selectedSkills');
        expect(stored.messages).toHaveLength(2);
        legacy = hook.result.current.messages[0];
        expect(await hook.result.current.sendMessage(undone!.content, undone!.attachments, {
          imageInputs: undone!.imageInputs, selectedSkills: undone!.selectedSkills,
        })).toBe(true);
      } else if (operation === 'edit') {
        expect(await hook.result.current.editAndResendMessage('user', edited)).toBe(true);
      } else {
        expect(await hook.result.current.retryTurn(operation === 'retry user' ? 'user' : 'answer')).toBe(true);
      }
    });
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    const user = hook.result.current.messages[2];
    expect(user.content).toBe(operation === 'edit' ? edited : content);
    expect(user.imageInputs).toEqual(imageInputs);
    expect(hook.result.current.messages[0]).toBe(legacy);
    const request = calls('send_message').at(-1)![1].request;
    for (const message of [user, stored.messages[2], request.messages[2]]) {
      if (selected) expect(message.selectedSkills).toEqual(selectedSkills);
      else expect(message).not.toHaveProperty('selectedSkills');
    }
    for (const message of [hook.result.current.messages[0], stored.messages[0], request.messages[0]]) {
      expect(message).not.toHaveProperty('selectedSkills');
      expect(message.content).toBe('Legacy history');
    }
    expect(request.messages[2].content).toBe(`${operation === 'edit' ? edited : content}\n\n## Attached context:\n- [File] \`/work/image.png\``);
  });
});

it('binds skill discovery to the visible session workspace', async () => {
  stored.session.workspaceCwd = '/session-workspace';
  const hook = await mounted();
  await act(async () => { await hook.result.current.listSkills(); });
  expect(calls('list_skills').at(-1)).toEqual(['list_skills', { paths: undefined, expectedWorkspace: '/session-workspace' }]);
});

describe('selected skill preflight', () => {
  const malformed: unknown[] = [
    null, 'debugging', {}, [null], [42], [undefined], new Array(1),
    [''], [' \t '], ['../debugging'], ['C:\\skills\\SKILL.md'], ['foo/bar'],
    ['foo_bar'], ['two words'], ['foo\nbar'], ['-foo'], ['foo-'], ['foo--bar'],
    ['é'], ['K'], ['a'.repeat(65)], ['CON'], ['prn'], ['aux'], ['nul'], ['com1'], ['lpt9'],
    ['one', 'two', 'three', 'four', 'five', 'six'], Array(6).fill('duplicate'),
  ];
  it.each([false, true])('rejects malformed or over-limit selections before any mutation (Council: %s)', async (council) => {
    stored.messages = [{ id: 'old', role: 'user', content: 'Unchanged history', timestamp: 1 }];
    const hook = await mounted();
    const history = hook.result.current.messages;
    const snapshot = structuredClone(history);
    const sessionId = hook.result.current.currentSessionId;
    let draft = '  Keep this draft.\n';
    const image = { id: 'image', name: 'image.png', path: '/work/image.png', isDir: false };
    for (const selectedSkills of malformed) {
      let accepted!: boolean;
      await act(async () => {
        accepted = await hook.result.current.sendMessage(draft, [image], {
          council, candidateA: { provider }, candidateB: { provider }, selectedSkills: selectedSkills as string[],
        });
        if (accepted) draft = '';
      });
      expect(accepted, JSON.stringify(selectedSkills)).toBe(false);
      expect(draft).toBe('  Keep this draft.\n');
      expect(hook.result.current.lastError).toMatch(/skill|five/i);
      expect(hook.result.current.messages).toBe(history);
      expect(hook.result.current.messages).toEqual(snapshot);
      expect(hook.result.current.currentSessionId).toBe(sessionId);
      expect(hook.result.current.sessionTitle).toBe('Skills');
      expect(hook.result.current.isStreaming).toBe(false);
      expect(hook.result.current.runStatus).toBeNull();
      for (const command of ['save_session', 'save_session_page', 'prepare_image_attachments', 'send_message', 'send_council_message']) {
        expect(calls(command), command).toHaveLength(0);
      }
    }
    await act(async () => { expect(await hook.result.current.sendMessage(draft)).toBe(true); });
  });

  it('accepts five valid names, including the 64-character boundary and nonreserved device-like slugs', async () => {
    const hook = await mounted();
    const selectedSkills = ['a'.repeat(64), 'COM0', 'lpt10', 'con-helper', '123'];
    await act(async () => { expect(await hook.result.current.sendMessage('Five skills', undefined, { selectedSkills })).toBe(true); });
    expect(stored.messages[0].selectedSkills).toEqual(selectedSkills.map((name) => name.toLowerCase()));
  });
});
