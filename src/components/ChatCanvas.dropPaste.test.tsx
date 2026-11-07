import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { ChatCanvas } from './ChatCanvas';
import type { AgentSettings } from '../types/agent';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('./MessageRow', () => ({ MessageRow: () => null }));

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

type CanvasProps = ComponentProps<typeof ChatCanvas>;

function agentSettings(): AgentSettings {
  return {
    providers: [{
      id: 'p1', name: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1',
      apiKey: 'test', model: 'gpt-4o', models: ['gpt-4o'],
    }],
    activeProviderId: 'p1',
    systemPrompt: 'test',
    temperature: 0.7,
    streamResponse: true,
    thinking: false,
    reasoningEffort: 'low',
    favoriteModels: [],
    skillPaths: [],
    mcpServers: [],
    maxTurns: 24,
    self_improvement_enabled: false,
  };
}

function renderCanvas(overrides: Partial<CanvasProps> = {}) {
  const props = {
    messages: [],
    isStreaming: false,
    stopRequested: false,
    settings: agentSettings(),
    onSendMessage: vi.fn(async () => true),
    onStop: vi.fn(),
    onSelectArtifact: vi.fn(),
    onClearSession: vi.fn(),
    onNewSession: () => 'new',
    onOpenSettings: vi.fn(),
    onOpenModelPicker: vi.fn(),
    effectiveConfig: null,
    sessionTitle: 'Drop paste test',
    artifactCount: 0,
    isArtifactOpen: false,
    onToggleArtifacts: vi.fn(),
    onListFiles: async () => [],
    ...overrides,
  } as CanvasProps;
  const rendered = render(<ChatCanvas {...props} />);
  return { ...rendered, props };
}

describe('ChatCanvas drag-and-drop and clipboard paste', () => {
  it('attaches an image when pasted via clipboard event on the textarea', async () => {
    renderCanvas();
    const textarea = screen.getByLabelText('Message AetherAgent');

    const fakeFile = new File([new Uint8Array([137, 80, 78, 71])], 'screenshot.png', { type: 'image/png' });
    Object.defineProperty(fakeFile, 'arrayBuffer', {
      value: async () => new Uint8Array([137, 80, 78, 71]).buffer,
    });

    const pasteEvent = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, 'clipboardData', {
      value: {
        items: [{
          kind: 'file',
          type: 'image/png',
          getAsFile: () => fakeFile,
        }],
        files: [fakeFile],
      },
    });

    fireEvent(textarea, pasteEvent);

    await waitFor(() => {
      expect(screen.getByText('screenshot.png')).toBeTruthy();
    });
  });

  it('attaches dropped files into context pills on drop event', async () => {
    const { container } = renderCanvas();
    const dropZone = container.firstChild as HTMLElement;

    const fakeFile = new File(['hello world'], 'notes.txt', { type: 'text/plain' });
    Object.defineProperty(fakeFile, 'path', { value: 'C:/docs/notes.txt' });

    fireEvent.drop(dropZone, {
      dataTransfer: {
        files: [fakeFile],
      },
    });

    await waitFor(() => {
      expect(screen.getByText('notes.txt')).toBeTruthy();
    });
  });
});
