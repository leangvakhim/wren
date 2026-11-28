# Changelog

What's new in each version of Wren. The release workflow copies the section
for the version being released to the top of the GitHub release notes, and
Wren shows it in the update window ("What's new"). Add a section before
tagging a new version.

## 0.2.2

- **See the code behind a page**: the browser panel now has **Preview | Code**
  tabs. Code shows the whole file of the page you're looking at (for
  example a Visualize animation) with colors and line numbers.
- **Copy, Save as… and Show in Finder** above the code. Wrap turns long
  lines on or off.
- **Edit and re-run**: change the code of a page from your project right in
  the Code tab. Wren saves it about a second after you stop typing and the
  preview reloads. If the AI rewrites the file, the new version appears
  there.
- **Web pages too**: for internet pages the Code tab shows their HTML
  source (read-only, fetched again without your cookies).

## 0.2.1

- **Many more folder icons**: the icon picker now has 2,148 built-in icons
  (all of Lucide), with a search box and categories. Click the icon next to a
  folder in the sidebar.
- **Download 5,509 more icons** (Tabler) with one click in the picker. Wren
  downloads them once from cdn.jsdelivr.net, checks them and keeps them on
  this computer. Nothing is downloaded unless you click.
- **Emoji as folder icons**: search by name, or paste any emoji.
- **Your own pictures as icons**: add a logo or any picture (SVG, PNG, JPEG,
  GIF or WebP, up to 1 MB). Wren keeps a copy in its own data folder.

## 0.2.0

- **Phone link**: use Wren from your phone. Watch every pane live, allow or
  deny approvals, answer question forms, send messages with photos or files,
  stop a task, start a new task (AI, account, model, permission, folder) and
  open saved chats, with push notifications when a pane needs you or
  finishes. Pair a phone with a QR code (Phone in the top bar) and allow it on
  the computer. Everything is end-to-end encrypted through a relay you run
  yourself (see relay/README.md); the relay can't read it.
- **Copy button under every answer**: copies the AI's response as Markdown.
- **Folder icons**: give each project folder an icon (code, web, mobile,
  database, AI, lab, physics, biology, data, math, …) and a color. Click the
  icon next to a folder in the sidebar. Wren suggests one from the project's
  files until you choose; it also shows in the message box's folder chip.
- Fixed: when Claude Code went on by itself (after a background command or a
  sub-agent finished), the pane looked finished and said "Message Claude
  Code" while Claude was still working. The pane now shows it as working,
  with the steps, the timer and Stop.
- Fixed: Claude Code's cost was counted too high from the second message of a
  conversation onwards (Claude reports a running total; Wren added it up
  again).
- Fixed: when reading an account's plan limits failed and no numbers were
  known yet, Wren kept asking again in a loop.

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
