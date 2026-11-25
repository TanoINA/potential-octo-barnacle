# Conversation controls

## Skill selection

Use **Add Context → Actions & Skills** to search installed skills and select up to five for the next message. Selected skills appear as removable chips; the composer text stays unchanged. Slash autocomplete and `/skill-name task` also attach the selected context.

Selections travel with the user turn, prompt queue, Undo, Retry, and Edit/Resend. The native runtime validates catalog names and loads the latest turn's instructions before contacting the provider. It does not silently rewrite the displayed prompt or automatically reload historical skill selections. Invalid or unreadable selected skills fail visibly.

## Prompt queue

Sending while a run is active puts the message in an in-memory queue inside the status stack docked above the composer. Each queued item keeps its attachments, selected skills, execution mode, and Council options. Click the arrow icon to stop the current run, wait for its final save, and run that queued prompt next while the rest stay paused. Click the pencil icon to move an item back into the composer, or click the trash icon to drop it.

Queue ownership follows the originating session, so switching chats neither discards queued items nor sends them into a different conversation. Rejected sends stay in the queue, and an in-flight dispatch cannot fire twice. Queues live in memory only and clear on restart.

## Activity details

Live activity, queued prompts, and the context meter sit together in the status stack right above the composer. Dragging or clicking the thin grip ridge above the stack folds it away per session when you want a cleaner viewport.

The activity tray groups active skills, subagents, and background tasks, staying collapsed until you open it and scrolling internally so it never pushes the composer off screen. Tool traces inside the transcript also stay quiet and collapsed once finished, keeping main actions separate from nested subagent calls. Failed tools appear as a separate count without disguising normal work as an app crash, and you can still expand any row to read its raw output.

## Theme skins and workspace bar

You can pick from five built-in themes (`Nous Dark`, `Antigravity Graphite`, `Nous Light`, `Catppuccin Mocha`, and `Hermes Ember`) in **Settings → Agent defaults**, cycle them from the palette icon in the sidebar footer, or switch skins from the slim workspace bar at the bottom of the window. That bottom bar also displays the current folder name, active MCP server count, and discovered skill count.

## Sidebar run status and project groups

Recent conversations group automatically by workspace folder with collapsible headers whenever search is empty. Each live chat can show **Running**, **Saving**, **Completed**, **Stopped**, or **Failed**, even while you look at another thread. **Completed** only shows up after the final disk save succeeds, so a failed save is never reported as done. Activity badges reset on restart instead of guessing status from old messages, and deleting or clearing a session removes its status record.

## Context compaction

`/compact` writes a full backup copy of the chat as a new session titled `… (before compact)`, then folds older turns into a local summary while keeping the latest turn as written. If saving that backup fails, compaction stops before altering anything. Because the summary drops detail, open the backup session from the sidebar whenever you need the original turns.

Automatic micro-compaction checks each request before sending to the provider and shortens older, bulky tool outputs when the estimated prompt grows large. It keeps the newest tool batch untouched and leaves saved history as-is on disk. Token counts are estimates, not a guarantee that the provider will accept the full context.

## File links

Safe local documents open through your OS default handler. Executables, installers, shortcuts, and unknown file types never launch directly; AetherAgent reveals them in the file manager instead and prints an error near the link if opening or revealing fails.

## Boundaries

None of these controls relax canonical history checks, installer signature verification, or tool path rules. Signed auto-updates still require operator signing keys and a hosted update manifest; building locally does not enable automatic updates.

## Welcome and session navigation

Empty chats center the welcome prompt together with the composer. Once the first message lands, the composer stays mounted at the bottom so in-progress text and attachments carry over without remounting. When no folder is open, the welcome screen shows an **Open folder** button, though chatting without a workspace still works and workspace-only suggestions wait until you pick a directory.

Model selection, reasoning effort, and execution mode live in the composer footer instead of cluttering the header or sidebar. The desktop artifact side pane docks alongside the chat and resizes from `280px` to `720px` via pointer drag or arrow keys, falling back to an overlay drawer on narrow windows. Sidebar previews strip Markdown formatting into plain text while keeping safe full-text search highlights and full-title tooltips intact.

## Optional main-agent turn limit

Fresh configs and requests without `maxTurns` default to `0`, meaning unlimited turns. Entering a positive integer caps the main agent loop (replacing the old implicit 200-turn ceiling). Saved positive caps stay active until you turn off **Limit agent turns** in **Settings → Agent defaults**, and the number input stays hidden while unlimited mode is selected.

Unlimited turns still end normally when the model gives a final answer, hits a provider limit, fails an unrecoverable check, or receives a stop request. Standard API token usage and tool approvals still apply, and subagents keep their own tighter turn caps. Rune uses the same `0`-for-unlimited setting convention; older screenshots or skill docs mentioning a hard 200-turn limit are outdated.
