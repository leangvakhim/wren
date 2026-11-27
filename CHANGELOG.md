# Changelog

What's new in each version of Wren. The release workflow copies the section
for the version being released to the top of the GitHub release notes, and
Wren shows it in the update window ("What's new"). Add a section before
tagging a new version.

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
