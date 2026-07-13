# Official Account Connectors

Everything connects to external accounts only through provider-supported OAuth and HTTP APIs. It does not scrape browser sessions, copy cookies, automate passwords, or bypass provider review requirements.

## Common flow

1. Open **Connections** in Everything Desktop.
2. Create an application in the provider's developer console.
3. Copy the callback URL shown by Everything into the provider configuration.
4. Enter the client ID, and a client secret only when that provider requires one.
5. Keep the scope field empty to use Everything's conservative defaults, or enter an explicitly reviewed subset.
6. Save, choose **Connect account**, and complete the provider's consent page in the system browser.

Access and refresh tokens, client secrets, and temporary PKCE verifiers are stored in the operating-system credential vault. SQLite stores non-secret configuration, scope names, expiry metadata, audit records, and one-time OAuth state hashes. OAuth state is provider-bound, expires, and can be consumed only once.

Environment-backed secrets are supported for headless deployments using the normalized form `EVERYTHING_SECRET_<PROVIDER>_<NAME>`. The desktop flow should normally use the operating-system vault instead.

## Gmail

Conservative default scope: `https://www.googleapis.com/auth/gmail.readonly`.

Available MVP actions:

- account profile and mailbox counters;
- unread message identifier summary using Gmail search syntax;
- bounded recent-message metadata lookup;
- mark a message read and archive a message when the connection has `gmail.modify`;
- send a new message or thread reply when the connection has `gmail.send`.

Everything uses installed-application OAuth with PKCE and a loopback callback. Local routines poll at the configured schedule. Gmail push notifications require Google Cloud Pub/Sub infrastructure and are intentionally not emulated by the local runtime. Write scopes are never silently added: the user must explicitly reconnect with `gmail.modify` and/or `gmail.send`, and every write remains approval-gated unless the exact action is granted to a narrow `ActWithinPolicy` routine. Recipient/header validation, bounded payloads, thread identifiers, idempotency, and audit controls apply before the API call.

## Spotify

Conservative default scopes cover only profile, playback-state inspection, current-item inspection, and private-playlist reads. Play, pause, skip, and queue require an explicit reconnect with `user-modify-playback-state`; they are account mutations and remain approval-gated unless a routine has a narrow `ActWithinPolicy` grant for that exact action.

Spotify playback-control endpoints require an eligible Premium account. Provider development mode and user allowlists can also limit who can authorize an unpublished application.

## TikTok

The connector can read basic creator information and recent videos, inspect creator posting capabilities, initialize a Content Posting API request from a verified HTTPS media URL, and poll publish status.

The conservative default connection omits `video.publish`. Publishing requires an explicit reconnect with that scope, provider approval, creator consent, creator-info checks, and provider-mandated privacy/interaction controls. Unaudited applications can be restricted to private posts. Everything does not weaken those restrictions and does not upload from arbitrary local paths through undocumented endpoints.

## Instagram

The connector targets eligible professional accounts through the official Instagram platform. It can read profile/media metadata and perform the documented two-step image-container and `media_publish` flow.

The conservative default connection requests only `instagram_business_basic`. Publishing requires an explicit reconnect with `instagram_business_content_publish`, an eligible professional account, approved permissions, and a publicly reachable media URL that Meta can fetch. Personal accounts or unavailable permissions remain unsupported rather than falling back to browser automation.

## Action safety

Every connector action has a typed descriptor containing:

- risk class;
- required scopes;
- JSON input schema;
- dry-run support;
- idempotency behavior.

Read-only actions can run under read permission. The desktop performs a required-scope preflight and blocks actions or routines whose connection lacks a declared scope. External publishing and account mutations require explicit approval by default. Autonomous execution is allowed only when the routine policy names the exact `provider:action` pair, has a non-zero external-write budget, and uses `ActWithinPolicy`. All attempts are persisted in the connector audit log with sanitized errors and without token material.

Connector input is validated against the action schema before execution and capped at 256 KiB. Redirect following is disabled for API calls, so an allowed provider host cannot redirect credentials to a different host. Idempotency keys are normalized and bounded before persistence.

Provider URLs use HTTPS and exact host allowlists. Redirects are restricted to HTTPS, response sizes and execution time are bounded, user curl configuration is disabled, and credentials are passed through stdin/config rather than command-line arguments where practical.


## GitHub

GitHub can use a personal access token or provider OAuth. Read actions cover profile/rate limits, repositories, branches, commits, contents, code search, issues, pull requests, notifications, workflow runs, releases, and security alerts. Typed mutations cover issues/comments, pull requests/merge, workflow dispatch/rerun/cancel, releases, and notification state.

For operations not yet represented by a typed action, `rest_read` and approval-gated `rest_write` expose bounded relative paths on the official REST API. `graphql_read` and `graphql_mutation` expose the official GraphQL endpoint with strict query/mutation separation. Arbitrary hosts, redirects, installation-token minting, raw token output, and credential-bearing shell/network tools remain blocked.

GitHub token scopes, fine-grained repository permissions, SSO authorization, rulesets, branch protection, API rate limits, and Actions policy remain authoritative. Mutations require explicit approval by default and are protected by connector idempotency and durable audit records.
