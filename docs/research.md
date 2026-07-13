# Native Web Research

Everything treats current web evidence as a first-class context source beside the code graph and structured memory.

## Provider order

1. A user-local SearXNG JSON endpoint when configured.
2. Keyless Bing RSS search.
3. Wikipedia OpenSearch for general/technical discovery.
4. OpenAlex for academic mode.

Provider requests fan out concurrently. Results are canonicalized, deduplicated, lexically scored, freshness-filtered, primary-source weighted, and domain-diversified. Page retrieval is capped at six concurrent native workers so local inference retains CPU and memory headroom.

## Safety

- Public targets must use HTTPS.
- Every redirect is re-parsed, re-resolved, and pinned to a public IP.
- Private, loopback (except the configured local sidecar), link-local, multicast, documentation, and reserved ranges are rejected.
- URL fragments/userinfo/control characters, unsafe headers, unsupported MIME types, oversized responses, and excessive redirects are rejected.
- robots.txt rules are cached and respected.
- Extracted content is bounded and passed to models only as untrusted evidence with citation ID, provider, domain, retrieval time, content hash, and source-quality metadata.

## Cache

Research uses a separate SQLite WAL database with FTS5. Search and document entries have TTLs, future-schema rejection, and hard limits of 1,000 cached searches and 5,000 cached documents. Expired entries and their FTS rows are removed together.

## Automatic policy

Deep mode researches external/current tasks by default. Fast and Balanced activate research when task language requests sources, current/latest information, standards, APIs, SDKs, releases, vulnerabilities, dependencies, comparisons, recommendations, or GitHub information. `[offline]`, `local only`, or equivalent operator language disables networking.
