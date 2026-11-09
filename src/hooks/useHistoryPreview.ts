import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SessionPage } from '../types/agent';

/** Viewport-only data. Never enters LiveSession, persistence, or model history. */
export function useHistoryPreview() {
  const [page, setPage] = useState<SessionPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [jump, setJump] = useState<{ messageId: string; request: number } | null>(null);
  const pageRef = useRef(page);
  const generation = useRef(0);
  const busy = useRef(false);
  useEffect(() => () => { generation.current += 1; pageRef.current = null; busy.current = false; }, []);
  const begin = useCallback(() => {
    const request = ++generation.current;
    busy.current = true; setLoading(true); setError(null);
    return () => generation.current === request;
  }, []);
  const clear = useCallback(() => {
    generation.current += 1;
    pageRef.current = null;
    busy.current = false;
    setPage(null); setLoading(false); setError(null);
    setJump({ messageId: '', request: generation.current });
  }, []);
  const open = useCallback(async (sessionId: string, messageId: string, isOrigin: () => boolean) => {
    const request = ++generation.current;
    busy.current = true;
    setLoading(true); setError(null);
    try {
      const next = await invoke<SessionPage>('load_session_around', { sessionId, messageId, limit: 50 });
      if (generation.current !== request || !isOrigin()) return false;
      if (next.data.session.id !== sessionId || !next.data.messages.some((message) => message.id === messageId)) {
        throw new Error('Message was not found; refresh search results.');
      }
      pageRef.current = next; setPage(next);
      setJump({ messageId, request });
      return true;
    } catch (cause) {
      if (generation.current === request && isOrigin()) setError(String(cause));
      return false;
    } finally {
      if (generation.current === request) { busy.current = false; setLoading(false); }
    }
  }, []);
  const loadOlder = useCallback(async () => {
    const current = pageRef.current;
    if (!current || current.window.offset <= 0 || busy.current) return;
    const request = generation.current;
    busy.current = true; setLoading(true); setError(null);
    try {
      const older = await invoke<SessionPage>('load_session_page', {
        sessionId: current.data.session.id, before: current.window.offset, limit: 50, revision: current.window.revision,
      });
      if (generation.current !== request || pageRef.current !== current) return;
      if (older.window.revision !== current.window.revision || older.window.total !== current.window.total
        || older.window.offset + older.data.messages.length !== current.window.offset) throw new Error('History changed; reopen the search result.');
      const next: SessionPage = {
        data: { ...current.data, messages: [...older.data.messages, ...current.data.messages] },
        window: { ...current.window, offset: older.window.offset, firstMessageId: older.window.firstMessageId },
      };
      pageRef.current = next; setPage(next);
    } catch (cause) {
      if (generation.current === request) setError(String(cause));
    } finally {
      if (generation.current === request) { busy.current = false; setLoading(false); }
    }
  }, []);
  const isActive = useCallback(() => pageRef.current !== null || busy.current, []);
  return { page, loading, error, jump, clear, begin, open, loadOlder, isActive };
}
