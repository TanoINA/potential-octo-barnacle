import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../types/agent';
import { ContextMenu } from './ContextMenu';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe('ContextMenu Component', () => {
  const dummyMessages: ChatMessage[] = [
    {
      id: 'msg-1',
      role: 'user',
      content: 'Hello AetherAgent',
      timestamp: 1000,
    },
    {
      id: 'msg-2',
      role: 'assistant',
      content: 'Hello! How can I assist you?',
      timestamp: 2000,
    },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
        readText: vi.fn().mockResolvedValue('clipboard content'),
      },
    });
    // mock window.__TAURI_INTERNALS__
    (window as any).__TAURI_INTERNALS__ = {};
  });

  it('renders nothing initially until a contextmenu event fires', () => {
    render(<ContextMenu workspaceCwd="D:/AetherAgent" />);
    expect(screen.queryByTestId('aether-context-menu')).toBeNull();
  });

  it('prevents default browser menu and opens custom context menu on right-click', () => {
    render(
      <div>
        <div data-testid="target-canvas">Empty Canvas</div>
        <ContextMenu workspaceCwd="D:/AetherAgent" />
      </div>,
    );

    const canvas = screen.getByTestId('target-canvas');
    const notPrevented = fireEvent.contextMenu(canvas, {
      clientX: 150,
      clientY: 200,
    });

    expect(notPrevented).toBe(false);
    expect(screen.getByTestId('aether-context-menu')).toBeTruthy();
    expect(screen.getByText('Open Workspace in Explorer')).toBeTruthy();
    expect(screen.getByText('Open Terminal in Workspace')).toBeTruthy();
  });

  it('closes on Escape key press', () => {
    render(
      <div>
        <div data-testid="target-canvas">Empty Canvas</div>
        <ContextMenu workspaceCwd="D:/AetherAgent" />
      </div>,
    );

    const canvas = screen.getByTestId('target-canvas');
    fireEvent.contextMenu(canvas, { clientX: 100, clientY: 100 });
    expect(screen.getByTestId('aether-context-menu')).toBeTruthy();

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByTestId('aether-context-menu')).toBeNull();
  });

  it('triggers onNewSession when New Chat is clicked', () => {
    const handleNewSession = vi.fn();
    render(
      <div>
        <div data-testid="canvas">Canvas</div>
        <ContextMenu onNewSession={handleNewSession} />
      </div>,
    );

    const canvas = screen.getByTestId('canvas');
    fireEvent.contextMenu(canvas, { clientX: 100, clientY: 100 });

    const newChatBtn = screen.getByText('New Chat');
    fireEvent.click(newChatBtn);

    expect(handleNewSession).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId('aether-context-menu')).toBeNull();
  });

  it('triggers onOpenSettings when Settings is clicked', () => {
    const handleOpenSettings = vi.fn();
    render(
      <div>
        <div data-testid="canvas">Canvas</div>
        <ContextMenu onOpenSettings={handleOpenSettings} />
      </div>,
    );

    const canvas = screen.getByTestId('canvas');
    fireEvent.contextMenu(canvas, { clientX: 100, clientY: 100 });

    const settingsBtn = screen.getByText('Settings');
    fireEvent.click(settingsBtn);

    expect(handleOpenSettings).toHaveBeenCalledTimes(1);
  });

  it('renders Selection section when text is highlighted', () => {
    vi.spyOn(window, 'getSelection').mockReturnValue({
      toString: () => 'const answer = 42;',
    } as Selection);

    render(
      <div>
        <div data-testid="text-container">const answer = 42;</div>
        <ContextMenu />
      </div>,
    );

    const container = screen.getByTestId('text-container');
    fireEvent.contextMenu(container, { clientX: 100, clientY: 100 });

    expect(screen.getByText('Selection')).toBeTruthy();
    expect(screen.getByText('Ask AetherAgent')).toBeTruthy();
    expect(screen.getByText('Copy')).toBeTruthy();
    expect(screen.getByText('Copy as Quote')).toBeTruthy();
    expect(screen.getByText('Run in PowerShell as Task')).toBeTruthy();
  });

  it('opens Ask AetherAgent submenu on hover and dispatches prompt event on click', async () => {
    vi.spyOn(window, 'getSelection').mockReturnValue({
      toString: () => 'def hello(): pass',
    } as Selection);

    const promptListener = vi.fn();
    window.addEventListener('aether:insert-prompt', promptListener);

    render(
      <div>
        <div data-testid="text-container">def hello(): pass</div>
        <ContextMenu />
      </div>,
    );

    const container = screen.getByTestId('text-container');
    fireEvent.contextMenu(container, { clientX: 100, clientY: 100 });

    const askBtn = screen.getByText('Ask AetherAgent').closest('button')!;
    fireEvent.mouseEnter(askBtn);

    expect(screen.getByTestId('aether-context-submenu')).toBeTruthy();
    expect(screen.getByText('Quote & Reply')).toBeTruthy();
    expect(screen.getByText('Explain Selection')).toBeTruthy();
    expect(screen.getByText('Refactor & Optimize')).toBeTruthy();

    const explainBtn = screen.getByText('Explain Selection');
    fireEvent.click(explainBtn);

    expect(promptListener).toHaveBeenCalled();
    const eventDetail = (promptListener.mock.calls[0][0] as CustomEvent).detail;
    expect(eventDetail.text).toContain('Explain the following snippet in detail');
    expect(eventDetail.text).toContain('def hello(): pass');
    expect(eventDetail.autoSend).toBe(true);

    window.removeEventListener('aether:insert-prompt', promptListener);
  });

  it('detects Code Block context and provides code actions', () => {
    const rawCode = 'console.log("AetherAgent");';
    render(
      <div>
        <div
          data-testid="code-wrapper"
          data-code-block="true"
          data-code-lang="typescript"
          data-code-content={encodeURIComponent(rawCode)}
        >
          <code>{rawCode}</code>
        </div>
        <ContextMenu />
      </div>,
    );

    const codeWrapper = screen.getByTestId('code-wrapper');
    fireEvent.contextMenu(codeWrapper, { clientX: 120, clientY: 150 });

    expect(screen.getByText('Code (typescript)')).toBeTruthy();
    expect(screen.getByText('Copy Code')).toBeTruthy();
    expect(screen.getByText('Explain Architecture & Flow')).toBeTruthy();
    expect(screen.getByText('Generate Unit Tests')).toBeTruthy();
    expect(screen.getByText('Audit Bugs & Edge Cases')).toBeTruthy();

    const copyCodeBtn = screen.getByText('Copy Code');
    fireEvent.click(copyCodeBtn);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(rawCode);
  });

  it('detects File Link context and invokes reveal_in_explorer', () => {
    render(
      <div>
        <a
          data-testid="file-anchor"
          data-file-link="true"
          data-file-path="D:/UNIKOM/Tugas 1/4_Cara_Instalasi_Zorin_OS.docx"
          href="file:///D:/UNIKOM/Tugas%201/4_Cara_Instalasi_Zorin_OS.docx"
        >
          4_Cara_Instalasi_Zorin_OS.docx
        </a>
        <ContextMenu />
      </div>,
    );

    const anchor = screen.getByTestId('file-anchor');
    fireEvent.contextMenu(anchor, { clientX: 120, clientY: 150 });

    expect(screen.getByText('File / Artifact')).toBeTruthy();
    expect(screen.getByText('Open File')).toBeTruthy();
    expect(screen.getByText('Reveal in File Explorer')).toBeTruthy();

    const revealBtn = screen.getByText('Reveal in File Explorer');
    fireEvent.click(revealBtn);

    expect(mockInvoke).toHaveBeenCalledWith('reveal_in_explorer', {
      path: 'D:/UNIKOM/Tugas 1/4_Cara_Instalasi_Zorin_OS.docx',
    });
  });

  it('detects Message Row context and executes retry/regenerate or rollback', () => {
    const handleRetry = vi.fn().mockResolvedValue(true);
    const handleRollback = vi.fn().mockResolvedValue(true);

    render(
      <div>
        <div
          data-testid="message-wrapper"
          data-message-id="msg-2"
          data-message-role="assistant"
        >
          <div>Hello! How can I assist you?</div>
        </div>
        <ContextMenu
          messages={dummyMessages}
          onRetryTurn={handleRetry}
          onRollbackToMessage={handleRollback}
        />
      </div>,
    );

    const msgWrapper = screen.getByTestId('message-wrapper');
    fireEvent.contextMenu(msgWrapper, { clientX: 120, clientY: 150 });

    expect(screen.getByText('Assistant Turn')).toBeTruthy();
    expect(screen.getByText('Regenerate Response')).toBeTruthy();
    expect(screen.getByText('Rollback to this Turn')).toBeTruthy();

    const regenBtn = screen.getByText('Regenerate Response');
    fireEvent.click(regenBtn);

    expect(handleRetry).toHaveBeenCalledWith('msg-2');
  });
});
