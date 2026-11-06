import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Search, Wrench, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import type { AgentSettings } from '../types/agent';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

interface ToolsInspectorModalProps {
  open: boolean;
  onClose: () => void;
  settings: AgentSettings | null;
}

interface NativeTool {
  name: string;
  description: string;
  parameters: unknown;
}

interface NativeToolEnvelope {
  function?: { name?: string; description?: string; parameters?: unknown };
}

interface McpTool {
  name?: string;
  description?: string;
  inputSchema?: unknown;
  input_schema?: unknown;
}

interface McpGroup {
  serverId: string;
  serverName: string;
  tools: McpTool[];
}

export const ToolsInspectorModal: React.FC<ToolsInspectorModalProps> = ({ open, onClose, settings }) => {
  const [filter, setFilter] = useState('');
  const [nativeTools, setNativeTools] = useState<NativeTool[]>([]);
  const [mcpGroups, setMcpGroups] = useState<McpGroup[]>([]);
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<string[]>([]);
  const filterRef = useRef<HTMLInputElement>(null);
  const dialogRef = useModalAccessibility<HTMLDivElement>(open, onClose, filterRef);

  useEffect(() => {
    if (!open) return;
    let active = true;
    setLoading(true);
    setErrors([]);
    const load = async () => {
      const nextErrors: string[] = [];
      try {
        const native = await invoke<NativeToolEnvelope[]>('list_native_tools');
        if (active) {
          setNativeTools(native.flatMap((entry) => {
            const definition = entry.function;
            if (!definition?.name) return [];
            return [{ name: definition.name, description: definition.description || 'No description provided.', parameters: definition.parameters ?? {} }];
          }));
        }
      } catch (error) {
        nextErrors.push(`Native tools: ${String(error)}`);
      }
      const servers = (settings?.mcpServers || []).filter((server) => server.enabled);
      const groups = await Promise.all(servers.map(async (server): Promise<McpGroup | null> => {
        try {
          const tools = await invoke<McpTool[]>('list_mcp_tools', { server });
          return { serverId: server.id, serverName: server.name, tools };
        } catch (error) {
          nextErrors.push(`${server.name}: ${String(error)}`);
          return null;
        }
      }));
      if (!active) return;
      setMcpGroups(groups.filter((group): group is McpGroup => group !== null));
      setErrors(nextErrors);
      setLoading(false);
    };
    void load();
    return () => { active = false; };
  }, [open, settings]);

  const filteredNative = useMemo(() => {
    const query = (filter || '').trim().toLowerCase();
    if (!query) return nativeTools;
    return nativeTools.filter((tool) => `${tool.name} ${tool.description}`.toLowerCase().includes(query));
  }, [filter, nativeTools]);

  const filteredMcp = useMemo(() => {
    const query = (filter || '').trim().toLowerCase();
    if (!query) return mcpGroups;
    return mcpGroups.map((group) => ({
      ...group,
      tools: group.tools.filter((tool) => `${tool.name || ''} ${tool.description || ''} ${group.serverName}`.toLowerCase().includes(query)),
    })).filter((group) => group.tools.length > 0);
  }, [filter, mcpGroups]);

  if (!open) return null;
  const mcpCount = filteredMcp.reduce((sum, group) => sum + group.tools.length, 0);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div ref={dialogRef} role="dialog" aria-modal="true" aria-label="Active capabilities and tools" tabIndex={-1} className="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-2xl">
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-[#30363d] bg-[#0d1117] px-4">
          <div className="flex min-w-0 items-center gap-2"><Wrench size={16} className="shrink-0 text-[#3fb950]" /><h2 className="truncate text-xs font-semibold uppercase tracking-wider">Active Capabilities & Tools</h2></div>
          <button type="button" onClick={onClose} aria-label="Close tools inspector" className="icon-button"><X size={14} /></button>
        </div>
        <div className="border-b border-[#30363d] bg-[#161b22]/50 p-3">
          <div className="relative"><Search size={13} className="absolute left-3 top-2.5 text-[#6e7681]" /><input ref={filterRef} type="text" value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter active tools and schemas..." aria-label="Filter tools" className="w-full rounded-lg border border-[#30363d] bg-[#0d1117] py-1.5 pl-8 pr-3 text-xs outline-none placeholder:text-[#6e7681] focus:border-[#58a6ff]" /></div>
        </div>
        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
          {loading && <p className="text-xs text-[#8b949e]">Loading live tool catalogs…</p>}
          {errors.map((error) => <p key={error} role="alert" className="rounded border border-[#f85149]/40 bg-[#f85149]/10 p-2 text-xs text-[#ff7b72]">{error}</p>)}
          <section>
            <h3 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">Native Tools ({filteredNative.length})</h3>
            <div className="space-y-2">{filteredNative.map((tool) => <article key={tool.name} className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3"><h4 className="font-mono text-xs font-bold text-[#58a6ff]">{tool.name}</h4><p className="mb-1.5 mt-1 text-xs leading-relaxed text-[#8b949e]">{tool.description}</p><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded border border-[#30363d]/40 bg-[#161b22] p-2 font-mono text-[10px] text-[#8b949e]">{JSON.stringify(tool.parameters, null, 2)}</pre></article>)}</div>
          </section>
          {(mcpCount > 0 || (settings?.mcpServers.some((server) => server.enabled) ?? false)) && <section><h3 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">Model Context Protocol Tools ({mcpCount})</h3><div className="space-y-3">{filteredMcp.map((group) => <div key={group.serverId}><p className="mb-1 text-[11px] font-medium text-[#d29922]">{group.serverName}</p><div className="space-y-2">{group.tools.map((tool, index) => <article key={`${group.serverId}:${tool.name || index}`} className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3"><h4 className="font-mono text-xs font-bold text-[#d29922]">{tool.name || 'Unnamed MCP tool'}</h4><p className="mt-1 text-xs text-[#8b949e]">{tool.description || 'No description provided.'}</p><pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words rounded border border-[#30363d]/40 bg-[#161b22] p-2 font-mono text-[10px] text-[#8b949e]">{JSON.stringify(tool.inputSchema ?? tool.input_schema ?? {}, null, 2)}</pre></article>)}</div></div>)}</div></section>}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[#30363d] bg-[#0d1117] px-4 py-2.5 text-[11px] text-[#6e7681]"><span>Catalogs are queried from the native runtime and enabled MCP servers.</span><button type="button" onClick={onClose} className="toolbar-button">Close</button></div>
      </div>
    </div>
  );
};
