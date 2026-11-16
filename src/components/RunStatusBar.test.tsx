import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { RunStatusBar } from './RunStatusBar';
import type { RunStatus } from '../hooks/useAgentLiveSession';
afterEach(() => { cleanup(); vi.useRealTimers(); });
it('shows phase and elapsed time with one timer only while active and visible', () => {
 vi.useFakeTimers(); vi.setSystemTime(10000);
 const status: RunStatus = { runId: 'r', sessionId: 's', origin: 'chat', phase: 'waiting_provider', startedAt: 8000, phaseSince: 9000 };
 const view = render(<RunStatusBar status={status} />);
 expect(screen.getByRole('status').textContent).toContain('Waiting for provider');
 expect(screen.getByText('2s elapsed · 1s in stage')).toBeTruthy();
 expect(vi.getTimerCount()).toBe(1);
 act(() => { vi.advanceTimersByTime(2000); });
 expect(screen.getByText('4s elapsed · 3s in stage')).toBeTruthy();
 view.rerender(<RunStatusBar status={{...status, phase:'completed', endedAt:12000, outcome:{answer:'completed',filesChanged:true, verification:'unknown'}}} />);
 expect(screen.getByRole('status').textContent).toContain('Answer complete · Changes applied · Verification not recorded');
 expect(vi.getTimerCount()).toBe(0);
 view.unmount(); expect(vi.getTimerCount()).toBe(0);
});
it('offers only a real preparation cancellation and pauses the clock in a hidden document', () => {
 vi.useFakeTimers(); vi.setSystemTime(1000);
 const cancel = vi.fn();
 const view=render(<RunStatusBar status={{runId:null,sessionId:'s',origin:'chat',phase:'preparing_attachments',startedAt:1000,phaseSince:1000}} onCancelPreparation={cancel} />);
 screen.getByRole('button',{name:'Cancel preparation'}).click(); expect(cancel).toHaveBeenCalledOnce();
 const hidden=vi.spyOn(document,'hidden','get').mockReturnValue(true);
 act(() => { document.dispatchEvent(new Event('visibilitychange')); }); expect(vi.getTimerCount()).toBe(0);
 hidden.mockReturnValue(false); act(() => {document.dispatchEvent(new Event('visibilitychange'));}); expect(vi.getTimerCount()).toBe(1);
 view.rerender(<RunStatusBar status={{runId:'r',sessionId:'s',origin:'chat',phase:'saving_results',startedAt:1000,phaseSince:1000}} onCancelPreparation={cancel} />);
 expect(screen.queryByRole('button')).toBeNull();
 view.unmount(); hidden.mockRestore(); expect(vi.getTimerCount()).toBe(0);
});
it('shows a save failure without presenting overall completion or verification success', () => {
 render(<RunStatusBar status={{runId:'r',sessionId:'s',origin:'chat',phase:'error',startedAt:1000,phaseSince:2000,endedAt:2000,saveFailed:true,outcome:{answer:'completed',filesChanged:true,verification:'unknown'}}} />);
 expect(screen.getByRole('status').textContent).toBe('Answer complete · Save failed');
 expect(screen.queryByText(/Verification passed/)).toBeNull();
});
it('renders context meter accurately with token counts and percentage', () => {
 render(<RunStatusBar contextUsage={{ usedTokens: 18400, maxTokens: 128000 }} />);
 expect(screen.getByTestId('context-meter')).toBeTruthy();
 expect(screen.getByText('18.4k / 128k tokens (14%)')).toBeTruthy();
 expect(screen.queryByRole('button', { name: 'Compact' })).toBeNull();
});

it('renders context meter over 100% without clamping percentage and offers compact button', () => {
 const compact = vi.fn();
 render(<RunStatusBar contextUsage={{ usedTokens: 210500, maxTokens: 128000 }} onCompact={compact} />);
 expect(screen.getByText('210.5k / 128k tokens (164%)')).toBeTruthy();
 const compactBtn = screen.getByRole('button', { name: 'Compact' });
 expect(compactBtn).toBeTruthy();
 compactBtn.click();
 expect(compact).toHaveBeenCalledOnce();
});
