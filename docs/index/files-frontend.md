# frontend (114 files, 27551 lines, 428 tests)

src/App.lazy.test.tsx | 56L | 1T
src/App.test.tsx | 402L | 20T
src/App.tsx | 344L | App
src/components/AboutModal.test.tsx | 379L | 12T
src/components/AboutModal.tsx | 159L | AboutModal
src/components/ActivityTray.test.tsx | 166L | 4T
src/components/ActivityTray.tsx | 75L | ActivityTray
src/components/AetherLogo.test.tsx | 20L | 1T
src/components/AetherLogo.tsx | 31L | AetherLogo
src/components/ArtifactPane.test.tsx | 78L | 2T
src/components/ArtifactPane.tsx | 287L | ArtifactPane
src/components/ArtifactsLibraryModal.test.tsx | 136L | 4T
src/components/ArtifactsLibraryModal.tsx | 391L | ArtifactsLibraryModal
src/components/AttachmentPreview.test.tsx | 193L | 8T
src/components/AttachmentPreview.tsx | 103L | AttachmentPreview
src/components/BackgroundTasksCard.test.tsx | 68L | 2T
src/components/BackgroundTasksCard.tsx | 186L | BackgroundTasksCard
src/components/ChatCanvas.attachments.test.tsx | 335L | 15T
src/components/ChatCanvas.commands.test.tsx | 680L | 32T
src/components/ChatCanvas.dropPaste.test.tsx | 124L | 3T
src/components/ChatCanvas.pagination.test.tsx | 334L | 20T
src/components/ChatCanvas.reflection.test.tsx | 32L
src/components/ChatCanvas.renderHotpath.test.tsx | 137L | 4T
src/components/ChatCanvas.search.test.tsx | 238L | 9T
src/components/ChatCanvas.timeline.integration.test.tsx | 177L | 3T
src/components/ChatCanvas.tsx | 2703L | SlashItem ChatCanvas
src/components/ChatCanvas.ui.test.tsx | 70L | 3T
src/components/ContextMenu.test.tsx | 427L | 14T
src/components/ContextMenu.tsx | 1102L | ContextMenuProps ContextMenu
src/components/CouncilView.test.tsx | 58L | 1T
src/components/CouncilView.tsx | 155L | CouncilView
src/components/ErrorBoundary.tsx | 67L | ErrorBoundary
src/components/ExecutionTrace.test.tsx | 335L | 14T
src/components/ExecutionTrace.tsx | 471L | ExecutionTrace
src/components/FilesChangedCard.test.tsx | 139L | 4T
src/components/FilesChangedCard.tsx | 364L | FilesChangedCard
src/components/InteractivePlanCard.test.tsx | 163L | 9T
src/components/InteractivePlanCard.tsx | 123L | InteractivePlanCard
src/components/InteractiveTerminal.test.tsx | 73L | 4T
src/components/InteractiveTerminal.tsx | 237L | InteractiveTerminal
src/components/markdownMath.ts | 54L | remarkPreserveMathCode normalizeMathDelimiters
src/components/MarkdownRenderer.math.test.tsx | 90L | 7T
src/components/MarkdownRenderer.test.tsx | 148L | 7T
src/components/MarkdownRenderer.tsx | 240L | MarkdownRenderer
src/components/MemoryModal.test.tsx | 20L | 1T
src/components/MemoryModal.tsx | 199L | MemoryModal
src/components/MessageRow.test.tsx | 344L | 13T
src/components/MessageRow.tsx | 426L | MessageRow
src/components/ModelPicker.tsx | 355L | ModelPicker
src/components/MotionSurfaces.test.ts | 49L | 1T
src/components/OptionalPanel.focus.test.tsx | 119L | 2T
src/components/OptionalPanel.test.tsx | 57L | 2T
src/components/OptionalPanel.tsx | 59L | OptionalPanel
src/components/release-wiring.test.ts | 16L | 2T
src/components/RunStatusBar.test.tsx | 37L | 3T
src/components/RunStatusBar.tsx | 61L | RunStatusBar
src/components/SelfImprovementBadge.test.tsx | 458L | 18T
src/components/SelfImprovementBadge.tsx | 204L | SelfImprovementBadge
src/components/SessionHistoryModal.test.tsx | 85L | 5T
src/components/SessionHistoryModal.tsx | 283L | SessionHistoryModal
src/components/SettingsModal.test.tsx | 237L | 11T
src/components/SettingsModal.tsx | 833L | SettingsModal
src/components/Sidebar.test.tsx | 402L | 11T
src/components/Sidebar.tsx | 534L | Sidebar
src/components/SideBySideDiff.test.tsx | 70L | 4T
src/components/SideBySideDiff.tsx | 186L | SideBySideDiff
src/components/SkillPicker.test.tsx | 45L | 3T
src/components/SkillPicker.tsx | 58L | SkillPicker
src/components/SpotlightBar.tsx | 135L | SpotlightBar
src/components/SubagentTreeCard.test.tsx | 106L | 7T
src/components/SubagentTreeCard.tsx | 115L | SubagentTreeCard
src/components/TerminalOutput.test.tsx | 34L | 1T
src/components/TerminalOutput.tsx | 141L | TerminalOutput
src/components/TimelineScrubber.test.tsx | 307L | 10T
src/components/TimelineScrubber.tsx | 357L | TimelineScrubber
src/components/ToolsInspectorModal.test.tsx | 41L | 1T
src/components/ToolsInspectorModal.tsx | 125L | ToolsInspectorModal
src/hooks/useAgent.cache.test.tsx | 99L | 4T
src/hooks/useAgent.committed-ack.test.tsx | 424L | 4T
src/hooks/useAgent.council.test.tsx | 126L
src/hooks/useAgent.delta-checkpoint.test.tsx | 413L | 12T
src/hooks/useAgent.history-conflicts.test.tsx | 277L | 1T
src/hooks/useAgent.images.test.tsx | 454L | 13T
src/hooks/useAgent.journal-lifecycle.test.tsx | 207L | 3T
src/hooks/useAgent.native-revision.test.tsx | 183L | 1T
src/hooks/useAgent.navigation.test.tsx | 123L | 3T
src/hooks/useAgent.pagination.test.tsx | 352L | 16T
src/hooks/useAgent.reflection.test.tsx | 928L | 29T
src/hooks/useAgent.revision-races.test.tsx | 241L | 4T
src/hooks/useAgent.run-status.test.tsx | 134L | 7T
src/hooks/useAgent.skills.test.tsx | 189L | 3T
src/hooks/useAgent.stop-journal.test.tsx | 149L | 1T
src/hooks/useAgent.streaming-performance.test.tsx | 269L | 3T
src/hooks/useAgent.tasks.test.tsx | 87L | 3T
src/hooks/useAgent.ts | 2571L | useAgent
src/hooks/useAgent.update-readiness.test.tsx | 135L | 5T
src/hooks/useAgentLiveSession.ts | 75L | RunPhase RunOutcome RunStatus LiveSession liveSession
src/hooks/useHistoryPreview.test.tsx | 123L | 5T
src/hooks/useHistoryPreview.ts | 73L | useHistoryPreview
src/hooks/useIsNarrowViewport.ts | 25L | useIsNarrowViewport
src/hooks/useModalAccessibility.ts | 75L | useModalAccessibility
src/hooks/useSpeechRecognition.test.tsx | 104L | 3T
src/hooks/useSpeechRecognition.ts | 118L | useSpeechRecognition
src/index.css | 118L
src/main.tsx | 14L
src/test/sessionPaging.ts | 67L | page pagingBackend
src/test/setup.ts | 22L
src/types/agent.ts | 357L | Role ProviderKind ReasoningEffort ToolStatus SessionActivity ArtifactType FileDiffStat ToolCallItem AttachedContext ImageInput CouncilCandidate CouncilTurn SelfImprovementAction SelfImprovementChange SelfImprovementEvent MessageSegment
src/ui-motion.test.ts | 50L | 3T
src/utils/audioChime.ts | 99L | playTaskCompleteChime showDesktopNotification
src/utils/encoding.test.ts | 33L
src/utils/encoding.ts | 7L | safeEncodeURIComponent
src/utils/sessionCacheBudget.test.ts | 78L | 7T
src/utils/sessionCacheBudget.ts | 104L | SESSION_CACHE_BUDGET estimateRetainedBytes pruneSessionCache SessionLoadMetric recordSessionLoad getSessionPerformanceSnapshot scheduleSessionIndexWarmup
