import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ChatCanvas } from './ChatCanvas';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('./MessageRow', () => ({ MessageRow: () => null }));

function setup() {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
  const review = vi.fn(async () => true);
  const send = vi.fn(async () => true);
  render(<ChatCanvas messages={[]} isStreaming={false} stopRequested={false}
    onSendMessage={send} onStop={vi.fn()} onSelectArtifact={vi.fn()} onClearSession={vi.fn()}
    onNewSession={() => 'new'} onOpenSettings={vi.fn()} onOpenModelPicker={vi.fn()}
    effectiveConfig={null} sessionTitle="Review test" artifactCount={0} isArtifactOpen={false}
    onToggleArtifacts={vi.fn()} onListFiles={async () => []} onReviewSession={review} />);
  return { review, send };
}

describe('ChatCanvas reflection commands', () => {
  it.each(['/review', '/reflect'])('dispatches %s to reflection, not a chat completion', async (command) => {
    const { review, send } = setup();
    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: command } });
    const option = await screen.findByText(command, { selector: '[data-slash-item] *' });
    fireEvent.click(option);
    await waitFor(() => expect(review).toHaveBeenCalledOnce());
    expect(send).not.toHaveBeenCalled();
  });
});
