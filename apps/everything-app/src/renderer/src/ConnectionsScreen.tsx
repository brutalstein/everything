import { useMemo, useState } from "react";
import type {
  ConnectorActionDescriptor,
  ConnectorActionResponse,
  ConnectorAuditRecord,
  ConnectorDescriptor,
  ConnectorProvider,
  OAuthStartResponse,
} from "./types";
import { JsonSchemaForm } from "./JsonSchemaForm";

type Props = {
  connectors: ConnectorDescriptor[];
  refresh(): Promise<void>;
  reportError(message: string): void;
};

const providerVariants: ConnectorProvider[] = ["Gmail", "Spotify", "Instagram", "TikTok", "GitHub"];
const providerSlug = (provider: ConnectorProvider) => provider.toLowerCase();

function request<T>(method: "GET" | "POST" | "DELETE", path: string, body?: unknown) {
  return window.everythingApp.request<T>({ method, path, body });
}

function pretty(value: unknown) {
  return JSON.stringify(value, null, 2);
}

export function ConnectionsScreen({ connectors, refresh, reportError }: Props) {
  const [provider, setProvider] = useState<ConnectorProvider>("Gmail");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [accessToken, setAccessToken] = useState("");
  const [scopes, setScopes] = useState("");
  const [busy, setBusy] = useState("");
  const [selected, setSelected] = useState<{ connector: ConnectorDescriptor; action: ConnectorActionDescriptor } | null>(null);
  const [actionInput, setActionInput] = useState("{}");
  const [lastResult, setLastResult] = useState<ConnectorActionResponse | null>(null);
  const [audits, setAudits] = useState<ConnectorAuditRecord[]>([]);
  const [showAudits, setShowAudits] = useState(false);

  const current = useMemo(
    () => connectors.find((connector) => connector.provider === provider),
    [connectors, provider],
  );
  const selectedMissingScopes = selected
    ? selected.action.required_scopes.filter((scope) => !selected.connector.granted_scopes.includes(scope))
    : [];

  async function configure() {
    if (!clientId.trim() && !accessToken.trim()) return;
    setBusy("configure");
    try {
      await request<ConnectorDescriptor>("POST", "/v1/connectors", {
        provider,
        client_id: clientId.trim(),
        client_secret: clientSecret.trim() || null,
        access_token: accessToken.trim() || null,
        scopes: scopes.split(/[ ,\n]+/).map((value) => value.trim()).filter(Boolean),
        metadata: {},
      });
      setClientSecret("");
      setAccessToken("");
      await refresh();
    } catch (error) {
      reportError(error instanceof Error ? error.message : "Bağlantı ayarı kaydedilemedi");
    } finally {
      setBusy("");
    }
  }

  async function connect(connector: ConnectorDescriptor) {
    setBusy(`oauth:${connector.provider}`);
    try {
      const response = await request<OAuthStartResponse>(
        "POST",
        `/v1/connectors/${providerSlug(connector.provider)}/oauth/start`,
        { provider: connector.provider, force_consent: false },
      );
      await window.everythingApp.openExternal(response.authorization_url);
    } catch (error) {
      reportError(error instanceof Error ? error.message : "OAuth başlatılamadı");
    } finally {
      setBusy("");
    }
  }

  async function toggleAudits() {
    if (showAudits) { setShowAudits(false); return; }
    setBusy("audits");
    try {
      setAudits(await request<ConnectorAuditRecord[]>("GET", "/v1/connectors/audits?limit=100"));
      setShowAudits(true);
    } catch (error) {
      reportError(error instanceof Error ? error.message : "Bağlantı işlem geçmişi okunamadı");
    } finally {
      setBusy("");
    }
  }

  async function disconnect(connector: ConnectorDescriptor) {
    if (!confirm(`${connector.display_name} bağlantısı ve yerel tokenları silinsin mi?`)) return;
    setBusy(`disconnect:${connector.provider}`);
    try {
      await request("DELETE", `/v1/connectors/${providerSlug(connector.provider)}`);
      await refresh();
    } catch (error) {
      reportError(error instanceof Error ? error.message : "Bağlantı kaldırılamadı");
    } finally {
      setBusy("");
    }
  }

  function chooseAction(connector: ConnectorDescriptor, action: ConnectorActionDescriptor) {
    setSelected({ connector, action });
    setActionInput(defaultInput(action.action_id));
    setLastResult(null);
  }

  async function executeAction(dryRun = false) {
    if (!selected) return;
    if (selectedMissingScopes.length > 0) {
      reportError(`Bu eylem için bağlantıyı şu izinlerle yeniden kur: ${selectedMissingScopes.join(", ")}`);
      return;
    }
    let input: unknown;
    try {
      input = actionInput.trim() ? JSON.parse(actionInput) : {};
    } catch {
      reportError("Eylem girdisi geçerli JSON olmalı");
      return;
    }
    const mutating = selected.action.risk !== "ReadOnly";
    const approved = !mutating || dryRun || confirm(
      `${selected.connector.display_name} hesabında “${selected.action.title}” eylemi gerçekten çalıştırılsın mı?`,
    );
    if (!approved) return;
    setBusy(`action:${selected.action.action_id}`);
    try {
      const result = await request<ConnectorActionResponse>(
        "POST",
        `/v1/connectors/${providerSlug(selected.connector.provider)}/actions`,
        {
          provider: selected.connector.provider,
          action_id: selected.action.action_id,
          input,
          approval_granted: mutating && !dryRun,
          dry_run: dryRun,
          idempotency_key: mutating && !dryRun
            ? `desktop-${selected.connector.provider}-${selected.action.action_id}-${Date.now()}`
            : null,
        },
      );
      setLastResult(result);
      await refresh();
    } catch (error) {
      reportError(error instanceof Error ? error.message : "Eylem çalıştırılamadı");
    } finally {
      setBusy("");
    }
  }

  return (
    <div className="settings-screen">
      <div className="settings-header">
        <div>
          <h1>Bağlantılar</h1>
          <p>Hesaplarını yalnızca resmi API ve OAuth akışlarıyla bağla. Tokenlar işletim sisteminin güvenli kasasında tutulur.</p>
        </div>
        <div className="header-actions"><button className="secondary" disabled={busy !== ""} onClick={() => void toggleAudits()}>{showAudits ? "Geçmişi kapat" : "İşlem geçmişi"}</button><button className="secondary" onClick={() => void refresh()}>Yenile</button></div>
      </div>

      <div className="connection-grid">
        {connectors.map((connector) => (
          <article className="connection-card" key={connector.provider}>
            <div className="connection-heading">
              <div className="provider-mark">{connector.display_name.slice(0, 1)}</div>
              <div>
                <h3>{connector.display_name}</h3>
                <span className={`connection-status ${connector.connected ? "connected" : ""}`}>{connector.status}</span>
              </div>
            </div>
            <p>{connector.description}</p>
            {connector.account_label && <div className="account-label">{connector.account_label}</div>}
            {connector.last_error && <div className="inline-warning">{connector.last_error}</div>}
            <div className="connection-actions">
              {!connector.configured && (
                <button className="secondary" onClick={() => setProvider(connector.provider)}>Ayarla</button>
              )}
              {connector.configured && !connector.connected && (
                <button className="primary" disabled={busy !== ""} onClick={() => void connect(connector)}>Hesabı bağla</button>
              )}
              {connector.connected && (
                <button className="secondary danger" disabled={busy !== ""} onClick={() => void disconnect(connector)}>Bağlantıyı kaldır</button>
              )}
            </div>
            {connector.connected && (
              <div className="action-chip-row">
                {connector.actions.map((action) => (
                  <button key={action.action_id} onClick={() => chooseAction(connector, action)}>
                    {action.title}<span>{action.risk === "ReadOnly" ? "oku" : "onay"}</span>
                  </button>
                ))}
              </div>
            )}
            <details>
              <summary>API sınırları</summary>
              <ul>{connector.limitations.map((item) => <li key={item}>{item}</li>)}</ul>
            </details>
          </article>
        ))}
      </div>

      {showAudits && (
        <section className="connector-audit-panel">
          <div className="section-title-row"><div><h2>Bağlantı işlem geçmişi</h2><p>Girdi içeriği yerine yalnız hash, risk, onay ve sonuç metadata'sı tutulur.</p></div></div>
          <div className="connector-audit-list">
            {audits.map((audit) => <article className="execution-row" key={audit.audit_id}><span className={`execution-status status-${audit.status.toLowerCase()}`}>{audit.status}</span><div><strong>{audit.provider} · {audit.action_id}</strong><p>{new Date(audit.started_at_epoch_millis).toLocaleString()} · {audit.risk} · {audit.approval_granted ? "onaylı" : "onaysız/salt-okunur"}{audit.error_code ? ` · ${audit.error_code}` : ""}</p></div><details><summary>Kanıt</summary><pre>{pretty(audit)}</pre></details></article>)}
            {audits.length === 0 && <div className="panel-empty">Henüz bağlantı işlemi yok.</div>}
          </div>
        </section>
      )}

      <section className="connector-config">
        <div>
          <h2>Uygulama kimliği</h2>
          <p>Sağlayıcının developer panelinde bir desktop/web uygulaması oluşturup bilgileri buraya gir.</p>
        </div>
        <label>Sağlayıcı<select value={provider} onChange={(event) => setProvider(event.target.value as ConnectorProvider)}>{providerVariants.map((item) => <option key={item}>{item}</option>)}</select></label>
        <label>Client ID<input value={clientId} onChange={(event) => setClientId(event.target.value)} placeholder={current?.configured ? "Değiştirmek için yeni Client ID" : "Client ID"} /></label>
        <label>Client secret <span>(gereken sağlayıcılarda)</span><input type="password" value={clientSecret} onChange={(event) => setClientSecret(event.target.value)} autoComplete="new-password" /></label>
        {provider === "GitHub" && <label>Fine-grained access token <span>(önerilen, OS kasasında saklanır)</span><input type="password" value={accessToken} onChange={(event) => setAccessToken(event.target.value)} autoComplete="new-password" placeholder="github_pat_…" /></label>}
        <label>Scopes <span>(boş bırakırsan güvenli varsayılan)</span><textarea rows={3} value={scopes} onChange={(event) => setScopes(event.target.value)} /></label>
        <div className="callback-copy">Callback: <code>{current?.metadata.oauth_callback_base ?? "http://127.0.0.1:43821/v1/connectors/oauth/callback"}/{providerSlug(provider)}</code></div>
        <button className="primary" disabled={(!clientId.trim() && !accessToken.trim()) || busy !== ""} onClick={() => void configure()}>Kaydet</button>
      </section>

      {selected && (
        <div className="action-drawer">
          <div className="section-title-row">
            <div><h2>{selected.action.title}</h2><p>{selected.action.description}</p></div>
            <button className="icon-button" onClick={() => setSelected(null)}>×</button>
          </div>
          <div className={`risk-banner ${selected.action.risk !== "ReadOnly" ? "warning" : ""}`}>
            Risk: {selected.action.risk} · {selected.action.risk === "ReadOnly" ? "Hesabı değiştirmez" : "Çalıştırmadan önce açık onay alınır"}
          </div>
          {selectedMissingScopes.length > 0 && (
            <div className="inline-warning">
              Bu bağlantı eylemin gerekli izinlerine sahip değil: <code>{selectedMissingScopes.join(", ")}</code>.
              <button
                className="text-button"
                onClick={() => {
                  setProvider(selected.connector.provider);
                  setScopes(Array.from(new Set([...selected.connector.granted_scopes, ...selectedMissingScopes])).join(" "));
                }}
              >
                Yeniden bağlantı izinlerini hazırla
              </button>
            </div>
          )}
          <JsonSchemaForm schema={selected.action.input_schema} value={actionInput} onChange={setActionInput} />
          <details className="advanced-json">
            <summary>Gelişmiş JSON</summary>
            <label>Ham eylem girdisi<textarea rows={9} value={actionInput} onChange={(event) => setActionInput(event.target.value)} /></label>
          </details>
          <div className="header-actions">
            {selected.action.supports_dry_run && <button className="secondary" disabled={selectedMissingScopes.length > 0 || busy !== ""} onClick={() => void executeAction(true)}>Önizle</button>}
            <button className="primary" disabled={selectedMissingScopes.length > 0 || busy !== ""} onClick={() => void executeAction(false)}>Çalıştır</button>
          </div>
          {lastResult && <pre className="result-preview">{pretty(lastResult)}</pre>}
        </div>
      )}
    </div>
  );
}

function defaultInput(actionId: string) {
  const inputs: Record<string, unknown> = {
    profile: {},
    repository: { owner: "", repo: "" },
    search_code: { query: "", per_page: 30 },
    issues: { owner: "", repo: "", state: "open", per_page: 30 },
    pull_requests: { owner: "", repo: "", state: "open", per_page: 30 },
    notifications: { all: false, participating: false, per_page: 30 },
    workflow_runs: { owner: "", repo: "", per_page: 30 },
    branches: { owner: "", repo: "", per_page: 30 },
    commits: { owner: "", repo: "", sha: "", path: "", per_page: 30 },
    releases: { owner: "", repo: "", per_page: 30 },
    contents: { owner: "", repo: "", path: "README.md", ref: "main" },
    security_alerts: { owner: "", repo: "", kind: "dependabot", state: "open", per_page: 30 },
    create_issue: { owner: "", repo: "", title: "", body: "", labels: [], assignees: [] },
    comment_issue: { owner: "", repo: "", number: 1, body: "" },
    create_pull_request: { owner: "", repo: "", title: "", head: "", base: "main", body: "", draft: false },
    merge_pull_request: { owner: "", repo: "", number: 1, merge_method: "squash", commit_title: "", commit_message: "" },
    workflow_dispatch: { owner: "", repo: "", workflow: "", ref: "main", inputs: {} },
    create_release: { owner: "", repo: "", tag_name: "v0.1.0", target_commitish: "main", name: "", body: "", draft: true, prerelease: false, generate_release_notes: true },
    mark_notification_read: { thread_id: "" },
    rerun_workflow: { owner: "", repo: "", run_id: 1 },
    cancel_workflow: { owner: "", repo: "", run_id: 1 },
    rest_read: { path: "/user", query: {} },
    rest_write: { method: "PATCH", path: "/repos/OWNER/REPO", query: {}, body: {} },
    graphql_read: { query: "query Viewer { viewer { login name } rateLimit { remaining resetAt } }", variables: {} },
    graphql_mutation: { query: "mutation Example($input: ChangeUserStatusInput!) { changeUserStatus(input: $input) { status { message } } }", variables: { input: { message: "" } } },
    unread_summary: { query: "is:unread in:inbox", max_results: 25 },
    latest_messages: { query: "in:inbox newer_than:1d", max_results: 10 },
    mark_read: { message_id: "" },
    archive: { message_id: "" },
    send_email: {
      to: "",
      cc: "",
      bcc: "",
      subject: "",
      body: "",
      reply_to_message_id: "",
      thread_id: "",
    },
    play: {}, pause: {}, next: {}, previous: {}, playback: {},
    queue: { uri: "spotify:track:" },
    videos: { max_count: 10 },
    creator_info: {},
    publish_video_url: {
      video_url: "https://example.com/video.mp4",
      privacy_level: "SELF_ONLY",
      title: "",
      disable_comment: false,
      disable_duet: false,
      disable_stitch: false,
      is_aigc: false,
    },
    publish_status: { publish_id: "" },
    media: { limit: 12 },
    publish_image_url: { image_url: "https://example.com/image.jpg", caption: "" },
  };
  return pretty(inputs[actionId] ?? {});
}
