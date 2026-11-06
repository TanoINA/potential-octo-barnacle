import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { useSpeechRecognition } from './useSpeechRecognition';

class FakeSpeechRecognition {
  static instances: FakeSpeechRecognition[] = [];
  continuous = false;
  interimResults = false;
  lang = '';
  onresult: ((event: unknown) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onend: (() => void) | null = null;
  start = vi.fn();
  stop = vi.fn();
  abort = vi.fn();
  constructor() {
    FakeSpeechRecognition.instances.push(this);
  }
}

beforeAll(() => {
  (window as unknown as { SpeechRecognition: unknown }).SpeechRecognition = FakeSpeechRecognition;
});

describe('useSpeechRecognition transcript emission', () => {
  it('emits only finalized transcripts, never interim ones', async () => {
    FakeSpeechRecognition.instances = [];
    const onTranscript = vi.fn();
    const { result } = renderHook(() => useSpeechRecognition({ onTranscript }));

    await waitFor(() => expect(result.current.isSupported).toBe(true));
    const recognition = FakeSpeechRecognition.instances[0];

    act(() => {
      result.current.startListening();
    });
    expect(result.current.isListening).toBe(true);

    // Interim result while the user is still speaking: the composer must not
    // receive it, otherwise every audio chunk appends duplicate text.
    act(() => {
      recognition.onresult?.({
        resultIndex: 0,
        results: [{ 0: { transcript: 'hello' }, isFinal: false, length: 1 }],
      });
    });
    expect(onTranscript).not.toHaveBeenCalled();

    // Finalized result: emitted exactly once.
    act(() => {
      recognition.onresult?.({
        resultIndex: 0,
        results: [{ 0: { transcript: 'hello world' }, isFinal: true, length: 1 }],
      });
    });
    expect(onTranscript).toHaveBeenCalledTimes(1);
    expect(onTranscript).toHaveBeenCalledWith('hello world');
  });

  it('emits only the final alternative when a result batch mixes final and interim entries', async () => {
    FakeSpeechRecognition.instances = [];
    const onTranscript = vi.fn();
    const { result } = renderHook(() => useSpeechRecognition({ onTranscript }));

    await waitFor(() => expect(result.current.isSupported).toBe(true));
    const recognition = FakeSpeechRecognition.instances[0];

    act(() => {
      result.current.startListening();
    });

    act(() => {
      recognition.onresult?.({
        resultIndex: 0,
        results: [
          { 0: { transcript: 'fix this' }, isFinal: true, length: 1 },
          { 0: { transcript: 'and this' }, isFinal: false, length: 1 },
        ],
      });
    });

    expect(onTranscript).toHaveBeenCalledTimes(1);
    expect(onTranscript).toHaveBeenCalledWith('fix this');
  });

  it('stops listening when the recognition stream ends', async () => {
    FakeSpeechRecognition.instances = [];
    const { result } = renderHook(() => useSpeechRecognition({ onTranscript: vi.fn() }));

    await waitFor(() => expect(result.current.isSupported).toBe(true));
    const recognition = FakeSpeechRecognition.instances[0];

    act(() => {
      result.current.startListening();
    });
    expect(result.current.isListening).toBe(true);

    act(() => {
      recognition.onend?.();
    });
    expect(result.current.isListening).toBe(false);
  });
});
