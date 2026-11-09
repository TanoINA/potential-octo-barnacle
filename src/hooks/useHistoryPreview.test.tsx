import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { AgentSettings, SessionData, SessionPage } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
  mocks.listeners.set(name, handler);
  return () => mocks.listeners.delete(name);
}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));
const provider = { id: 'p', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: 'p', systemPrompt: '', temperature: 0.7, streamResponse: true,
  thinking: false, reasoningEffort: 'medium', favoriteModels: [], skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'one', title: 'Original', createdAt: 1, updatedAt: 2, messageCount: 102 },
  messages: [{ id: 'u100', role: 'user', content: 'Prompt', timestamp: 100 }, { id: 'a101', role: 'assistant', content: 'Answer', timestamp: 101 }],
  artifacts: [],
};
const window = { offset: 100, total: 102, revision: 'r1', firstMessageId: 'u100' };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}
beforeEach(() => {
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session_page') return { data: { ...structuredClone(initial), session: { ...initial.session, id: args.sessionId } }, window: { ...window } };
    if (command === 'save_session_page' || command === 'checkpoint_session_page') return { ...args.window, revision: 'r2', total: args.window.offset + args.data.messages.length };
    if (command === 'load_session') return structuredClone(initial);
    return undefined;
  });
});
afterEach(cleanup);


const around: SessionPage = { data: { ...initial, messages: [{ id: 'old', role: 'user', content: 'Old prompt', timestamp: 1 }] }, window: { offset: 10, total: 102, end: 11, revision: 'r1', firstMessageId: 'old' } };
it('keeps preview actions stable and composing disabled throughout pending reads', async () => {
  const hook = await mounted();
  const actions = ['sendMessage', 'undoTurn', 'retryTurn', 'editAndResendMessage', 'rollbackToMessage', 'reviewSession', 'compactSession', 'selectCouncilWinner', 'loadOlderMessages'] as const;
  const before = actions.map((name) => hook.result.current[name]);
  const pending = deferred<SessionPage>();
  const backend = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command: string, args: any) => command === 'load_session_around' ? pending.promise : backend(command, args));
  let opening!: Promise<boolean>;
  await act(async () => { opening = hook.result.current.openMessage('one', 'old'); });
  expect(hook.result.current.isHistoryPreview).toBe(true);
  await act(async () => {
    expect(await hook.result.current.sendMessage('blocked')).toBe(false);
    expect(await hook.result.current.undoTurn()).toBeNull();
    expect(await hook.result.current.retryTurn('old')).toBe(false);
    expect(await hook.result.current.rollbackToMessage('old')).toBe(false);
    expect(await hook.result.current.editAndResendMessage('old', 'changed')).toBe(false);
    expect(await hook.result.current.reviewSession()).toBe(false);
    await hook.result.current.compactSession();
    hook.result.current.clearCurrentSession();
  });
  expect(mocks.invoke.mock.calls.filter(([name]) => ['send_message','save_session','save_session_page','load_session'].includes(name))).toHaveLength(0);
  await act(async () => { pending.resolve(around); expect(await opening).toBe(true); });
  actions.forEach((name, i) => expect(hook.result.current[name], name).toBe(before[i]));
  act(() => hook.result.current.loadLatestMessages());
  expect(hook.result.current.messages.map(m => [m.id,m.content])).toEqual(initial.messages.map(m => [m.id,m.content]));
});
it.each(['resolve','reject'] as const)('discards a late preview %s after navigation', async (outcome) => {
  const hook = await mounted(); const pending = deferred<SessionPage>();
  const backend = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command: string, args: any) => command === 'load_session_around' ? pending.promise : backend(command,args));
  let opening!: Promise<boolean>;
  await act(async () => { opening = hook.result.current.openMessage('one','old'); });
  await act(async () => { await hook.result.current.switchSession('two'); });
  await act(async () => { if(outcome === 'resolve') pending.resolve(around); else pending.reject(new Error('late')); expect(await opening).toBe(false); });
  expect(hook.result.current.currentSessionId).toBe('two'); expect(hook.result.current.isHistoryPreview).toBe(false); expect(hook.result.current.historyError).toBeNull();
});
it('rejects a superseded open before its session switch continuation runs', async () => {
  const hook = await mounted(); let opening!: Promise<boolean>;
  await act(async () => { opening = hook.result.current.openMessage('one','old'); await hook.result.current.switchSession('one'); expect(await opening).toBe(false); });
  expect(mocks.invoke.mock.calls.some(([name])=>name === 'load_session_around')).toBe(false);
});
it('keeps the live stream separate while displaying a detached preview', async () => {
  const hook = await mounted(); await act(async () => { await hook.result.current.sendMessage('live'); });
  const live = hook.result.current.messages.at(-1)!;
  const backend = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command: string,args: any) => command === 'load_session_around' ? Promise.resolve(around) : backend(command,args));
  await act(async () => { expect(await hook.result.current.openMessage('one','old')).toBe(true); });
  act(() => mocks.listeners.get('agent-event')!({ payload: { type:'chunk', data:{ sessionId:'one', runId:live.runId, text:'stream survived' } } }));
  expect(hook.result.current.messages.map(m=>m.id)).toEqual(['old']);
  act(() => hook.result.current.loadLatestMessages());
  await waitFor(() => expect(hook.result.current.messages.at(-1)!.content).toBe('stream survived'));
});

it('drops a late older preview page after returning to live history', async () => {
 const hook=await mounted(); const pending=deferred<SessionPage>(); const backend=mocks.invoke.getMockImplementation()!;
 mocks.invoke.mockImplementation((command:string,args:any)=>command==='load_session_around'?Promise.resolve(around):command==='load_session_page' && args.before!==undefined?pending.promise:backend(command,args));
 await act(async()=>{await hook.result.current.openMessage('one','old');});
 let loading!:Promise<void>; act(()=>{loading=hook.result.current.loadOlderMessages();});
 act(()=>hook.result.current.loadLatestMessages());
 await act(async()=>{pending.resolve({...around,data:{...around.data,messages:[{id:'older',role:'user',content:'older',timestamp:0}]},window:{...around.window,offset:9,firstMessageId:'older'}}); await loading;});
 expect(hook.result.current.isHistoryPreview).toBe(false);expect(hook.result.current.messages.map(m=>m.id)).toEqual(['u100','a101']);
});

it('blocks synchronous mutations immediately when an exact-message navigation begins', async () => {
 const hook=await mounted(); const pending=deferred<SessionPage>(); const backend=mocks.invoke.getMockImplementation()!;
 mocks.invoke.mockImplementation((command:string,args:any)=>command==='load_session_around'?pending.promise:backend(command,args));
 let opening!:Promise<boolean>; let sending!:Promise<boolean>;
 act(()=>{opening=hook.result.current.openMessage('one','old'); sending=hook.result.current.sendMessage('must not leak');});
 await act(async()=>{expect(await sending).toBe(false);});
 await act(async()=>{pending.resolve(around);await opening;});
});
