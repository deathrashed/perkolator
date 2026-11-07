# Marketplace Authentication and Provider Connectivity

## UX contract

Every source has independent authentication and connection states. Show `Not required`, `Not configured`, `CLI missing`, `Ready`, `Expired`, `Rate limited`, `Unavailable`, `Needs access`, `Unsupported`. A source can be enabled even when auth is missing; it should show setup CTA, not block the entire UI. A valid login is not proof a given endpoint is authorised.

The source settings panel offers **Open website**, **Connect**, **Re-authenticate**, **Test access**, **Disconnect**, **View documentation**, **Choose CLI executable**, **Refresh results**, and **Enable search** as supported. `Connect` launches only a provider-documented OAuth URL, CLI login or API key setup; do not construct speculative OAuth flows. Browser callback and CLI integration must work with macOS/KDE Wayland constraints. Some sources need no login: do not force it.

## Authentication strategies

- **None:** public MCP Registry, GitHub anonymous (subject to quota), curated GitHub pages and local inventory; display no authentication requirement. Optional GitHub token improves quotas.
- **LobeHub:** detect `lhm` from PATH; `lhm auth status` and version; use `lhm login` for browser user authentication and `lhm register --base-url https://market.lobehub.com --name 'Perkolator' --source codex` for M2M registration if required by the operation. Show user vs M2M distinctly. Registration may be rate limited; never repeat automatically. Follow CLI's credentials location without reading/printing tokens unnecessarily. Do not launch login until user requests it.
- **skills.sh:** direct REST docs may call for Vercel OIDC, not an arbitrary API key. Prefer supported CLI operations; if CLI handles auth, delegate to it. Never fake direct API access.
- **SkillsMP / Glama:** document the actual official key-management page and requirements after checking docs; user supplies API key through a password/secret input stored in OS secure storage. Run minimal authenticated test request; never display the secret afterward.
- **Smithery:** use officially documented CLI login/credential flow if supported in installed release; detect `smithery` version; do not guess its CLI switches or assume JSON output.
- **GitHub index sources:** public repository reads without token where possible; optional GitHub token via secure credential storage for rate limits. Reuse one GitHub credential per identity (with user permission) across all GitHub-backed sources.
- **prompts.chat:** public GitHub and/or supported MCP discovery. Run `initialize` before MCP calls where protocol requires; authenticated functions may need login, public browse shouldn't.
- **PulseMCP:** partner API may require an approved access arrangement; mark `Needs access` and open official documentation. No unsupported workaround.

## Flow

1. User selects source and clicks `Connect` (or `Setup API key`).
2. Provider adapter returns an explicit `AuthIntent`: `OpenExternalUrl`, `SpawnCliFlow`, `PromptSecret`, `NoAuthRequired`, or `Unsupported`.
3. Show destination and permissions/scopes where known; user activates supported flow. Open native browser for OAuth, CLI flow in supported terminal/PTY if truly interactive. Never silently run registration or overwrite previously saved CLI auth.
4. On OAuth callback, validate state, PKCE, redirect target and issuer. On CLI exit, re-query provider status. On API key test, validate scoped access without dumping response secrets.
5. Save only tokens the app itself owns, in macOS Keychain / Secret Service as supported. CLI-owned auth remains CLI-owned. Settings only hold metadata/reference to credential slots.
6. Validate **search entitlement** with a limited actual catalogue request; status is `Authenticated / Search forbidden` if login worked but API is denied.
7. On expiry, show reconnect once, use refresh token when provider explicitly supports it, don't create login loops.
8. `Disconnect` revokes owned credentials if provider supports it and confirms ownership; otherwise remove local reference with clear explanation. Never remove unrelated CLI credentials without explicit user choice.

## Security

No embedded client secret in repository, example TOML, log output or subprocess args if secret can be supplied via secure environment or stdin. Redact request headers and OAuth code/PKCE verifier. Respect provider rate limits (especially LobeHub registration). User consent for publishing, installing scripts, enabling hooks or sending workspace data. No remote auth request while user is just browsing local assets.

## Required smoke tests

- Public source works without login.
- LobeHub login state distinguishes user OAuth from M2M and checks real search permission.
- Disabled source does not make HTTP/CLI call.
- Missing CLI and malformed JSON fail gracefully.
- Expired token triggers nonblocking reauthentication UI.
- Auth denial for one provider leaves others searchable.
- Key is not present in logs, config, crash report or command lines.
