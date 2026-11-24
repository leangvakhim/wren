# Wren

A lightweight desktop GUI for AI coding CLIs: **Claude Code**, **Codex** and
**Antigravity CLI** in up to 4 side-by-side conversations.

Built with **Tauri 2** (Rust backend) and **Svelte 5** (UI). Wren runs the
official, unmodified CLIs on your computer in their headless JSON modes and
shows their output as chat bubbles. It never reads or stores your login tokens.

## Prerequisites (macOS)

1. **Xcode Command Line Tools**: `xcode-select --install`
2. **Rust**: <https://rustup.rs> (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
3. **Node.js 20+**: <https://nodejs.org> or `brew install node`
4. The CLIs you want to use (you can sign in from inside Wren):
   - Claude Code: `curl -fsSL https://claude.ai/install.sh | bash`
   - Codex: `npm install -g @openai/codex`
   - Antigravity CLI: `curl -fsSL https://antigravity.google/cli/install.sh | bash`

Windows and Linux work too. See the Tauri prerequisites:
<https://tauri.app/start/prerequisites/>.

## Run

```bash
npm install
npm run tauri dev      # opens the Wren window, reloads as you edit
```

## Build an installer

```bash
# Builds also make signed update files, so they need your update key (see "Updates" below):
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/wren.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<your key password>"
npm run tauri build    # macOS: .app and .dmg in src-tauri/target/release/bundle/
```

The Mac build is ad-hoc signed (free, no Apple Developer account). On another
Mac, the first launch needs System Settings → Privacy & Security → "Open Anyway".

## Release for macOS, Windows and Linux

`.github/workflows/release.yml` builds Wren on GitHub for Apple Silicon and Intel
Macs, Windows and Linux, and puts the installers on a **draft** GitHub release.

1. Set the new version in `src-tauri/tauri.conf.json` (and `package.json`,
   `src-tauri/Cargo.toml`).
2. Commit, then push a matching tag: `git tag v0.1.0 && git push origin v0.1.0`
   (or run the workflow by hand: GitHub → Actions → release → Run workflow).
3. When the four builds finish (about 15–25 min), open GitHub → Releases, check
   the draft and press **Publish release**.

Files: macOS `Wren_<v>_aarch64.dmg` / `Wren_<v>_x64.dmg`, Windows
`Wren_<v>_x64-setup.exe` / `.msi`, Linux `.AppImage` / `.deb` / `.rpm`.

## Updates

Installed copies check the latest **published** GitHub release (`latest.json`) at
start and every 6 hours. When a newer version exists, an **Update available**
button appears in the top bar: one click downloads, installs and restarts Wren
(accounts and conversations stay). Settings → Updates → Check for updates does it
by hand.

Updates are signed. One-time setup (already done for this repo):

1. `npm run tauri signer generate -- -w ~/.tauri/wren.key`. Keep `wren.key` private
   and backed up: without it, installed copies can't be updated anymore.
2. Put the text of `~/.tauri/wren.key.pub` in `src-tauri/tauri.conf.json` →
   `plugins.updater.pubkey`.
3. GitHub repo → Settings → Secrets and variables → Actions: add
   `TAURI_SIGNING_PRIVATE_KEY` (text of `wren.key`) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Copies of 0.1.0 have no updater: install 0.1.1 by hand once.

## Using Wren

- **1 / 2 / 3 / 4** in the top bar sets how many conversations are shown.
- **Accounts** (top bar): add as many accounts per AI as you like and click
  **Sign in**. Wren runs the AI's own sign-in, shows the **link with a Copy
  button**, and a small terminal in case the AI asks you to paste a code.
  Open the link in the browser (or browser profile) of the account you want.
  - Claude Code and Codex accounts each get their own private login folder, so
    several can be used at the same time.
  - Antigravity keeps one Google login per computer: one account is *active*
    at a time; signing in to another makes it the active one.
  - Each AI also has a **Default** account = the login you made in a terminal.
- Each pane has its own **AI**, **account**, **project folder** and **permission**.
  The folder is the chip in the message box (click to change). New panes start
  in **~/Desktop** on macOS and your **home folder** on Windows/Linux.
- If an account hits its usage limit, the pane offers your other accounts.
  You choose; Wren never switches by itself. Switching starts a new
  conversation, because the old one belongs to the other account.
- **Permission** (per pane, like Claude Desktop's Manual / Auto). Changing it keeps the conversation.
  - *Manual* (default): before the AI edits files or runs a command, a card appears in the
    chat with **Allow**, **Allow for this chat** and **Deny**. In Plan, Claude asks with
    **Start** / **Keep planning** when its plan is ready. Codex only offers "for this chat"
    for some requests; the button is hidden when it doesn't.
    Antigravity can't ask in this mode, so it refuses instead and the card offers
    **Allow & retry** (your last message again, with Auto for that one turn).
  - *Auto*: the AI's own auto mode decides; safe actions run, risky ones are blocked. Nothing is asked.
  - *Plan*: researches and proposes; changes nothing.
  - *Full access*: edits and runs any command without asking or checking. Use with care.
- **Model & effort**: the star chip in the message box (next to the folder chip)
  picks the model and the effort tier, like Claude Desktop. Every model shows its full name
  and version with a one-line description (e.g. "Opus 5.5", "GPT-6.2-Sol",
  "Gemini 4 Carbon"), read live from each CLI. Claude models follow the newest
  version of their family, so the version shown updates by itself. Changing the model keeps the conversation going.
- **Conversations sidebar** (left, toggle with the panel button at the top left):
  every conversation is saved **only on this computer**, grouped by project
  folder. Click one to open it in the active pane and continue it. Rename or
  delete with the buttons that appear on hover.
- **Usage**: tokens in/out and an **≈ USD estimate** under each pane. Claude Code
  reports its own cost; Codex and Antigravity are estimated from API list prices
  in `src/lib/pricing.ts` (update it when prices change). On subscriptions you
  aren't billed per token, so this is an estimate.
- **When an account runs out of usage** (Settings, the gear icon): Wren can
  *continue in another AI* by itself (with a recap of the conversation, in the
  order you choose) and/or *wait for the reset and continue* (it reads the reset
  time from the message and re-sends your last message; keep Wren open). It
  never rotates between accounts of the same AI on its own.
- **Usage** (top bar): estimated cost and tokens per day (stacked by AI), totals,
  and breakdowns by AI, account, model and project folder, for 7/30/90 days or
  all time, with a "How to read this" explanation. Built from Wren's local
  conversation history only.
- **Privacy** (top bar): what stays local, what Wren turns off automatically,
  and where to switch off model training for each AI.
- **Plan limits** (5-hour and weekly) for each Claude Code and Codex account come
  from the AI's own CLI with its own sign-in (Wren never sees your tokens). See
  them in **Accounts** (bars per account, Refresh), at the bottom of each pane
  (click to refresh) and in **Usage** ("Plan limits right now" and "Daily usage
  per account"). A pane warns you at **80%** and **95%**. Codex also shows
  OpenAI's own token count per day. Antigravity only shows its quota inside agy,
  so **Accounts → Open /usage** opens that screen for you.
- **Study** and **Deep research**: the **+** button in the message box.
  - *Study*: a tutor. It explains step by step, gives hints before answers and
    checks your understanding (Claude Code uses its built-in "Learning" style).
  - *Deep research*: searches the web widely (Claude: web search, fetch and
    parallel sub-agents; Codex: live web search; Antigravity: its own search),
    checks sources and writes a cited report. The report is also saved in the
    project folder under `research/`. It uses more of your plan.
  - Turning a mode on or off keeps the conversation. Click the mode chip to turn it off.
- **Files**: paste (⌘V), drop files on the message box, or use **+ → Add files…**.
  Images, PDFs, spreadsheets (CSV, Excel), documents, zips, code and scripts, up to
  100 MB each and 10 per message. Programs and installers are refused. Wren keeps a
  private copy in its own data folder and tells the AI where it is (images and PDFs
  go straight into the message where the AI supports it).
- Type `\new` (or press **New**) to start a fresh conversation in that pane.
- **Stop** ends the current task. Your next message continues the conversation.
- **Light / Dark / System** sets the appearance.

## Privacy

- Wren has no server. Its settings and conversation history stay in its app
  data folder on your computer (shown in the Privacy window).
- Claude Code panes run with telemetry, error reporting and surveys turned off
  (`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and related settings).
- Codex runs with `analytics.enabled=false` and `feedback.enabled=false`.
- Antigravity's telemetry is a one-time switch inside agy (`/config` → Account →
  Enable Telemetry); the Privacy window opens it for you.
- Your prompts and code still go to the AI company you chose, because that is
  where the model runs. Their own data policies apply.

## Notes

- On Claude Pro/Max plans, headless use (`claude -p`, which Wren uses) draws
  from the plan's separate monthly Agent SDK credit, not your interactive limit.
- Cost in USD comes from the CLI where it reports it (Claude Code). On
  subscription plans it is an API-price estimate, not what you are billed.

## Project layout

```
src/                 Svelte UI (routes/+page.svelte = window, lib/Pane.svelte = one conversation,
                     lib/AccountsDialog.svelte + lib/LoginSheet.svelte = accounts & sign-in)
src-tauri/src/
  providers.rs       how each CLI is launched + its JSON events parsed (with tests)
  sessions.rs        up to 4 CLI processes, stop/resume/new (with end-to-end test)
  accounts.rs        accounts per AI, private login folders, status/sign-out (with tests)
  login.rs           sign-in terminal (PTY) + link finder (with tests)
  limits.rs          plan limits (5-hour / weekly / daily) read from each CLI (with tests)
  reports.rs         Deep research reports saved to <project>/research/ (with tests)
  models.rs          model + effort lists per AI (with tests)
  history.rs         local conversation history (with tests)
  events.rs          the one event format the UI understands
  lib.rs             Tauri commands
app-icon.svg         the "Perch" logo; regenerate icons with `npx tauri icon app-icon.png`
```

Run the Rust tests with `cd src-tauri && cargo test`, and the usage-limit
tests with `node --experimental-strip-types src/lib/limits.test.ts`.
