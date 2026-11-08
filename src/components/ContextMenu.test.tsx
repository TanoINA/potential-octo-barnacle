import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../types/agent';
import { useState } from 'react';
import { MessageRow } from './MessageRow';
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

  it('opens the real inline editor without resending until the user submits', () => {
    const resend = vi.fn();
    render(<><MessageRow message={dummyMessages[0]} onSelectArtifact={vi.fn()} onEditAndResend={resend} />
      <ContextMenu messages={dummyMessages} onEditAndResendMessage={resend} /></>);
    fireEvent.contextMenu(screen.getByText('Hello AetherAgent'), { clientX: 100, clientY: 100 });
    fireEvent.click(screen.getByRole('button', { name: 'Edit & Resend from Here' }));
    expect(resend).not.toHaveBeenCalled();
    expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Hello AetherAgent');
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Edited prompt' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save & Submit' }));
    expect(resend).toHaveBeenCalledWith('Edited prompt');
  });

  it.each(['Paste', 'Paste as Code Block'])('updates React controlled state through %s', async (label) => {
    function ControlledInput() {
      const [value, setValue] = useState('draft');
      return <><textarea aria-label="Controlled draft" value={value} onChange={(event) => setValue(event.target.value)} /><output>{value}</output><ContextMenu /></>;
    }
    render(<ControlledInput />);
    const input = screen.getByLabelText('Controlled draft') as HTMLTextAreaElement;
    input.focus();
    input.setSelectionRange(5, 5);
    fireEvent.contextMenu(input, { clientX: 100, clientY: 100 });
    fireEvent.click(screen.getByRole('button', { name: label === 'Paste' ? 'Paste Ctrl+V' : label }));
    const expected = label === 'Paste' ? 'draftclipboard content' : 'draft\n```\nclipboard content\n```\n';
    await screen.findByText((_, element) => element?.tagName === 'OUTPUT' && element.textContent === expected);
    expect(input.value).toBe(expected);
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

  it.each(['aether-context-menu', 'aether-context-submenu'])('bounds %s to the viewport and enables vertical scrolling', (testId) => {
    render(<ContextMenu currentSessionId="session-1" onExportSession={vi.fn()} />);
    fireEvent.contextMenu(document.body, { clientX: 100, clientY: 100 });
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Export Session' }));

    const panel = screen.getByTestId(testId);
    expect(panel.classList.contains('max-h-[80vh]')).toBe(true);
    expect(panel.classList.contains('overflow-y-auto')).toBe(true);
  });

  it.each(['aether-context-menu', 'aether-context-submenu'])('keeps both panels open when scrolling inside %s', (testId) => {
    render(<ContextMenu currentSessionId="session-1" onExportSession={vi.fn()} />);
    fireEvent.contextMenu(document.body, { clientX: 100, clientY: 100 });
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Export Session' }));

    const panel = screen.getByTestId(testId);
    fireEvent.scroll(panel);
    expect(screen.queryByTestId('aether-context-menu')).not.toBeNull();
    expect(screen.queryByTestId('aether-context-submenu')).not.toBeNull();

    // Descendant scroll events also travel through the window capture listener.
    fireEvent.scroll(panel.querySelector('button')!);
    expect(screen.queryByTestId('aether-context-menu')).not.toBeNull();
    expect(screen.queryByTestId('aether-context-submenu')).not.toBeNull();
  });

  it.each(['window', 'outside container'])('still dismisses both panels when scrolling the %s', (target) => {
    render(<><div data-testid="outside-scroll" /><ContextMenu currentSessionId="session-1" onExportSession={vi.fn()} /></>);
    fireEvent.contextMenu(document.body, { clientX: 100, clientY: 100 });
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Export Session' }));
    expect(screen.getByTestId('aether-context-submenu')).toBeTruthy();

    fireEvent.scroll(target === 'window' ? window : screen.getByTestId('outside-scroll'));
    expect(screen.queryByTestId('aether-context-menu')).toBeNull();
    expect(screen.queryByTestId('aether-context-submenu')).toBeNull();
  });

  it('handles menu keys before composer handlers without submitting the composer', () => {
    const composerKeyDown = vi.fn();
    const openSettings = vi.fn();
    render(<><textarea aria-label="Composer" onKeyDown={composerKeyDown} /><ContextMenu onOpenSettings={openSettings} /></>);
    const composer = screen.getByLabelText('Composer');
    composer.focus();
    fireEvent.contextMenu(composer, { clientX: 100, clientY: 100 });

    // Hover chooses an action while keyboard focus remains in the composer.
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Settings Ctrl+,' }));
    expect(document.activeElement).toBe(composer);
    expect(fireEvent.keyDown(composer, { key: 'Enter' })).toBe(false);
    expect(composerKeyDown).not.toHaveBeenCalled();
    expect(openSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId('aether-context-menu')).toBeNull();

    // Closing the menu must remove its capturing keyboard listener.
    fireEvent.keyDown(composer, { key: 'Enter' });
    expect(composerKeyDown).toHaveBeenCalledTimes(1);
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

  it('supports full keyboard navigation with ArrowDown and Enter to select an item', () => {
    const handleNewSession = vi.fn();
    render(
      <div>
        <div data-testid="canvas">Canvas</div>
        <ContextMenu onNewSession={handleNewSession} />
      </div>,
    );

    const canvas = screen.getByTestId('canvas');
    fireEvent.contextMenu(canvas, { clientX: 100, clientY: 100 });
    expect(screen.getByTestId('aether-context-menu')).toBeTruthy();

    // First item in canvas menu is "New Chat"
    fireEvent.keyDown(window, { key: 'ArrowDown' });
    fireEvent.keyDown(window, { key: 'Enter' });

    expect(handleNewSession).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId('aether-context-menu')).toBeNull();
  });

  it('prioritizes clicked target when right-clicking outside of highlighted text selection', () => {
    // Mock selection that does not intersect target
    const mockRange = {
      intersectsNode: vi.fn().mockReturnValue(false),
    };
    vi.spyOn(window, 'getSelection').mockReturnValue({
      toString: () => 'some old highlighted text',
      rangeCount: 1,
      getRangeAt: () => mockRange as unknown as Range,
    } as unknown as Selection);

    render(
      <div>
        <a
          data-testid="file-anchor"
          data-file-link="true"
          data-file-path="D:/project/README.md"
          href="file:///D:/project/README.md"
        >
          README.md
        </a>
        <ContextMenu />
      </div>,
    );

    const anchor = screen.getByTestId('file-anchor');
    fireEvent.contextMenu(anchor, { clientX: 100, clientY: 100 });

    // Should open File / Artifact menu, NOT Selection menu
    expect(screen.getByText('File / Artifact')).toBeTruthy();
    expect(screen.queryByText('Selection')).toBeNull();
  });
});
