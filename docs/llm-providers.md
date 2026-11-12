# LLM provider research (LLM-01)

## Decisions accepted 2026-11-12

These override `docs/ARCHITECTURE.md` and `docs/TASKS.md` where they conflict.

1. Do not build `LLM-03` (Claude CLI). Claude is reachable only through the optional Advanced `api` transport, with the key stored in the Keychain.
2. Cursor is dropped from v1. Do not build the Cursor half of `LLM-05`. Gemini is skipped too.
3. v1 subscription transports are `codex-cli` and `grok-cli`. Ollama is the offline fallback. API keys remain an optional Advanced transport.
4. Unchanged: hold `⌥Space` dictation hotkey, dictation audio kept by default, macOS 14.4 or newer, and every dictation is logged in the app.

`TransportId` is `'codex-cli' | 'grok-cli' | 'ollama' | 'api'`.

Verified on this machine on 2026-11-12. Commands below were run against the real binaries, not shell functions. No CLI config directory was read. No permission-skipping flag was passed. Prompts were the single sentence `Reply with the single word pong.` via stdin or a temp file, in an empty `0700` temp directory, except Cursor, whose help only accepts the prompt as an argument.

## Verdicts

| Transport | Installed | Headless result | Terms | Ship? |
|---|---|---|---|---|
| `claude-cli` | yes, `2.1.322` | works | **no-go** | Do not build `LLM-03`. Advanced `api` only |
| `codex-cli` | yes, `0.167.0` | ignore-config run did not load MCP; the turn failed on the default model | **go, pending a pinned model** | Build `LLM-04` after `LLM-02`. Pass `codexModel` via `--model` |
| `grok-cli` | yes, `1.0.77` | works | **go** | Build the Grok half of `LLM-05` |
| `cursor-cli` | binary exists, `2026.11.05-86047df` | print mode says not authenticated, including the retry | **unavailable** | Dropped from v1 |
| `gemini-cli` | yes, `0.37.1` | auth rejected | **unavailable** | Skipped |
| `ollama` | yes, `0.40.13`, server up | local HTTP works | **go** | Build `LLM-06`. The only installed model is not offline |

## How the binaries were resolved

`zsh -lic 'whence -av …'` on 2026-11-12:

| Name | What it is |
|---|---|
| `claude` | A shell function in `~/.zshrc`, and also `/Users/cinematicgenius007/.local/bin/claude`. Call the file path. Never invoke the function. |
| `codex` | `/opt/homebrew/bin/codex` |
| `grok` | `/Users/cinematicgenius007/.grok/bin/grok` (also linked from `~/.local/bin/grok`) |
| `agent` | The same Grok binary. It is not Cursor. |
| `cursor-agent` | `/Users/cinematicgenius007/.local/bin/cursor-agent` |
| `cursor` | Not on `PATH` |
| `gemini` | `/opt/homebrew/bin/gemini` |
| `ollama` | `/usr/local/bin/ollama` |

## Claude Code — no-go

Binary: `/Users/cinematicgenius007/.local/bin/claude` · version `2.1.322 (Claude Code)`.

### Terms

[Consumer Terms](https://assets.claude.com/43c13f51a6d9755341433f79cf63c966ed9200ea.pdf) prohibit access "through automated or non-human means, whether through a bot, script, or otherwise" except via an Anthropic API key or where Anthropic explicitly permits it.

[Claude Code legal and compliance](https://code.claude.com/docs/en/legal-and-compliance) says OAuth is for ordinary use of Claude Code and other native Anthropic apps, and that third-party developers may not route requests through Free, Pro, Max, or Ultra credentials. API-key auth is the permitted automation path.

Minutae spawning `claude -p` under a subscription login is automated use of the subscription. That is a no-go. The advanced API-key transport can still use Claude later, with the key in the Keychain. Do not pass `--bare`: that mode ignores OAuth and reads only `ANTHROPIC_API_KEY`.

### Flags that worked

```text
claude -p \
  --tools "" \
  --no-session-persistence \
  --output-format json \
  --strict-mcp-config \
  --permission-mode dontAsk \
  --permission-prompts none
```

Prompt was on stdin. There is no `--prompt-file`. `--tools ""` is the documented way to disable every built-in tool. `--model <alias>` selects the model. Do not pass `--allow-dangerously-skip-permissions` or `--dangerously-skip-permissions`.

### Sample success (redacted)

Exit 0. The JSON `result` field was `pong`. `is_error` was false. `num_turns` was 1. `permission_denials` was empty. The model reported was `claude-haiku-5-5`. Duration was about 2.6 s.

The raw JSON also contains `session_id`, `uuid`, usage, and `credential_expires_at`. A parser must keep `result` and the usage counts, and must not log the rest. `--no-session-persistence` did not stop the CLI from emitting a session id. Whether a session file was written was not checked, because that would mean reading Claude's config directory.

### Sample error

```text
error: unknown option '--not-a-real-flag'
```

Exit 1. A logged-out run and a rate-limit run were not captured. The account was signed in, and it was not logged out.

## Codex CLI — go, pending a pinned model

Binary: `/opt/homebrew/bin/codex` · version `codex-cli 0.167.0`.

### Terms

`codex exec` is the documented non-interactive command, including stdin via `-`, `--json`, `--ephemeral`, and `--skip-git-repo-check` ([CLI reference](https://learn.chatgpt.com/docs/cli/reference.md)). OpenAI's public position is that a ChatGPT login may be used from the Codex CLI, including from other tools that shell out to that CLI. The docs still recommend an API key for CI. No clause was found that forbids the user running their own installed `codex` binary on their own machine, one request at a time. Do not read `~/.codex` or its tokens.

### Flags to use

The first successful probe did not include the two ignore flags, and it loaded user MCP config. The follow-up below used the full flag set.

```text
codex exec \
  --skip-git-repo-check \
  --sandbox read-only \
  --ephemeral \
  --ignore-user-config \
  --ignore-rules \
  --json \
  --color never \
  -C <empty-temp-dir> \
  -o <last-message-file> \
  -
```

`-` reads the prompt from stdin. `--model` overrides the model. Do not pass `--dangerously-bypass-approvals-and-sandbox` or `--dangerously-bypass-hook-trust`. `--ignore-user-config` still uses `CODEX_HOME` for auth, which is the CLI's own login, not something Minutae reads.

### Sample success, without the ignore flags

Exit 0. Stdout JSONL included `thread.started`, `turn.started`, `item.completed` with `item.type` `agent_message` and `text` `pong`, then `turn.completed` with token usage. A thread id was still issued under `--ephemeral`. Stderr logged failed connections to MCP servers from the user's Codex config, including a localhost server and a remote server that asked for OAuth. The hosts and scopes are intentionally not copied here.

### Follow-up with `--ignore-user-config` and `--ignore-rules`

Same prompt, empty temp directory, plus `-o <last-message-file>`. Exit 1. Stderr had no MCP client errors. The only stderr lines were `codex_models_manager` failing to refresh the model list because the request timed out. Stdout was:

```text
{"type":"thread.started","thread_id":"…"}
{"type":"turn.started"}
{"type":"error","message":"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."}
{"type":"turn.failed","error":{"message":"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."}}
```

The last-message file was empty. User MCP config was not loaded. Dropping the user config also dropped the user's model choice, and the CLI default (`gpt-6.1-sol`) is rejected for a ChatGPT login.

The model name is not hardcoded. `LLM-04` adds a `codexModel` setting and passes that value with `--model`. The default name is not chosen here. The transport's Test action runs this probe and shows the classified error, including the rejection above when the setting is empty or names a model the ChatGPT login does not accept.

### Sample error

```text
error: unexpected argument '--not-a-real-flag' found
```

Exit 2. Logged-out and rate-limit bodies were not captured.

## Grok CLI — go

Binary: `/Users/cinematicgenius007/.grok/bin/grok` · version `grok 1.0.77 (50ca579fa996) [stable]`.

Official headless docs: [Headless and scripting](https://docs.x.ai/build/cli/headless-scripting), last updated 10 June 2026. They describe `grok -p` for scripts and bots. The [X Subscription consumer terms](https://x.ai/legal/x-subscription-terms-of-service) (27 October 2026) were read for an automated-access ban. The restrictions found are the usual ones: no reverse engineering, no resale, no using the service to train a competing model. They do not say that running the official CLI headlessly on your own machine is forbidden. Do not pass `--always-approve` or `--permission-mode bypassPermissions`.

### Flags that worked

```text
grok \
  --prompt-file <file> \
  --output-format json \
  --tools "" \
  --disable-web-search \
  --no-subagents \
  --max-turns 1 \
  --cwd <empty-temp-dir> \
  --verbatim
```

`--model` selects the model. `-p` / `--single` also exists, but that puts the prompt in argv, so prefer `--prompt-file`. Do not pass `--continue`, `--resume`, or `--session-id` when the intent is a fresh call. This run still returned a `sessionId`. Whether `~/.grok/sessions` grew was not checked.

### Sample success (redacted)

Exit 0. JSON `text` was `pong`, `stopReason` was `end_turn`, `num_turns` was 1. The model reported was `grok-4.7-build`. The CLI reported a dollar cost. Input tokens were about 15k even with `--tools ""`, so the empty tool list did not remove the system prompt.

### Sample errors

Unknown flag, exit 2:

```text
error: unexpected argument '--not-a-real-flag' found
```

`grok usage` (this does not open a session), exit 1:

```text
Error: `grok usage` can't show your plan's usage here yet. Run /usage inside grok instead.
```

Logged-out and rate-limit bodies were not captured.

## Cursor CLI — unavailable

Binary: `/Users/cinematicgenius007/.local/bin/cursor-agent` · version `2026.11.05-86047df`. There is no `cursor` binary on `PATH`. The `agent` command is Grok, not Cursor.

`cursor-agent --help` documents `-p` / `--print`, `--output-format text|json|stream-json`, `--mode ask` (read-only Q&A), `--model`, `--sandbox enabled|disabled`, and `--workspace`. It does not document a way to disable tools entirely or to skip session persistence. `--print` says it has access to all tools, including write and shell, unless a read-only mode is set. There is no stdin prompt flag. Do not pass `--force` or `--yolo`.

### What happened

`cursor-agent status` exited 0:

```text
✓ Login successful!
Logged in (unable to fetch user details)
```

The headless call exited 1:

```text
cursor-agent -p --mode ask --output-format json --sandbox enabled \
  --workspace <empty-temp-dir> -- "Reply with the single word pong."
```

```text
Error: Authentication required. Please run 'agent login' first, or set CURSOR_API_KEY environment variable.
```

Status and print mode disagree. A retry without `--sandbox`, with `--trust` and `--output-format text`, exited 1 with the same message:

```text
Error: Authentication required. Please run 'agent login' first, or set CURSOR_API_KEY environment variable.
```

That is the final verdict for v1: unavailable. Cursor is dropped. The help text telling the user to run `agent login` is misleading on this machine, because `agent` is Grok.

### Sample error

Unknown flag, exit 1:

```text
error: unknown option '--not-a-real-flag'
```

## Gemini CLI — unavailable

Binary: `/opt/homebrew/bin/gemini` · version `0.37.1`.

Headless flags from `--help`: `-p` / `--prompt` (appended to stdin), `--model`, `--output-format text|json|stream-json`, `--approval-mode plan` (read-only). There is no flag that disables tools outright, and no flag that disables session persistence. `--resume` is how a session is continued, so a one-shot call must not pass it.

The official CLI docs do describe headless mode for scripts. This install cannot use it. The probe exited 1 before any model output:

```text
Error authenticating: IneligibleTierError: This client is no longer supported
for Gemini Code Assist for individuals. To continue using Gemini, please
migrate to the Antigravity suite of products: https://antigravity.google
reasonCode: UNSUPPORTED_CLIENT
```

Before that, the process printed a long list of skill-file paths under `~/.agents/skills` and `~/.gemini/skills`. A Minutae transport must not run Gemini until auth works, and it must not log that preamble.

Gemini is skipped for v1. Do not build a Gemini transport.

### Sample error

`--not-a-real-flag` exits 1 and reprints the help, starting with `Unknown arguments: not-a-real-flag, notARealFlag`.

## Ollama — go, but not offline on this machine

Binary: `/usr/local/bin/ollama` · version `0.40.13`. The server was already running.

`ollama list`:

```text
NAME                ID              SIZE    MODIFIED
gemma4:31b-cloud    ef09f235533c    -       2 months ago
```

`GET http://127.0.0.1:11434/api/tags` returned that same model. Its record says `remote_host` `https://ollama.com` and `remote_model` `gemma4:31b`. A completion through it would leave the machine. The architecture's offline fallback is not actually available until a local model is pulled. `LLM-06` can still talk to `localhost:11434`. It must tell the user when Ollama is down, and when the selected model is a cloud model.

Ollama is local software. There is no vendor subscription term to violate by calling it. Model licenses still apply to whatever the user pulls.

The completion API, for the later task, is `POST /api/chat` with `"stream": false` and `"format": "json"`. That call was not made.

## What was not verified

- Logged-out stderr for Claude, Codex, and Grok. The accounts were left signed in.
- A rate-limit body for every CLI.
- That `--no-session-persistence` (Claude), `--ephemeral` (Codex), or a fresh Grok call actually avoid writing a session file.
- A Codex completion that both ignores user config and returns text. The ignore-config run was executed. It proved MCP was not loaded, then failed because the default model is `gpt-6.1-sol`. The model name is a `codexModel` setting, supplied separately.
- An Ollama completion, local or cloud.
- That `ps` hides the prompt. Claude, Codex, Grok, and Gemini were given the prompt on stdin or in a file. Cursor's help has no stdin prompt, so that probe used argv.
