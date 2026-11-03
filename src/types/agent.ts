export type Role = 'user' | 'assistant' | 'system' | 'tool';

export interface ToolCallItem {
  id: string;
  name: string;
  args: Record<string, any>;
  status: 'pending' | 'running' | 'done' | 'error';
  result?: string;
  artifactId?: string;
}

export interface ChatMessage {
  id: string;
  role: Role;
  content: string;
  timestamp: number;
  reasoning?: string;
  toolCalls?: ToolCallItem[];
  isStreaming?: boolean;
}

export interface Artifact {
  id: string;
  title: string;
  type: 'code' | 'diff' | 'terminal' | 'markdown' | 'file';
  language?: string;
  content: string;
  filePath?: string;
  timestamp: number;
}

export interface ProviderConfig {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  model: string;
}

export interface Session {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messageCount: number;
}

export interface AgentSettings {
  providers: ProviderConfig[];
  activeProviderId: string;
  systemPrompt: string;
  temperature: number;
  streamResponse: boolean;
  maxTokens?: number;
}

export interface AgentEvent {
  type: 'chunk' | 'reasoning' | 'tool_update' | 'artifact' | 'done' | 'error';
  data: any;
}
