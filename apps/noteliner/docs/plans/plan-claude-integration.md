# Claude Integration — Implementation Plan

## Overview

Let the user write prompts inside notes and run them against Claude without leaving NoteLiner. A prompt lives in the note as a fenced ` ```claude ` block (or a free-form prompt typed into a panel); running it streams a response into a right-side **Claude panel**, and the result is saved back into the project — inline under the prompt, or as a new note. Because every project is a git repo with auto-commit on write (`project-service.js:213`), everything Claude produces is versioned for free.

The key architectural decision: **do not build a second vault API.** NoteLiner already exposes its project as thirteen structured tools through `McpService` (`mcp-service.js:280`), with a shipped permission model — confirm-before-write, per-tool disable, session trust (`mcp-service.js:472`). The in-app Claude runs a tool-use loop against **that same registry, in-process**. External Claude Code over the MCP socket and in-app Claude become two front doors onto one set of tools, one permission gate, and one write path.

This supersedes `plan-ai-integration.md`, whose "Current State" predates the MCP server (that plan's Option E shipped as `mcp-service.js` / `bin/noteliner-mcp-bridge.js`).

## Goals

1. Write a prompt in a note; run it with one keystroke; get the answer saved into the project as a real, committed note or an inline block.
2. Reuse `McpService`'s tool registry and permission gate verbatim — no second write path, no second confirm dialog, no drift between in-app and external AI.
3. Ship opt-in and off by default, following the `mcpEnabled` precedent (`main.js:59-66`). A user who never turns it on sees no behavior change and pays no startup cost.
4. Keep credentials out of the project repo, and out of plaintext on disk.

## Current State

What already exists and will be reused:

| Capability | Location | Reuse |
|---|---|---|
| 13 vault tools + JSON schemas | `mcp-service.js:280` (`toolDefinitions`) | Mapped to Anthropic tool definitions |
| Tool dispatch, transport-agnostic | `mcp-service.js:529` (`invokeTool`) | Called directly, no socket |
| Disabled-tools + confirm-before-write | `mcp-service.js:472` (`preflight`) | Called directly |
| Confirm modal + queue | `McpConfirmModal.svelte`, `App.svelte` `mcpConfirmQueue` | Reused as-is |
| Confirm IPC round-trip | `main.js:432`, `main.js:441` | Reused as-is |
| Auto-commit + debounced push | `project-service.js:151`, `:213` | Free, via the tools |
| Right-side resizable panel pattern | `App.svelte:1234-1251` (Preview) | Cloned for the Claude panel |
| Persisted layout flags | `App.svelte:41` `DEFAULT_LAYOUT` | `showClaude`, `claudeWidth` added |
| Global prefs in `userData` | `main.js:56` `loadUIPrefs` | `claude*` keys added |
| Log stream to renderer | `main.js:245-248` (`git:log`) | Reused for Claude run logging |
| Editor selection state | `project.svelte.js:44,47` | Drives "run selection" |

`McpService` is cleanly layered — `handleToolsCall` (`:446`) is a thin JSON-RPC wrapper over `preflight` + `invokeTool`. Nothing in the dispatch path knows or cares about the socket. That is what makes the in-process reuse a small change rather than a refactor.

## Provider Decision

Three ways to reach Claude from an Electron main process:

### Option 1 — Anthropic SDK, in-process tool loop (recommended)

Ship `@anthropic-ai/sdk`, call `client.messages.stream()` from the main process, and run the tool-use loop against `McpService.invokeTool`.

- **Pros** — No external install; works identically in a packaged app on all three platforms; full control over streaming, model, effort, and cost display; the vault tools are exactly NoteLiner's tools; pure-JS dependency, no native rebuild.
- **Cons** — NoteLiner owns credential storage (mitigated below).

### Option 2 — Spawn the `claude` CLI

`spawn('claude', ['-p', prompt])` with `cwd` = project path.

- **Pros** — No key handling; uses the user's existing Claude subscription auth, their `CLAUDE.md`, their skills.
- **Cons** — **PATH resolution is the blocker.** A GUI-launched Electron app on macOS and Linux does not inherit the login shell's environment, so `claude` installed via nvm/asdf/npm-prefix is frequently not on `PATH`. Also: no structured tool use (stdout parsing only), and the CLI would reach the vault through the filesystem rather than through `ProjectService`, so writes could land outside `noteliner.json`.

### Option 3 — Claude Agent SDK (`@anthropic-ai/claude-agent-sdk`)

Claude Code packaged as a library.

- **Pros** — Batteries-included harness, built-in file tools, subagents.
- **Cons** — Its built-in Read/Write/Edit tools bypass `ProjectService` entirely, which is precisely the thing NoteLiner's MCP layer exists to prevent. Heavy dependency for a note-taking app. It is the right tool for a coding agent, not for "run this prompt and save the answer."

### Recommendation

**Option 1**, with the provider behind a thin `ClaudeService` interface so Option 2 can be added later as an alternate backend for subscription users (see Phasing v3). Option 3 is declined — it duplicates and undermines the existing tool layer.

**Model defaults:** `claude-opus-5` (1M context, $5/$25 per MTok), adaptive thinking (`thinking: { type: 'adaptive', display: 'summarized' }` — summarized so the panel can show reasoning instead of a long silent pause), `output_config: { effort: 'high' }`, streaming with `max_tokens: 64000`. Model and effort are configurable in Settings; `claude-sonnet-5` ($2/$10) and `claude-haiku-4-5` ($1/$5) offered as cheaper choices. Never downgrade silently.

## Architecture

```
┌──────────────────────── Electron main ─────────────────────────┐
│                                                                │
│  ClaudeService ──── messages.stream() ────► api.anthropic.com  │
│       │                                                        │
│       │ tool_use blocks                                        │
│       ▼                                                        │
│  McpService.preflight(name, args)   ← confirm modal, disabled  │
│  McpService.invokeTool(name, args)  ← the same 13 tools        │
│       │                                                        │
│       ▼                                                        │
│  ProjectService ──► noteliner.json + .md ──► GitService commit │
└────────────────────────────────────────────────────────────────┘
          ▲ claude:chunk / claude:tool / claude:done
          │
   Claude panel (renderer)
```

`bin/noteliner-mcp-bridge.js` → socket → `McpService` remains untouched and enters the same box from the left. One tool registry, two callers.

### New file: `src/main/claude-service.js`

```js
class ClaudeService {
  constructor({ mcpService, getPrefs, getApiKey, log, onEvent }) { ... }

  // Anthropic tool defs from the MCP registry: inputSchema -> input_schema,
  // minus any tool the user disabled in Settings.
  toolsForRequest() {
    const disabled = this.getPrefs().disabledTools || [];
    return this.mcpService.toolDefinitions()
      .filter(t => !disabled.includes(t.name))
      .map(({ name, description, inputSchema }) =>
        ({ name, description, input_schema: inputSchema }));
  }

  // Streams, executes tool_use blocks through McpService, loops until end_turn.
  async run(runId, { prompt, systemPrompt, contextNoteIds }) { ... }

  cancel(runId) { ... }   // AbortController per run
}
```

Every tool call goes through `mcpService.preflight()` first, so the existing confirm modal fires for in-app Claude exactly as it does for external Claude Code. A `ToolError` becomes a `tool_result` with `is_error: true` and the loop continues — the model can recover, same as over MCP.

`ClaudeService` holds **no** direct reference to `ProjectService`. Its only route into the vault is `McpService`. That invariant is what keeps the two front doors honest, and is worth a comment in the constructor.

## The Prompt-in-a-Note Convention

A prompt is a fenced block with the `claude` info string:

````markdown
```claude
Summarize every note tagged #meeting from the last two weeks and list the
open action items with owners.
```
````

Running it (`Ctrl+Enter` with the caret inside the block, a gutter Run button, or the command palette) streams the response into the panel and then applies the configured output mode.

Optional per-block front matter on the info line for overrides:

````markdown
```claude model=claude-sonnet-5 out=note
````

Recognized keys: `model`, `effort`, `out` (`inline` | `note` | `panel`), `name` (target note name for `out=note`).

**Inline output** is written back delimited by HTML comment markers so a re-run replaces the previous answer instead of appending forever:

```markdown
<!-- claude:begin 7f3a -->
…response…
<!-- claude:end 7f3a -->
```

The id ties an answer to its prompt block; a run with no matching end marker appends a fresh pair. Markdown surgery of this kind already has precedent in `src/renderer/lib/slideEdits.js`, and the same testing approach applies (`tests/integration/slide-edits.test.mjs`).

Parsing lives in a new `src/renderer/lib/claudeBlocks.js`: `findBlocks(md)`, `blockAtLine(md, line)`, `replaceAnswer(md, id, text)` — pure functions over a string, unit-testable with no Electron.

**Run modes**, all reaching the same `claude:run` IPC:

| Trigger | Prompt source |
|---|---|
| `Ctrl+Enter` in editor | The ` ```claude ` block containing the caret |
| Command palette → *Run Selection as Claude Prompt* | Current selection (`selectionRange`) |
| Command palette → *Run Note as Claude Prompt* | Whole note body |
| Panel input box | Free-form text typed in the panel |

## UI Design

### Claude panel

A right-side panel, peer of Preview/History/Attachments, cloned from the Preview branch at `App.svelte:1234-1251` (drag handle, `claudeWidth` clamped 200–1600px, persisted through `saveWindowState`). The sidebar is 180–500px and already hosts six panes — a conversation does not belong there.

```
┌───────────────────────────────────────┐
│ CLAUDE            [model ▾]  [⨯]      │
├───────────────────────────────────────┤
│  You                                  │
│  Summarize every note tagged #meeting…│
│                                       │
│  Claude                    opus-5     │
│  ▸ thinking (summarized)              │
│  ⚙ search { query: "tag:meeting" }    │
│  ⚙ read_note { name: "2026-08-18" }   │
│                                       │
│  Across four meeting notes there are  │
│  eleven open items…                   │
│                                       │
├───────────────────────────────────────┤
│  [ask a follow-up…              ] [▶] │
│  ↳ docs/plans/  ·  1,240 in / 890 out │
├───────────────────────────────────────┤
│ [Save as Note] [Insert Inline] [Stop] │
└───────────────────────────────────────┘
```

Tool calls render as compact one-liners using the existing `McpService.summarizeToolCall()` text (`mcp-service.js:503`), so the panel and the confirm modal describe an action with the same words. Assistant text renders through `marked`, already a dependency.

Follow-up input makes the panel multi-turn from v1 — the conversation array is already required by the tool loop, so single-shot would be extra work, not less.

### Toolbar and shortcuts

`ToolbarButton icon="fa-wand-magic-sparkles"`, title *Claude (Ctrl+Shift+C)*, active when `layout.showClaude`. Rendered only when `projectOpen` **and** the module is enabled.

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+C` | Toggle Claude panel |
| `Ctrl+Enter` | Run the prompt block at the caret |
| `Esc` | Cancel the running turn (when the panel has focus) |

Palette entries under a new **Claude** section, following `App.svelte:183-246`: `claude.togglePanel`, `claude.runBlock`, `claude.runSelection`, `claude.runNote`, `claude.saveAsNote`, `claude.cancel`. Each carries a `when` guard so they are hidden while the module is off.

### Settings → Claude tab

New tab in `SettingsModal.svelte` beside `ui` / `mcp` / `shortcuts` (`SettingsModal.svelte:188-191`):

- **Enable Claude** — master toggle, default off.
- **API key** — password field; shows `sk-ant-…last4` once set, with *Replace* and *Remove*. Never echoed back to the renderer in full.
- **Model** — `claude-opus-5` (default) / `claude-sonnet-5` / `claude-haiku-4-5`, each labelled with its per-MTok price.
- **Effort** — low / medium / high (default) / xhigh / max.
- **Vault tools** — *Let Claude read this project* (default on) and *Let Claude write to this project* (default **off**). Writes additionally honour the existing per-tool disable list, so the MCP tab and this tab agree.
- **Confirm before writes** — default on for in-app Claude (stricter than the MCP default, because the panel is a lower-friction trigger).
- **Default output** — panel / inline / new note, and target folder for new notes (default `docs/plans/`).

### Status bar

Extend the existing MCP indicator block (`StatusBar.svelte:169`) with a Claude state chip: idle / streaming / tokens used this session.

## Data & IPC

### Preferences (`{userData}/ui-preferences.json`, via `loadUIPrefs`)

```
claudeEnabled: false
claudeModel: 'claude-opus-5'
claudeEffort: 'high'
claudeAllowRead: true
claudeAllowWrite: false
claudeConfirmWrites: true
claudeDefaultOutput: 'panel'
claudeOutputFolder: 'docs/plans'
```

The API key is **not** stored here. It goes in `{userData}/claude-credentials.json`, encrypted with Electron's built-in `safeStorage.encryptString()` (OS keychain on macOS, libsecret on Linux, DPAPI on Windows), file mode `0600`, with a plaintext-with-a-warning fallback where `safeStorage.isEncryptionAvailable()` returns false. No new dependency.

### IPC surface

| Channel | Direction | Purpose |
|---|---|---|
| `claude:getConfig` | r→m | Prefs + `hasKey: bool` + key last-4. Never the key. |
| `claude:setConfig` | r→m | Patch prefs. |
| `claude:setApiKey` | r→m | Store/replace/clear the encrypted key. |
| `claude:run` | r→m | `{ prompt, conversationId, contextNoteIds, overrides }` → `runId`. |
| `claude:cancel` | r→m | Abort a run. |
| `claude:chunk` | m→r | `{ runId, type: 'text'\|'thinking', delta }`. |
| `claude:tool` | m→r | `{ runId, name, summary, status }` for the transcript. |
| `claude:done` | m→r | `{ runId, text, usage, stopReason, error }`. |
| `claude:saveOutput` | r→m | Persist a completed turn (`inline` or `note`). |

`preload.js` gains the matching `window.api.*` wrappers plus `onClaudeChunk` / `onClaudeTool` / `onClaudeDone` listener-with-unsubscribe helpers, following the `onMcpConfirmRequest` shape (`preload.js:102-108`).

Existing `mcp:confirm-request` / `mcp:confirm-response` are reused unchanged for tool confirmation — the modal does not need to know which caller asked.

### Save paths

- **New note** — `projectService.createFile(name, tags, { parentId })` then `writeFile`, i.e. the `create_note` tool. Default tags `['ai']`; `parentId` set to the source note when the run came from a prompt block, so answers nest under their question in the FILES tree. Name collisions resolve through the existing `uniqueFilename` (`project-service.js:570`).
- **Inline** — renderer-side `replaceAnswer()` on the editor buffer, then the ordinary save path. No new write mechanism.
- Both auto-commit through `ProjectService`. Nothing new is needed for git.

## Security Considerations

- **API key never crosses to the renderer.** `claude:getConfig` returns `hasKey` and the last four characters only. `setApiKey` is write-only.
- **Redact on the way to the log.** `git:log` is rendered in the log panel and can be copied; strip `sk-ant-[A-Za-z0-9_-]+` from every message `ClaudeService` emits.
- **Prompt content is user data.** It goes to the Anthropic API and nowhere else. No telemetry, no error reporting that includes note bodies.
- **Writes are gated twice** — `claudeAllowWrite` must be on, and `preflight` still applies the per-tool disable list and confirm prompt.
- **No ambient filesystem access.** `ClaudeService` gets no `fs` handle and no `ProjectService` reference; the vault tools are its only reach, and they are already constrained to the open project.
- **Bound the loop.** Cap tool iterations per turn (default 25) and total output tokens per run; surface the cap in the panel rather than looping silently. Every run is abortable via `AbortController`.
- **Off by default, and only when a project is open** — matching `maybeStartMcp` (`main.js:465-468`).

## Testing

- **Unit** (`tests/integration/claude-blocks.test.mjs`, node-only, following `slide-edits.test.mjs`) — block parsing, caret-to-block resolution, marker round-trip, re-run replacement, malformed/unterminated blocks.
- **Integration** (`tests/integration/claude-service.test.js`, following `mcp-bridge.test.js`) — `ClaudeService` against a stub Anthropic client: tool-use loop terminates, `ToolError` becomes `is_error` and the loop continues, `preflight` denial aborts the call, cancel unwinds mid-stream, iteration cap trips.
- **E2E** (`tests/e2e/11-claude.spec.js`) — with a mock provider injected by env var: panel toggles, prompt block runs, response lands inline, new-note output appears in FILES. No network in CI.

## Open Questions

1. **API key vs. subscription auth.** Option 1 requires an Anthropic API key (console billing) even for a user who already pays for a Claude subscription; only the CLI path uses subscription auth. If that trade is unacceptable, the CLI backend moves from v3 to v1 and the PATH problem gets solved instead (probe `which claude`, common nvm/asdf locations, and a Settings field for an explicit path). **This is the decision to make before implementation starts** — it is the only one that changes v1's shape.
2. **Automatic project context.** Should each run silently prepend a project summary (note list, tags), or rely on Claude calling `list_notes` / `search` itself? The latter is cheaper and cache-friendlier; the former is one less round trip. Recommend starting with tools-only and measuring.
3. **Conversation persistence.** Panel-only and lost on close, or written to `{userData}` per project? Not per-project-repo — transcripts are personal, same reasoning as window state.
4. **Prompt library.** Reuse `_templates/` (`template-service.js`) for saved prompts, or a separate `_prompts/` folder? Reusing templates is less machinery but muddies what a template means.

## Phasing

**v1** — `ClaudeService` + Anthropic SDK, read-only tools, Claude panel, prompt-block parsing, `Ctrl+Enter` run, panel/inline/new-note output, Settings tab, encrypted key storage, palette entries, unit + integration tests.

**v2** — Write tools enabled (behind `claudeAllowWrite` + confirm), multi-turn follow-ups with prompt caching on the system prompt and tool list, token/cost display, run history in the panel.

**v3** — CLI backend behind the same `ClaudeService` interface for subscription auth; provider dropdown in Settings.

**v4** — Prompt library, per-note system prompts via frontmatter, batch runs ("run this prompt against every note tagged X").
