import { lazy, useCallback, useEffect, useState } from 'react';
import { AlertCircle, X } from 'lucide-react';
import { ChatCanvas } from './components/ChatCanvas';
import { ContextMenu } from './components/ContextMenu';
import { Sidebar } from './components/Sidebar';
import { OptionalPanel } from './components/OptionalPanel';
import { useAgent } from './hooks/useAgent';
import { useIsNarrowViewport } from './hooks/useIsNarrowViewport';

const ArtifactPane = lazy(() => import('./components/ArtifactPane').then((module) => ({ default: module.ArtifactPane })));
const ArtifactsLibraryModal = lazy(() => import('./components/ArtifactsLibraryModal').then((module) => ({ default: module.ArtifactsLibraryModal })));
const InteractiveTerminal = lazy(() => import('./components/InteractiveTerminal').then((module) => ({ default: module.InteractiveTerminal })));
const MemoryModal = lazy(() => import('./components/MemoryModal').then((module) => ({ default: module.MemoryModal })));
const ModelPicker = lazy(() => import('./components/ModelPicker').then((module) => ({ default: module.ModelPicker })));
const SessionHistoryModal = lazy(() => import('./components/SessionHistoryModal').then((module) => ({ default: module.SessionHistoryModal })));
const SettingsModal = lazy(() => import('./components/SettingsModal').then((module) => ({ default: module.SettingsModal })));
const SpotlightBar = lazy(() => import('./components/SpotlightBar').then((module) => ({ default: module.SpotlightBar })));
const ToolsInspectorModal = lazy(() => import('./components/ToolsInspectorModal').then((module) => ({ default: module.ToolsInspectorModal })));

const AboutModal = lazy(() => import('./components/AboutModal').then((module) => ({ default: module.AboutModal })));

export function App() {
  const agent = useAgent();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
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
  const [rightPaneWidth, setRightPaneWidth] = useState<number>(() => {
    try {
      const saved = Number(window.localStorage.getItem('aether.rightPaneWidth'));
      return Number.isFinite(saved) && saved >= 280 ? Math.min(720, saved) : 420;
    } catch {
      return 420;
    }
  });
  const [isResizingRightPane, setIsResizingRightPane] = useState(false);

  useEffect(() => {
    if (!isResizingRightPane) return;
    const onMove = (event: MouseEvent) => {
      const max = Math.min(720, Math.floor(window.innerWidth * 0.55));
      const next = Math.max(280, Math.min(max, window.innerWidth - event.clientX));
      setRightPaneWidth(Math.round(next));
    };
    const onUp = () => setIsResizingRightPane(false);
    document.body.style.cursor = 'col-resize';
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      document.body.style.cursor = '';
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [isResizingRightPane]);

  useEffect(() => {
    if (isResizingRightPane) return;
    try {
      window.localStorage.setItem('aether.rightPaneWidth', String(rightPaneWidth));
    } catch {
      // Ignore restricted storage in sandboxed environments.
    }
  }, [rightPaneWidth, isResizingRightPane]);

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
      if (event.defaultPrevented || document.querySelector('[role="dialog"][aria-modal="true"]')) return;
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

  const hasRunningTasks = Boolean(agent.subagents?.some((s) => s.status === 'running'));
  const canOpenInspector = (agent.artifacts?.length ?? 0) > 0 || hasRunningTasks;

  useEffect(() => {
    if (artifactOpen && !canOpenInspector) {
      setArtifactOpen(false);
    }
  }, [artifactOpen, canOpenInspector]);

  const selectArtifact = useCallback((id: string) => {
    agent.setActiveArtifactId(id);
    setArtifactOpen(true);
  }, [agent.setActiveArtifactId]);

  const selectSession = useCallback((sessionId: string) => {
    void agent.switchSession(sessionId);
    if (isNarrowViewport) setSidebarOpen(false);
  }, [agent.switchSession, isNarrowViewport]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-ink-primary">
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
          sessionActivities={agent.sessionActivities}
          currentSessionId={agent.currentSessionId}
          onNewSession={agent.newSession}
          onSelectSession={selectSession}
          onDeleteSession={agent.deleteSession}
          onRenameSession={agent.renameSession}
          onAutoTitleSession={agent.generateSessionTitle}
          onTogglePinSession={agent.togglePinSession}
          onDeepSearch={agent.deepSearch}
          onOpenSearchResult={agent.openSearchResult}
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
        {agent.isHistoryPreview && (
          <div role="status" className="flex items-center justify-between border-b border-hairline bg-surface px-4 py-2 text-xs">
            <span>History preview — read-only — live work continues in the background</span>
            <button type="button" onClick={agent.loadLatestMessages} className="rounded border border-accent px-3 py-1">Return to latest</button>
          </div>
        )}
        <ChatCanvas
          workspaceCwd={agent.workspaceCwd}
          onOpenFolder={() => { void agent.selectWorkspace(); }}
          messages={agent.messages}
          isHistoryPreview={agent.isHistoryPreview}
          jumpToMessage={agent.jumpToMessage}
          isSessionLoading={agent.isSessionLoading}
          loadingOlderMessages={agent.loadingOlderMessages}
          onLoadOlderMessages={agent.loadOlderMessages}
          onLoadLatestMessages={agent.loadLatestMessages}
          hasOlderMessages={agent.hasOlderMessages}
          totalMessageCount={agent.totalMessageCount}
          historyError={agent.historyError}
          runStatus={agent.runStatus}
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
          isArtifactOpen={artifactOpen && canOpenInspector}
          onToggleArtifacts={() => setArtifactOpen((value) => !value)}
          onListFiles={agent.listWorkspaceFiles}
          onReviewSession={agent.isHistoryPreview ? undefined : agent.reviewSession}
          isReviewing={agent.isReviewing}
          onCompactSession={agent.isHistoryPreview ? undefined : agent.compactSession}
          onUndoTurn={agent.isHistoryPreview ? undefined : agent.undoTurn}
          onRetryTurn={agent.isHistoryPreview ? undefined : agent.retryTurn}
          onEditAndResendMessage={agent.isHistoryPreview ? undefined : agent.editAndResendMessage}
          onRollbackToMessage={agent.isHistoryPreview ? undefined : agent.rollbackToMessage}
          onInitAgentsMd={agent.initAgentsMd}
          onExportSession={(format, destination) => { void agent.exportSession(agent.currentSessionId, format, destination); }}
          onSelectCouncilWinner={agent.isHistoryPreview ? undefined : agent.selectCouncilWinner}
          onRenameSession={agent.renameSession}
          currentSessionId={agent.currentSessionId}
          settings={agent.settings}
          onUpdateOverride={(override) => { void agent.updateRunConfig(override); }}
          onSelectModel={(providerId, model) => { void agent.selectModel(providerId, model); }}
          subagents={agent.subagents}
          backgroundTasks={agent.backgroundTasks}
          onKillBackgroundTask={agent.killBackgroundTask}
          onRefreshBackgroundTasks={agent.refreshBackgroundTasks}
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
      {artifactOpen && canOpenInspector && !isNarrowViewport && (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize side pane"
          aria-valuenow={rightPaneWidth}
          aria-valuemin={280}
          aria-valuemax={720}
          tabIndex={0}
          onMouseDown={(e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            setIsResizingRightPane(true);
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowLeft') {
              e.preventDefault();
              setRightPaneWidth((w) => Math.min(720, w + 24));
            }
            if (e.key === 'ArrowRight') {
              e.preventDefault();
              setRightPaneWidth((w) => Math.max(280, w - 24));
            }
          }}
          className={`w-1 shrink-0 cursor-col-resize transition-colors ${isResizingRightPane ? 'bg-[var(--ui-accent)]' : 'bg-transparent hover:bg-[var(--ui-accent)]/60'}`}
          title="Drag to resize side pane"
        />
      )}
      {artifactOpen && canOpenInspector && (
        <OptionalPanel label="Artifacts" onClose={() => setArtifactOpen(false)}>
          <ArtifactPane
            artifacts={agent.artifacts}
            activeArtifactId={agent.activeArtifactId}
            onSelectArtifact={agent.setActiveArtifactId}
            onClose={() => setArtifactOpen(false)}
            overlay={isNarrowViewport}
            width={isNarrowViewport ? undefined : rightPaneWidth}
          />
        </OptionalPanel>
      )}

      {modelOpen && agent.settings && agent.effectiveConfig && (
        <OptionalPanel label="Model picker" onClose={() => setModelOpen(false)}>
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
        </OptionalPanel>
      )}

      {artifactsLibraryOpen && (
        <OptionalPanel label="Artifacts library" onClose={() => setArtifactsLibraryOpen(false)}>
          <ArtifactsLibraryModal
            open={artifactsLibraryOpen}
            onClose={() => setArtifactsLibraryOpen(false)}
            onLoadArtifacts={agent.listAllArtifacts}
            onSelectSession={agent.switchSession}
          />
        </OptionalPanel>
      )}

      {settingsOpen && agent.settings && (
        <OptionalPanel label="Settings" onClose={() => setSettingsOpen(false)}>
          <SettingsModal
            settings={agent.settings}
            onSave={agent.updateSettings}
            onAbout={() => { setSettingsOpen(false); setAboutOpen(true); }}
            onClose={() => setSettingsOpen(false)}
          />
        </OptionalPanel>
      )}

      {aboutOpen && <OptionalPanel label="About AetherAgent" onClose={() => setAboutOpen(false)}><AboutModal onClose={() => setAboutOpen(false)} onBeforeInstall={agent.prepareForUpdate} /></OptionalPanel>}

      {historyOpen && (
        <OptionalPanel label="Session history" onClose={() => setHistoryOpen(false)}>
          <SessionHistoryModal
            open={historyOpen}
            onClose={() => setHistoryOpen(false)}
            sessionTitle={agent.sessionTitle}
            sessionId={agent.currentSessionId ?? ''}
            messages={agent.messages}
            onCompact={() => { void agent.compactSession(); }}
            onExport={(format) => { void agent.exportSession(agent.currentSessionId, format); }}
          />
        </OptionalPanel>
      )}

      {toolsOpen && agent.settings && (
        <OptionalPanel label="Tools" onClose={() => setToolsOpen(false)}>
          <ToolsInspectorModal
            open={toolsOpen}
            onClose={() => setToolsOpen(false)}
            settings={agent.settings}
          />
        </OptionalPanel>
      )}

      {memoryOpen && (
        <OptionalPanel label="Memory" onClose={() => setMemoryOpen(false)}>
          <MemoryModal
            open={memoryOpen}
            onClose={() => setMemoryOpen(false)}
            workspaceCwd={agent.workspaceCwd}
            onReadMemory={agent.readAgentsMd}
            onSaveMemory={agent.saveAgentsMd}
            onInitMemory={agent.initAgentsMd}
          />
        </OptionalPanel>
      )}

      {spotlightOpen && (
        <OptionalPanel label="Spotlight" onClose={() => setSpotlightOpen(false)}>
          <SpotlightBar
            isOpen={spotlightOpen}
            onClose={() => setSpotlightOpen(false)}
            onSubmitPrompt={agent.sendMessage}
          />
        </OptionalPanel>
      )}

      {liveShellOpen && (
        <div className="fixed inset-y-0 right-0 z-40 w-full max-w-sm border-l border-[var(--stroke-nous)] bg-[var(--ui-bg-chrome)] shadow-nous">
          <button
            type="button"
            aria-label="Close live shell"
            onClick={() => setLiveShellOpen(false)}
            className="absolute right-2 top-2 z-10 rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3]"
          >
            <X size={14} />
          </button>
          <OptionalPanel label="Live shell" onClose={() => setLiveShellOpen(false)}>
            <InteractiveTerminal />
          </OptionalPanel>
        </div>
      )}

      {agent.lastError && (
        <div role="alert" className="fixed bottom-4 right-4 z-50 flex max-w-md items-center gap-3 rounded-xl border border-[var(--stroke-nous)] bg-[var(--ui-bg-elevated)] px-4 py-3 text-sm text-[var(--ui-red)] shadow-nous">
          <AlertCircle size={18} className="shrink-0" />
          <p className="flex-1 text-xs leading-relaxed">{agent.lastError}</p>
          <button
            aria-label="Dismiss error"
            onClick={agent.clearError}
            className="rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#f85149]"
          >
            <X size={14} />
          </button>
        </div>
      )}
      {agent.isLoading && (
        <div className="fixed bottom-3 left-1/2 z-40 -translate-x-1/2 rounded-lg border border-[var(--stroke-nous)] bg-[var(--ui-bg-elevated)] px-3 py-1.5 text-[10px] text-[var(--ui-text-tertiary)] shadow-nous">
          Loading local state…
        </div>
      )}

      <ContextMenu
        workspaceCwd={agent.workspaceCwd}
        currentSessionId={agent.currentSessionId}
        onNewSession={agent.newSession}
        onOpenSettings={openSettings}
        onOpenModelPicker={openModelPicker}
        onToggleSidebar={toggleSidebar}
        onExportSession={(format) => { void agent.exportSession(agent.currentSessionId, format); }}
        messages={agent.messages}
        onRetryTurn={agent.isHistoryPreview ? undefined : agent.retryTurn}
        onEditAndResendMessage={agent.isHistoryPreview ? undefined : agent.editAndResendMessage}
        onRollbackToMessage={agent.isHistoryPreview ? undefined : agent.rollbackToMessage}
        onRefreshGitStatus={agent.refreshGitStatus}
      />
    </div>
  );
}

export default App;
