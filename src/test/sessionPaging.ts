import type { SessionData, SessionPage, SessionWindow } from '../types/agent';

export function page(data: SessionData, window: Partial<SessionWindow> = {}): SessionPage {
  return {
    data,
    window: {
      offset: 0,
      total: data.messages.length,
      revision: 'test-revision',
      firstMessageId: data.messages[0]?.id ?? null,
      ...window,
    },
  };
}

/** Adapt full-session fixtures while preserving deferred backend timing and failures.
 * Remember acknowledged writes so new sessions can acquire a one-row revision.
 * Ordinary loads still reach the fixture (including intentionally stale reloads).
 */
export function pagingBackend(backend: (...args: any[]) => any) {
  const saved = new Map<string, SessionData>();
  const revisions = new Map<string, string>();
  let revision = 0;
  const acknowledge = (data: SessionData, window: Partial<SessionWindow> = {}) => {
    const next = page(data, {
      ...window,
      total: (window.offset ?? 0) + data.messages.length,
      revision: 'test-revision:' + ++revision,
      firstMessageId: data.messages[0]?.id ?? null,
    }).window;
    saved.set(data.session.id, structuredClone(data));
    revisions.set(data.session.id, next.revision);
    return next;
  };
  return async (command: string, args?: any) => {
    if (command === 'load_session_page') {
      const data = args.limit === 1 && saved.has(args.sessionId)
        ? structuredClone(saved.get(args.sessionId)!)
        : await backend('load_session', args);
      const end = args.before ?? data.messages.length;
      const offset = Math.max(0, end - (args.limit ?? end));
      return page({ ...data, messages: data.messages.slice(offset, end) }, {
        offset, total: data.messages.length,
        revision: revisions.get(args.sessionId) ?? 'test-revision',
        firstMessageId: data.messages[offset]?.id ?? null,
      });
    }
    if (command === 'save_session_page' || command === 'checkpoint_session_page') {
      await backend(command === 'save_session_page' ? 'save_session' : 'checkpoint_session', args);
      return acknowledge(args.data, args.window);
    }
    if (command === 'save_session' || command === 'checkpoint_session') {
      const result = await backend(command, args);
      acknowledge(args.data);
      return result;
    }
    if (command === 'checkpoint_session_delta') {
      const result = await backend(command, args);
      return result ?? { revision: args.request.baseRevision, sequence: args.request.sequence };
    }
    if (command === 'warm_session_index') {
      return await backend(command, args) ?? { processed: 0, remaining: 0, errors: 0 };
    }
    return backend(command, args);
  };
}
