# rune (62 files, 23939 lines, 217 tests)

rune/.cargo/config.toml | 3L
rune/Cargo.toml | 7L
rune/crates/aether-core/Cargo.toml | 39L
rune/crates/aether-core/src/agent.rs | 64L | f:create_hidden_command
rune/crates/aether-core/src/lib.rs | 95L | 3T
rune/crates/aether-core/src/service.rs | 1630L | 2T | T:Backend f:new f:dispatch f:supported_commands f:resolve_open_target
rune/crates/aether-core/src/updater.rs | 316L | T:UpdateCheck T:UpdateOutcome T:Updater f:disabled f:compiled f:status f:check f:install
rune/crates/aether-core/tests/service_dispatch.rs | 497L | 14T
rune/crates/aether-core/tests/skill_profile.rs | 283L | 5T
rune/crates/aether-rune/Cargo.toml | 23L
rune/crates/aether-rune/DESKTOP_INTEGRATION.md | 16L
rune/crates/aether-rune/RICH_CONTENT.md | 93L
rune/crates/aether-rune/SHELL_INTEGRATION.md | 8L
rune/crates/aether-rune/src/app.rs | 454L | T:AppState T:ShellState f:new f:configuration_ready f:configuration_error f:retry_configuration f:set_error f:new_session f:send f:candidate_a f:council_choices f:candidate_b f:cycle_candidate_b f:add_attachments f:on_key f:spec
rune/crates/aether-rune/src/cards_tests.rs | 458L | 9T
rune/crates/aether-rune/src/cards.rs | 842L | T:PlanStatus T:PlanItem T:PlanBlock T:PlanChange f:trace_key f:trace_open f:execution_trace f:reasoning f:subagents f:plan_blocks f:apply_plan_change f:render_content
rune/crates/aether-rune/src/cli.rs | 47L | T:Options f:parse
rune/crates/aether-rune/src/completion.rs | 243L | T:Kind T:Token T:Suggestion T:CompletionState f:token f:input_changed f:sync f:on_key f:render
rune/crates/aether-rune/src/composer.rs | 184L | f:is_local f:render
rune/crates/aether-rune/src/desktop.rs | 370L | 6T | T:DesktopState f:should_register_shortcut f:install_native f:set_waker f:sync f:palette_focus_return f:toggle_history
rune/crates/aether-rune/src/export.rs | 317L | T:Confirmation T:ExportState f:handle f:sync f:render
rune/crates/aether-rune/src/lib.rs | 16L
rune/crates/aether-rune/src/lifecycle.rs | 213L | T:Lifecycle f:close_group f:on_close f:on_quit f:on_focus f:poll f:advance f:retry f:keep_open f:close_without_saving f:render
rune/crates/aether-rune/src/main.rs | 94L
rune/crates/aether-rune/src/native.rs | 396L | T:Origin T:PendingAttachment T:Pending T:CachedImage T:NativeState f:new f:preparing f:image f:cached_bytes f:cancel f:origin f:image_candidate f:attach_paths f:choose_attachment f:choose_workspace f:paste
rune/crates/aether-rune/src/panels.rs | 2200L | T:Panel T:UiState f:title f:set_form f:form f:pending f:open f:sync f:conversation_action f:render f:wrap_message f:message_menu
rune/crates/aether-rune/src/panels/domain.rs | 248L | 5T | f:settings_document f:redact f:object f:scope f:code f:needs_confirmation f:plain_text
rune/crates/aether-rune/src/panels/PANEL_INTEGRATION.md | 22L
rune/crates/aether-rune/src/panels/tests.rs | 538L | 17T
rune/crates/aether-rune/src/rich_content_tests.rs | 335L | 15T
rune/crates/aether-rune/src/rich_content.rs | 734L | T:Formula T:Document T:TaskChange T:MathStatus T:RichContent f:markdown f:parse f:original_offset f:apply_task_change f:new f:take_changed f:document f:render f:render_with_tasks f:render_formula_svg
rune/crates/aether-rune/src/statequeue.rs | 468L | T:QueuedInstruction T:SessionQueue T:QueueState T:QueueEdit f:total f:bytes f:push f:remove f:enqueue f:sync f:save_edit f:render f:modal
rune/crates/aether-rune/src/view.rs | 540L | f:view
rune/crates/aether-rune/src/widgets.rs | 42L | f:button f:caption f:card
rune/crates/aether-rune/tests/cli.rs | 13L | 1T
rune/crates/aether-rune/tests/completion.rs | 103L | 10T
rune/crates/aether-rune/tests/composer.rs | 737L | 11T
rune/crates/aether-rune/tests/desktop.rs | 79L | 2T
rune/crates/aether-rune/tests/export.rs | 422L | 13T
rune/crates/aether-rune/tests/lifecycle.rs | 222L | 8T
rune/crates/aether-rune/tests/message_edit.rs | 124L | 2T
rune/crates/aether-rune/tests/shell.rs | 585L | 19T
rune/crates/aether-rune/tests/startup_contract.rs | 13L | 1T
rune/crates/aether-state/Cargo.toml | 17L
rune/crates/aether-state/INTEGRATION.md | 94L
rune/crates/aether-state/src/commands.rs | 325L | T:UiAction T:InputAction T:SendOptions f:prompt_options f:execute_input f:route_input
rune/crates/aether-state/src/controller.rs | 3581L | 42T | T:Controller f:new f:new_with_workspace f:with_runtime f:with_backend f:from_backend f:ready_to_send f:selected_run_active f:send_captured f:navigation_generation f:model f:model_mut f:set_waker f:take_result f:new_session f:open_session
rune/crates/aether-state/src/events.rs | 105L | 1T | T:BackendEvent T:EventQueue f:new f:set_waker f:wake
rune/crates/aether-state/src/history.rs | 526L | T:HistoryAction T:ReplayPrompt f:undo f:undo_turn f:clear_history f:rollback_turn f:retry_turn f:edit_and_resend f:history
rune/crates/aether-state/src/lib.rs | 14L
rune/crates/aether-state/src/message_edit.rs | 166L | 2T | f:update_message_content
rune/crates/aether-state/src/model.rs | 237L | T:RunPhase T:RunStatus T:CommandResult T:CompletedRun T:Model f:new f:take_completion f:changed f:protected
rune/crates/aether-state/src/reducer.rs | 306L | 1T
rune/crates/aether-state/tests/controller_chat.rs | 343L | 5T
rune/crates/aether-state/tests/history_commands.rs | 386L | 7T
rune/crates/aether-state/tests/HISTORY_INTEGRATION.md | 42L
rune/crates/aether-state/tests/history-harness.ps1 | 35L
rune/crates/aether-state/tests/message_edit.rs | 199L | 5T
rune/crates/aether-state/tests/slash_commands.rs | 355L | 11T
rune/docs/feature-parity.md | 294L
rune/README.md | 218L
rune/shell-tree.json | 2563L
