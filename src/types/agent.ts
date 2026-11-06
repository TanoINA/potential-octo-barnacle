export type Role = 'user' | 'assistant' | 'system' | 'tool';
export type ProviderKind = 'openai' | 'anthropic' | 'deepseek' | 'ollama';
export type ReasoningEffort = 'minimal' | 'low' | 'medium' | 'high' | 'extra_high' | 'max' | 'ultra';
export type ToolStatus = 'pending' | 'running' | 'done' | 'error' | 'cancelled';
export type ArtifactType = 'code' | 'diff' | 'terminal' | 'markdown' | 'file';

export interface FileDiffStat {
  path: string;
  additions: number;
  deletions: number;
}

export interface ToolCallItem {
  id: string;
  name: string;
  args: Record<string, unknown>;
  status: ToolStatus;
  result?: string;
  artifactId?: string;
  argumentsRaw?: string;
  startedAt?: number;
  completedAt?: number;
  durationMs?: number;
  exitCode?: number;
  terminalOutput?: string;
  filesChanged?: FileDiffStat[];
}

export interface AttachedContext {
  id: string;
  path: string;
  name: string;
  isDir: boolean;
}

export interface CouncilCandidate {
  providerId: string;
  model: string;
  content: string;
  reasoning?: string;
  isStreaming?: boolean;
  durationMs?: number;
  winner?: boolean;
}

export interface CouncilTurn {
  candidateA: CouncilCandidate;
  candidateB: CouncilCandidate;
  chosenWinner?: 'A' | 'B';
}

export interface SelfImprovementEvent {
  run_id: string;
  session_id: string;
  action: 'skill_created' | 'skill_updated' | 'no_action';
  skill_name?: string;
  summary?: string;
  workspace_cwd?: string;
}

export interface MessageSegment {
  id: string;
  text?: string;
  toolCalls?: ToolCallItem[];
}

export interface ChatMessage {
  id: string;
  role: Role;
  content: string;
  timestamp: number;
  reasoning?: string;
  toolCalls?: ToolCallItem[];
  isStreaming?: boolean;
  attachments?: AttachedContext[];
  councilTurn?: CouncilTurn;
  runId?: string;
  runStatus?: 'completed' | 'cancelled' | 'failed';
  selfImprovement?: SelfImprovementEvent;
  preamble?: string;
  segments?: MessageSegment[];
}

export interface Artifact {
  id: string;
  title: string;
  type: ArtifactType;
  language?: string;
  content: string;
  filePath?: string;
  timestamp: number;
}

export interface GlobalArtifactItem {
  sessionId: string;
  sessionTitle: string;
  sessionUpdatedAt: number;
  artifact: Artifact;
}

export interface DeepSearchResult {
  sessionId: string;
  sessionTitle: string;
  sessionUpdatedAt: number;
  snippet: string;
  matchCount: number;
  score: number;
}

export interface ProviderConfig {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  apiKey: string;
  model: string;
  models?: string[];
  customModels?: string[];
  extraHeaders?: Record<string, string>;
  extraBody?: Record<string, unknown>;
}

export interface McpServerConfig {
  id: string;
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  enabled: boolean;
}

export interface AgentSettings {
  providers: ProviderConfig[];
  activeProviderId: string;
  systemPrompt: string;
  temperature: number;
  streamResponse: boolean;
  maxTokens?: number;
  thinking: boolean;
  reasoningEffort: ReasoningEffort;
  favoriteModels: string[];
  skillPaths: string[];
  mcpServers: McpServerConfig[];
  maxTurns: number;
  notificationsEnabled?: boolean;
  soundChimeEnabled?: boolean;
  self_improvement_enabled: boolean;
  autoCompactionEnabled?: boolean;
  autoCompactionThresholdTokens?: number;
}

export interface Session {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messageCount: number;
  workspaceCwd?: string;
  preview?: string;
  pinned?: boolean;
  pinnedAt?: number;
}

export interface SessionModelOverride {
  providerId?: string;
  model?: string;
  thinking?: boolean;
  reasoningEffort?: ReasoningEffort;
}

export interface SessionData {
  session: Session;
  messages: ChatMessage[];
  artifacts: Artifact[];
  modelOverride?: SessionModelOverride | null;
}

export interface SkillInfo {
  name: string;
  description: string;
  path: string;
  source: string;
}

export interface UndoTurnResult {
  content: string;
  attachments?: AttachedContext[];
}

export interface AgentRunRequest {
  runId: string;
  sessionId: string;
  provider: ProviderConfig;
  systemPrompt?: string;
  messages: ChatMessage[];
  thinking: boolean;
  reasoningEffort: ReasoningEffort;
  temperature: number;
  maxTokens?: number;
  maxTurns: number;
  streamResponse: boolean;
  skillPaths: string[];
  mcpServers: McpServerConfig[];
  self_improvement_enabled: boolean;
  autoCompactionEnabled?: boolean;
  autoCompactionThresholdTokens?: number;
}

export interface MicroCompactionEvent {
  runId: string;
  sessionId: string;
  prunedToolCount: number;
  estimatedTokensSaved: number;
}

interface EventScope {
  runId: string;
  sessionId: string;
}

export interface SubagentProgress {
  id: string;
  role: string;
  task: string;
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  startedAt: number;
  completedAt?: number;
  summary?: string;
  artifactId?: string;
}

export type AgentEvent =
  | { type: 'chunk'; data: EventScope & { text: string } }
  | { type: 'reasoning'; data: EventScope & { text: string } }
  | { type: 'tool_update'; data: EventScope & { tool: ToolCallItem } }
  | { type: 'artifact'; data: EventScope & { artifact: Artifact } }
  | { type: 'terminal_output'; data: EventScope & { artifactId: string; delta: string; stream: 'stdout' | 'stderr' | 'system' } }
  | { type: 'done'; data: EventScope & { cancelled: boolean; successful?: boolean } }
  | { type: 'error'; data: EventScope & { message: string } }
  | { type: 'subagent_update'; data: EventScope & { subagent: SubagentProgress } }
  | { type: 'skill_activated'; data: EventScope & { skill: SkillInfo } }
  | { type: 'council_chunk'; data: EventScope & { candidate: 'A' | 'B'; kind: 'content' | 'reasoning'; text: string } }
  | { type: 'council_done'; data: EventScope & { candidate: 'A' | 'B'; durationMs: number } };

export interface EffectiveRunConfig {
  provider: ProviderConfig;
  thinking: boolean;
  reasoningEffort: ReasoningEffort;
  usesSettingsDefault: boolean;
}

export interface GitFileChange {
  path: string;
  status: string;
}

export interface GitRepoStatus {
  isRepo: boolean;
  branch: string;
  hasChanges: boolean;
  summary: string;
  files: GitFileChange[];
}
