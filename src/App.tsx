import { useCallback, useEffect, useState } from 'react';
import { AlertCircle, X } from 'lucide-react';
import { ArtifactPane } from './components/ArtifactPane';
import { ArtifactsLibraryModal } from './components/ArtifactsLibraryModal';
import { ChatCanvas } from './components/ChatCanvas';
import { InteractiveTerminal } from './components/InteractiveTerminal';
import { MemoryModal } from './components/MemoryModal';
import { ModelPicker } from './components/ModelPicker';
import { SessionHistoryModal } from './components/SessionHistoryModal';
import { SettingsModal } from './components/SettingsModal';
import { Sidebar } from './components/Sidebar';
import { SpotlightBar } from './components/SpotlightBar';
import { ToolsInspectorModal } from './components/ToolsInspectorModal';
import { useAgent } from './hooks/useAgent';
import { useIsNarrowViewport } from './hooks/useIsNarrowViewport';

export function App() {
  const agent = useAgent();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [artifactOpen, setArtifactOpen] = useState(false);
  const [artifactsLibraryOpen, setArtifactsLibraryOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [spotlightOpen, setSpotlightOpen] = useState(false);
  const [liveShellOpen, setLiveShellOpen] = useState(false);
  const isNarrowViewport = useIsNarrowViewport();
  const [sidebarOpen, setSidebarOpen] = useState(() => !isNarrowViewport);

  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const openModelPicker = useCallback(() => setModelOpen(true), []);
  const openArtifactsLibrary = useCallback(() => setArtifactsLibraryOpen(true), []);
  const openHistory = useCallback(() => setHistoryOpen(true), []);
  const openTools = useCallback(() => setToolsOpen(true), []);
  const openMemory = useCallback(() => setMemoryOpen(true), []);
  const openSpotlight = useCallback(() => setSpotlightOpen(true), []);
  const openLiveShell = useCallback(() => setLiveShellOpen(true), []);
  const toggleSidebar = useCallback(() => setSidebarOpen((value) => !value), []);

  // Background tasks / artifacts do not auto-force the pane open.
  // The top-right task bubble alerts the user, and clicking it opens the overlay drawer.

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && isNarrowViewport && sidebarOpen) {
        setSidebarOpen(false);
        return;
      }
      if (!event.ctrlKey) return;
      if (event.key.toLowerCase() === 'n') {
        event.preventDefault();
        agent.newSession();
      } else if (event.key.toLowerCase() === 'k') {
        event.preventDefault();
        openModelPicker();
      } else if (event.key === ',') {
        event.preventDefault();
        openSettings();
      } else if (event.key.toLowerCase() === 'l' && event.shiftKey) {
        event.preventDefault();
        openArtifactsLibrary();
      } else if (event.key.toLowerCase() === 'b') {
        event.preventDefault();
        toggleSidebar();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [agent.newSession, openModelPicker, openSettings, openArtifactsLibrary, toggleSidebar, isNarrowViewport, sidebarOpen]);

  const selectArtifact = useCallback((id: string) => {
    agent.setActiveArtifactId(id);
    setArtifactOpen(true);
  }, [agent.setActiveArtifactId]);

  const selectSession = useCallback((sessionId: string) => {
    void agent.switchSession(sessionId);
    if (isNarrowViewport) setSidebarOpen(false);
  }, [agent.switchSession, isNarrowViewport]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#0d1117] text-[#e6edf3]">
      {sidebarOpen && isNarrowViewport && (
        <button
          type="button"
          aria-label="Close sidebar"
          onClick={() => setSidebarOpen(false)}
          className="fixed inset-0 z-30 bg-black/50"
        />
      )}
      {sidebarOpen && (
        <Sidebar
          sessions={agent.sessions}
          currentSessionId={agent.currentSessionId}
          onNewSession={agent.newSession}
          onSelectSession={selectSession}
          onDeleteSession={agent.deleteSession}
          onRenameSession={agent.renameSession}
          onTogglePinSession={agent.togglePinSession}
          onDeepSearch={agent.deepSearch}
          onExportSession={agent.exportSession}
          onOpenSettings={openSettings}
          onOpenArtifactsLibrary={openArtifactsLibrary}
          workspaceCwd={agent.workspaceCwd}
          onSelectWorkspace={agent.selectWorkspace}
          onClearWorkspace={agent.clearWorkspace}
          effectiveConfig={agent.effectiveConfig}
          overlay={isNarrowViewport}
          gitStatus={agent.gitStatus}
          onRefreshGitStatus={agent.refreshGitStatus}
          onCommitWorkspace={agent.commitWorkspaceChanges}
        />
      )}
      <main className="flex h-full min-w-0 flex-1 flex-col">
        <ChatCanvas
          messages={agent.messages}
          isStreaming={agent.isStreaming}
          stopRequested={agent.stopRequested}
          onSendMessage={agent.sendMessage}
          onStop={agent.stopAgent}
          onSelectArtifact={selectArtifact}
          onClearSession={agent.clearCurrentSession}
          onNewSession={agent.newSession}
          onOpenSettings={openSettings}
          onOpenModelPicker={openModelPicker}
          effectiveConfig={agent.effectiveConfig}
          sessionTitle={agent.sessionTitle}
          artifactCount={agent.artifacts.length}
          isArtifactOpen={artifactOpen && agent.artifacts.length > 0}
          onToggleArtifacts={() => setArtifactOpen((value) => !value)}
          onListFiles={agent.listWorkspaceFiles}
          onReviewSession={agent.reviewSession}
          isReviewing={agent.isReviewing}
          onCompactSession={agent.compactSession}
          onUndoTurn={agent.undoTurn}
          onRetryTurn={agent.retryTurn}
          onEditAndResendMessage={agent.editAndResendMessage}
          onRollbackToMessage={agent.rollbackToMessage}
          onInitAgentsMd={agent.initAgentsMd}
          onExportSession={(format, destination) => { void agent.exportSession(agent.currentSessionId, format, destination); }}
          onSelectCouncilWinner={agent.selectCouncilWinner}
          onRenameSession={agent.renameSession}
          currentSessionId={agent.currentSessionId}
          settings={agent.settings}
          onUpdateOverride={(override) => { void agent.updateRunConfig(override); }}
          onSelectModel={(providerId, model) => { void agent.selectModel(providerId, model); }}
          subagents={agent.subagents}
          activeSkills={agent.activeSkills}
          onListSkills={agent.listSkills}
          onOpenHistory={openHistory}
          onOpenTools={openTools}
          onOpenMemory={openMemory}
          onOpenSpotlight={openSpotlight}
          onOpenLiveShell={openLiveShell}
          sidebarOpen={sidebarOpen}
          onToggleSidebar={toggleSidebar}
        />
      </main>
      {artifactOpen && agent.artifacts.length > 0 && (
        <ArtifactPane
          artifacts={agent.artifacts}
          activeArtifactId={agent.activeArtifactId}
          onSelectArtifact={agent.setActiveArtifactId}
          onClose={() => setArtifactOpen(false)}
          overlay={true}
        />
      )}

      {modelOpen && agent.settings && agent.effectiveConfig && (
        <ModelPicker
          open={modelOpen}
          settings={agent.settings}
          effective={agent.effectiveConfig}
          override={agent.modelOverride}
          onClose={() => setModelOpen(false)}
          onSelect={agent.selectModel}
          onUpdateOverride={agent.updateRunConfig}
          onClearOverride={agent.clearModelOverride}
          onToggleFavorite={agent.toggleFavorite}
          onAddCustomModel={agent.addCustomModel}
          onRefreshModels={agent.refreshModels}
        />
      )}

      {artifactsLibraryOpen && (
        <ArtifactsLibraryModal
          open={artifactsLibraryOpen}
          onClose={() => setArtifactsLibraryOpen(false)}
          onLoadArtifacts={agent.listAllArtifacts}
          onSelectSession={agent.switchSession}
        />
      )}

      {settingsOpen && agent.settings && (
        <SettingsModal
          settings={agent.settings}
          onSave={agent.updateSettings}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {historyOpen && (
        <SessionHistoryModal
          open={historyOpen}
          onClose={() => setHistoryOpen(false)}
          sessionTitle={agent.sessionTitle}
          sessionId={agent.currentSessionId ?? ''}
          messages={agent.messages}
          onCompact={() => { void agent.compactSession(); }}
          onExport={(format) => { void agent.exportSession(agent.currentSessionId, format); }}
        />
      )}

      {toolsOpen && agent.settings && (
        <ToolsInspectorModal
          open={toolsOpen}
          onClose={() => setToolsOpen(false)}
          settings={agent.settings}
        />
      )}

      {memoryOpen && (
        <MemoryModal
          open={memoryOpen}
          onClose={() => setMemoryOpen(false)}
          workspaceCwd={agent.workspaceCwd}
          onReadMemory={agent.readAgentsMd}
          onSaveMemory={agent.saveAgentsMd}
          onInitMemory={agent.initAgentsMd}
        />
      )}

      {spotlightOpen && (
        <SpotlightBar
          isOpen={spotlightOpen}
          onClose={() => setSpotlightOpen(false)}
          onSubmitPrompt={agent.sendMessage}
        />
      )}

      {liveShellOpen && (
        <div className="relative h-full w-full max-w-sm shrink-0 border-l border-[#30363d]">
          <button
            type="button"
            aria-label="Close live shell"
            onClick={() => setLiveShellOpen(false)}
            className="absolute right-2 top-2 z-10 rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3]"
          >
            <X size={14} />
          </button>
          <InteractiveTerminal />
        </div>
      )}

      {agent.lastError && (
        <div className="fixed bottom-4 right-4 z-50 flex max-w-md items-center gap-3 rounded-lg border border-[#f85149]/40 bg-[#161b22] px-4 py-3 text-sm text-[#f85149] shadow-xl">
          <AlertCircle size={18} className="shrink-0" />
          <p className="flex-1 text-xs leading-relaxed">{agent.lastError}</p>
          <button
            onClick={agent.clearError}
            className="rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#f85149]"
          >
            <X size={14} />
          </button>
        </div>
      )}
      {agent.isLoading && (
        <div className="fixed bottom-3 left-1/2 z-40 -translate-x-1/2 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-[10px] text-[#8b949e]">
          Loading local state…
        </div>
      )}
    </div>
  );
}

export default App;