import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ChatHeader } from './ChatHeader';

describe('ChatHeader controls', () => {
  it('keeps model selection in the composer while preserving session actions', () => {
    const onOpenInChatSearch = vi.fn();
    render(<ChatHeader totalMessageCount={0} councilMode={false} setCouncilMode={vi.fn()}
      onOpenModelPicker={vi.fn()} effectiveConfig={{ provider: { model: 'test-model' } }}
      onToggleArtifacts={vi.fn()} artifactCount={0} isArtifactOpen={false}
      onOpenInChatSearch={onOpenInChatSearch} onOpenSettings={vi.fn()} onClearConversation={vi.fn()} />);
    expect(screen.queryByText('test-model')).toBeNull();
    expect(screen.queryByTitle('Select model / override settings')).toBeNull();
    expect(screen.queryByText('0 messages')).toBeNull();
    fireEvent.click(screen.getByTitle('More actions (Search, Export, Settings)'));
    fireEvent.click(screen.getByText('Find in chat'));
    expect(onOpenInChatSearch).toHaveBeenCalledTimes(1);
  });
});
