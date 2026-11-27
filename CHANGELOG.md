# Changelog

What's new in each version of Wren. The release workflow copies the section
for the version being released to the top of the GitHub release notes, and
Wren shows it in the update window ("What's new"). Add a section before
tagging a new version.

## 0.1.10

- Codex chats now stay only in Wren: Codex no longer saves them in its own
  history, so they don't show up in the Codex app. If Codex has to start
  again in the middle of a chat, Wren gives it the earlier messages from its
  own copy. Settings → Privacy → "Keep Codex chats only in Wren" (on by
  default). Chats from earlier versions stay in Codex's history.

## 0.1.9

- Notifications: Wren tells you when a pane needs your approval, asks you a
  question, finishes a task, or stops with a problem (an error or a usage
  limit). They only appear while Wren isn't the window in front, and coming
  back to Wren makes that pane the active one.
- Settings → Notifications: turn them on or off, choose which kinds you want,
  play a sound or not, and send a test notification. Each computer keeps its
  own choice.

## 0.1.8

- Wren checks for a new version a few seconds after it opens (and every 6
  hours while it stays open) and opens an update window by itself, with
  what's new in that version.
- "Later" hides the window until you open Wren again; the "Update available"
  button in the top bar stays.

## 0.1.7

- Change Manual / Auto / Plan / Full access while a pane is working.
  Claude Code switches at once; Codex and Antigravity use the new mode for
  the rest of the task where they can, and fully from your next message.
- Switching to Auto or Full access allows the approval card that is waiting.
- Answered question forms and approval cards move into the "Worked for…"
  steps, so the chat stays clean. Open the steps to see them again.
