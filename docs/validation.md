# Astralink validation (2026-10-14)

This is a snapshot of checks run on the developer Windows machine. Do not infer that the imported hosts are generally reachable or that deployment operations were tested.

## Verified

- Rust MSVC toolchain and Windows SDK installed. `cargo test --lib --offline`: 67 passed, 2 opt-in local tests ignored. Opt-in Tabby sample test: 21 profiles, 11 ready, 10 missing credentials, no passwords printed.
- `npm test --workspace @astralink/engine`: 47 passed including real ssh2 loopback OTP, shell, jump-forward, SFTP access control, lifecycle and upstream 10-tool schema tests.
- `npm test`: 45 frontend tests passed. `npm run build`: typecheck and Vite bundle, with no test lifecycle hook.
- `npm audit --audit-level=high`: 0 vulnerabilities. Upstream and engine MCP SDK resolve to 1.32.1.
- Real HTTP MCP client using the authorized `192.168.97.166` private-key connection: tools/list returned 10; read-only `hostname && id && uname -s` returned hostname and Linux user info.
- Native debug Tauri app started, connected to localhost control service, and discovered local Tabby/Xshell/Netcatty/OpenSSH source locations. Native preview of 21 Tabby profiles and 5 existing SSH MCP JSON profiles succeeded; no real passwords were sent to the webview preview.
- Native test (isolated local data directory) against an SSH loopback server emitted an OTP dialog. Submitting the correct code returned '测试通过'; after importing an authorized existing private-key profile, the Rust→Node engine connection test completed successfully (read-only hostname/id).
- One-click Codex registration was tested against an isolated CODEX_HOME. A separate Codex app-server process discovered 10 Astralink MCP tools from the registered localhost URL. The user's real Codex config was not modified by this validation.
- Windows NSIS package produced at `src-tauri/target/release/bundle/nsis/Astralink_0.1.0_x64-setup.exe` (28.12 MiB). Silent installation into an isolated directory confirmed bundled Node runtime and engine files. The installed app started its MCP engine and rendered the per-client Codex guide with a placeholder, after Windows extended-length resource paths were normalized for Node.

## Boundaries

- JumpServer production MFA, target asset choice, deploy commands, and file uploads to actual remote hosts were not run; only local fixture and authorized readonly host checks.
- Xshell on this machine had a user directory but no `.xsh` sessions. Synthetic nested-directory, encoding and ZIP-compatible `.xts` tests passed. Proprietary/non-ZIP `.xts` requires additional sample work.
- Netcatty direct encrypted app vault import is not supported: use CSV export; local backup metadata is not equivalent to current config.
- OpenSSH Include/Match and unsupported proxy directives warn and disable affected entries rather than silently changing routing.
- Shell-mode upstream SFTP remains rejected. Local file access is restricted to engine cwd and explicit allowedLocalPaths; remote path allowlists are optional and lexical, not a shell sandbox.
- Default connectionName is permitted only when an enabled connection named `default` exists; otherwise supply connectionName explicitly.
- Full-node bundle is larger than a pure-Rust alternative; install size, cold start and idle memory were not yet measured under a final installer.

## Client connection guides

- Update detection compares installed semver with GitHub's latest stable release, checks platform assets, and distinguishes network failure from current status. UI tests cover the release parser; a live WebView fetch of `v0.1.0` returned five assets and classified installed `0.1.0` as current. The local `0.1.1` Windows NSIS bundle was built with the new capability. Installed-app interaction and a future-release upgrade prompt are not yet verified. Opening releases is limited to the repository's fixed URL; the app does not download or silently install updates.

- Importer fixtures cover FinalShell nested groups, NyaTerm v1 JSON password refs and `.redb` SSH metadata, MobaXterm bookmarks, SecureCRT XML, Electerm groups, WindTerm plaintext/encrypted auto-login and HomeDir key paths, Termius encrypted V8 records and LevelDB copy, and authenticated `.nya` ZIP/redb backup with hash and credential extraction. Only SSH profiles are converted; inline private keys require manual local file selection. The real local NyaTerm `.redb` preview returned 3 records (1 needs credentials, 2 unsupported/disabled), while the real Termius IndexedDB returned 0 hosts. Both source hashes were unchanged, and temporary copies were removed. No actual import from user data or populated Termius store has been performed.
- Encrypted WebDAV snapshot unit tests covered credential roundtrip, wrong passphrase, tampering, and HTTPS-only URL validation. Backend commands enforce explicit remote fingerprint checks and conditional writes; live WebDAV authentication, upload, and restore remain unverified without a user-chosen endpoint. Synchronization is manual, not background.

- The settings page exposes per-client connection guidance with a token placeholder. Claude Desktop and WorkBuddy use the installed stdio bridge; user-level JSON registration preserves unrelated servers and backs up existing files. Cherry Studio uses a manual stdio JSON import with absolute path placeholders; no undocumented internal config file is written. Cloud ChatGPT is explicitly gated on a separate authenticated HTTPS gateway, not the localhost URL.
- The real token is read from OS credentials only after a native explicit copy action; the browser preview keeps this action disabled. Clipboard-write completion was verified in the native UI; independent clipboard-content read was not established due WebView permission prompts.
- Frontend snippets and native config registration were checked by unit tests. The stdio bridge used by Claude Desktop and WorkBuddy listed 10 tools and invoked `list-servers` through a real local HTTP engine; packaged Node/script paths and installed resource engine startup were verified. Claude Desktop, WorkBuddy, Cherry Studio, and cloud ChatGPT UI sign-in and tool execution have not been exercised in their own applications.
