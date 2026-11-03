import { useState } from 'react';
import { Sidebar } from './components/Sidebar';
import { ChatCanvas } from './components/ChatCanvas';
import { ArtifactPane } from './components/ArtifactPane';
import { SettingsModal } from './components/SettingsModal';
import { useAgent } from './hooks/useAgent';

export function App() {
  const {
    messages,
    artifacts,
    activeArtifactId,
    setActiveArtifactId,
    isStreaming,
    sessions,
    currentSessionId,
    settings,
    workspaceCwd,
    sendMessage,
    stopAgent,
    newSession,
    switchSession,
    updateSettings,
  } = useAgent();

  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isArtifactOpen, setIsArtifactOpen] = useState(true);

  const handleSelectArtifact = (id: string) => {
    setActiveArtifactId(id);
    setIsArtifactOpen(true);
  };

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#0b0f17] text-slate-100 antialiased select-none font-sans">
      {/* 1. Left Sidebar */}
      <Sidebar
        sessions={sessions}
        currentSessionId={currentSessionId}
        onNewSession={newSession}
        onSelectSession={switchSession}
        onOpenSettings={() => setIsSettingsOpen(true)}
        workspaceCwd={workspaceCwd}
        settings={settings}
      />

      {/* 2. Main Chat Canvas (Virtualized 60 FPS) */}
      <main className="flex-1 flex flex-col h-full overflow-hidden relative">
        <ChatCanvas
          messages={messages}
          isStreaming={isStreaming}
          onSendMessage={sendMessage}
          onStop={stopAgent}
          onSelectArtifact={handleSelectArtifact}
        />
      </main>

      {/* 3. Right Artifact Sidecar Panel */}
      {isArtifactOpen && artifacts.length > 0 && (
        <ArtifactPane
          artifacts={artifacts}
          activeArtifactId={activeArtifactId}
          onSelectArtifact={setActiveArtifactId}
          onClose={() => setIsArtifactOpen(false)}
        />
      )}

      {/* 4. Settings Modal */}
      {isSettingsOpen && settings && (
        <SettingsModal
          settings={settings}
          onSave={updateSettings}
          onClose={() => setIsSettingsOpen(false)}
        />
      )}
    </div>
  );
}

export default App;
