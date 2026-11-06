import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { AgentSettings } from '../types/agent';
import { ToolsInspectorModal } from './ToolsInspectorModal';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const settings: AgentSettings = {
  providers: [],
  activeProviderId: '',
  systemPrompt: '',
  temperature: 0.7,
  streamResponse: true,
  thinking: false,
  reasoningEffort: 'medium',
  favoriteModels: [],
  skillPaths: [],
  maxTurns: 24,
  self_improvement_enabled: false,
  mcpServers: [{ id: 'local', name: 'Local MCP', command: 'node', args: [], env: {}, enabled: true }],
};

describe('ToolsInspectorModal', () => {
  it('loads real native schemas and passes each enabled MCP server config', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_native_tools') {
        return [{ function: { name: 'fetch_web_page', description: 'Fetch a page', parameters: { type: 'object' } } }];
      }
      if (command === 'list_mcp_tools') {
        return [{ name: 'echo', description: 'Echo text', inputSchema: { type: 'object' } }];
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    render(<ToolsInspectorModal open onClose={vi.fn()} settings={settings} />);
    expect(await screen.findByText('fetch_web_page')).toBeTruthy();
    expect(await screen.findByText('echo')).toBeTruthy();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('list_mcp_tools', { server: settings.mcpServers[0] }));
  });
});
