# Conversation controls

## Skill selection

Use **Add Context → Actions & Skills** to search installed skills and select up to five for the next message. Selected skills appear as removable chips; the composer text stays unchanged. Slash autocomplete and `/skill-name task` also attach the selected context.

Selections travel with the user turn, prompt queue, Undo, Retry, and Edit/Resend. The native runtime validates catalog names and loads the latest turn's instructions before contacting the provider. It does not silently rewrite the displayed prompt or automatically reload historical skill selections. Invalid or unreadable selected skills fail visibly.

## Prompt queue

While a run is active, sending adds a queued prompt. Each item preserves attachments, selected skills, execution mode, and Council options.

- **Arrow:** stop the active run, wait for its final save, and send that item. Other queued items stay paused.
- **Pencil:** restore the item to the composer for editing.
- **Trash:** remove the queued item.

Queue ownership follows the originating session. Switching chats does not discard queued items or send them into another chat. Rejected sends retain the item; pending acceptance cannot dispatch it twice. Queues are in-memory state, not restart-persistent storage.

## Activity details

The compact activity tray groups skills, subagents, and background tasks. It opens only on request and bounds expanded content so the composer remains usable.

Tool traces distinguish **main actions** from **subagents**. Child tool calls are grouped under their owning subagent and rendered when that group is opened. Failed tool counts are separate from the neutral action count; a failed tool or intentional RED test is not automatically an application crash. Raw results remain inspectable.

## Sidebar run status

Each live chat can show **Running**, **Saving**, **Completed**, **Stopped**, or **Failed**, including when another chat is open. Completed appears only after the final save acknowledgement; a failed save is not labeled completed.

Activity badges reflect the current application lifetime. After restart, the UI does not guess historical completion from old messages. Clearing or deleting a session removes its activity state.

## File links

Safe local documents open through the OS handler. Executable, installer, shortcut, and unknown types are not automatically executed: the chat falls back to revealing the file in the file manager. Open/reveal failures are shown near the link.

## Boundaries

These controls do not weaken canonical history guards, installer verification, or tool execution policy. Signed updates require separately provisioned operator credentials and hosting; a successful local build does not enable automatic updates.
