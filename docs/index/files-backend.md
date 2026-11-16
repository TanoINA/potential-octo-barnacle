# backend (32 files, 35796 lines, 452 tests)

src-tauri/src/agent/events.rs | 100L | 3T | T:EventSink T:AgentHost f:new f:emit
src-tauri/src/agent/git.rs | 941L | 15T | T:GitFileChange T:GitRepoStatus f:parse_porcelain_output f:check_git_status f:git_commit f:git_push f:git_fetch f:git_revert_file f:git_stage_file
src-tauri/src/agent/history_maintenance.rs | 16L | f:warm_session_index
src-tauri/src/agent/images.rs | 542L | 9T | f:prepare_attachments f:validate_inputs f:user_content f:validate_request_images
src-tauri/src/agent/mcp.rs | 1044L | 14T | T:McpClient T:McpManager f:connect f:notify f:request f:list_tools f:call_tool f:shutdown f:contains f:call f:result_text f:list_server_tools f:call_server_tool
src-tauri/src/agent/mod.rs | 42L | f:create_hidden_command
src-tauri/src/agent/native_smoke.rs | 282L | 1T | f:profile f:enter
src-tauri/src/agent/profile.rs | 577L | 6T | f:resolve_root f:configure_storage_root f:root f:import_profile
src-tauri/src/agent/provider_audit_tests.rs | 77L | 5T
src-tauri/src/agent/provider_payload_audit_tests.rs | 724L | 20T
src-tauri/src/agent/provider_state_audit_tests.rs | 135L | 4T
src-tauri/src/agent/provider.rs | 2589L | 21T | T:ChatCompletionToolCall T:ChatCompletionFunctionCall T:StreamResult T:ToolDelta T:CompletionOptions f:assistant_message f:fetch_models f:is_retryable_provider_error f:stream_chat_completion f:supports_thinking_off
src-tauri/src/agent/pty.rs | 52L | T:ShellCommandResult f:run_shell_command
src-tauri/src/agent/reflection_tests.rs | 1065L | 22T
src-tauri/src/agent/reflection.rs | 765L | f:should_reflect f:reflect f:read_generated_skill
src-tauri/src/agent/release_download.rs | 409L | 19T
src-tauri/src/agent/release_policy.rs | 98L | 3T | f:validate_url f:is_newer f:validate_key
src-tauri/src/agent/release.rs | 244L | 1T | @get_release_info @check_release_update @install_release_update @restart_after_update T:ReleaseState T:ReleaseInfo T:UpdateResult T:InstallResult f:get_release_info f:check_release_update f:install_release_update f:restart_after_update
src-tauri/src/agent/review_pending.rs | 784L | 8T | T:PendingWrite f:pending_root f:stage f:list_pending f:pending_diff f:approve_pending f:reject_pending
src-tauri/src/agent/review_skills.rs | 1258L | 13T | T:ReviewSkillStore f:new f:changes f:execute
src-tauri/src/agent/review.rs | 635L | 9T | T:ReviewRequest f:review
src-tauri/src/agent/runner.rs | 5477L | 64T | T:UpdatePermit T:AgentRunner f:new f:acquire_update_permit f:begin f:abort f:finish f:review_session f:run f:run_council f:completed_review_snapshot
src-tauri/src/agent/search.rs | 524L | 6T | T:DeepSearchResult f:open_connection f:init_db f:index_session f:delete_session_index f:search_query
src-tauri/src/agent/session_journal.rs | 2161L | 27T
src-tauri/src/agent/session_pages.rs | 1741L | 23T | T:WarmIndexResult f:cache_session f:load_page f:load_around f:read_message_image_inputs f:read_artifact f:warm_index f:list_catalog f:invalidate
src-tauri/src/agent/skills.rs | 1129L | 16T | f:default_skill_paths f:ensure_starter_skills f:init_skills_environment f:workspace_skill_paths f:review_creation_root f:effective_skill_paths f:load_skills f:render_skill_catalog f:find_skill f:normalize_skill_instructions f:read_skill_content f:save_skill_markdown
src-tauri/src/agent/storage.rs | 5861L | 78T | T:CommittedSaveProof T:CommittedSaveError f:config_dir f:default_settings f:load_settings f:save_settings f:list_sessions f:list_all_artifacts f:load_session f:parse f:recover_save_ack f:save_session f:replace_session_checked f:checkpoint_session f:record_self_improvement f:attach_pending_link
src-tauri/src/agent/tasks.rs | 1311L | 19T | T:TaskInfo T:TaskManager f:get_task_manager f:new f:spawn_task f:has_running_tasks f:list_tasks f:get_status f:send_input f:kill_task
src-tauri/src/agent/tools.rs | 2526L | 29T | T:ToolEvent T:ToolContext T:ToolExecutionResult f:success f:failure f:review_tool_definitions f:get_agent_tools f:execute_tool f:resolve_workspace_path f:list_workspace_files
src-tauri/src/agent/types.rs | 876L | 8T | T:ProviderKind T:ReasoningEffort T:ProviderConfig T:McpServerConfig T:ExecutionMode T:ReviewConfig T:AgentSettings T:FileDiffStat T:ToolCallItem T:AttachedContext T:CouncilCandidate T:CouncilTurn T:SelfImprovementAction T:SelfImprovementChange T:SelfImprovementEvent T:ImageInput
src-tauri/src/lib.rs | 1804L | 9T | @send_message @send_council_message @review_session @read_reflection_skill @list_review_proposals @review_proposal_diff @approve_review_proposal @reject_review_proposal @abort_agent @get_settings @save_settings @warm_session_index @list_sessions @load_session @load_session_page @load_session_around
src-tauri/src/main.rs | 7L
