import React, { useState } from 'react';
import { X, Copy, Check, Terminal, FileCode, ExternalLink } from 'lucide-react';
import type { Artifact } from '../types/agent';

interface ArtifactPaneProps {
  artifacts: Artifact[];
  activeArtifactId: string | null;
  onSelectArtifact: (id: string) => void;
  onClose: () => void;
}

export const ArtifactPane: React.FC<ArtifactPaneProps> = ({
  artifacts,
  activeArtifactId,
  onSelectArtifact,
  onClose,
}) => {
  const [copied, setCopied] = useState(false);

  const active = artifacts.find((a) => a.id === activeArtifactId) || artifacts[0];

  if (!active) return null;

  const handleCopy = () => {
    navigator.clipboard.writeText(active.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const lines = active.content.split('\n');

  return (
    <aside className="w-96 lg:w-[460px] bg-[#0d1117] border-l border-[#30363d] flex flex-col h-full shadow-2xl select-none">
      {/* Pane Header */}
      <div className="p-3 border-b border-[#30363d] flex items-center justify-between bg-[#161b22]">
        <div className="flex items-center gap-2 truncate">
          {active.type === 'terminal' ? (
            <Terminal className="w-4 h-4 text-emerald-400 shrink-0" />
          ) : (
            <FileCode className="w-4 h-4 text-cyan-400 shrink-0" />
          )}
          <span className="text-xs font-semibold text-white truncate">{active.title}</span>
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={handleCopy}
            className="p-1.5 rounded hover:bg-[#21262d] text-slate-400 hover:text-white transition-colors"
            title="Copy content"
          >
            {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
          </button>
          <button
            onClick={onClose}
            className="p-1.5 rounded hover:bg-[#21262d] text-slate-400 hover:text-white transition-colors"
            title="Close Sidecar Pane"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Artifacts Tab Strip (if multiple) */}
      {artifacts.length > 1 && (
        <div className="flex items-center gap-1 px-2 py-1.5 border-b border-[#30363d] bg-[#0d1117] overflow-x-auto">
          {artifacts.map((art) => {
            const isSelected = art.id === active.id;
            return (
              <button
                key={art.id}
                onClick={() => onSelectArtifact(art.id)}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-[11px] whitespace-nowrap transition-colors ${
                  isSelected
                    ? 'bg-[#21262d] text-cyan-400 font-medium border border-[#30363d]'
                    : 'text-slate-400 hover:bg-[#161b22] hover:text-slate-200'
                }`}
              >
                <span>{art.title}</span>
              </button>
            );
          })}
        </div>
      )}

      {/* File Path Bar */}
      {active.filePath && (
        <div className="px-3 py-1.5 bg-[#161b22]/50 border-b border-[#30363d] flex items-center justify-between text-[10px] text-slate-400 font-mono">
          <span className="truncate" title={active.filePath}>
            {active.filePath}
          </span>
          <ExternalLink className="w-3 h-3 shrink-0 ml-1 opacity-70" />
        </div>
      )}

      {/* Code / Content Viewer */}
      <div className="flex-1 overflow-auto bg-[#0b0f17] font-mono text-[11px] leading-relaxed p-2 select-text">
        <table className="w-full border-collapse">
          <tbody>
            {lines.map((line, idx) => (
              <tr key={idx} className="hover:bg-[#161b22]/70 group">
                <td className="w-10 pr-3 text-right text-slate-400 select-none group-hover:text-slate-400 align-top">
                  {idx + 1}
                </td>
                <td className="text-slate-200 whitespace-pre break-all pl-2">
                  {line || ' '}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </aside>
  );
};
