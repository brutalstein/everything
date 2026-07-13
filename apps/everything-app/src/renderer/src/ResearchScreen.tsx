import { useEffect, useMemo, useState } from "react";
import type { ResearchFreshness, ResearchMode, ResearchReport, ResearchStatus } from "./types";

type Props = { reportError(message: string): void };

function request<T>(method: "GET" | "POST", path: string, body?: unknown) {
  return window.everythingApp.request<T>({ method, path, body });
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function hostLabel(url: string) {
  try { return new URL(url).hostname; } catch { return url; }
}

export function ResearchScreen({ reportError }: Props) {
  const [status, setStatus] = useState<ResearchStatus | null>(null);
  const [report, setReport] = useState<ResearchReport | null>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<ResearchMode>("technical");
  const [freshness, setFreshness] = useState<ResearchFreshness>("month");
  const [allowedDomains, setAllowedDomains] = useState("");
  const [fetchPages, setFetchPages] = useState(4);
  const [busy, setBusy] = useState(false);

  async function refreshStatus() {
    try { setStatus(await request<ResearchStatus>("GET", "/v1/research/status")); }
    catch (error) { reportError(error instanceof Error ? error.message : "Araştırma durumu okunamadı"); }
  }

  useEffect(() => { void refreshStatus(); }, []);

  async function search() {
    if (!query.trim() || busy) return;
    setBusy(true);
    try {
      setReport(await request<ResearchReport>("POST", "/v1/research/search", {
        query: query.trim(), mode, freshness, max_results: 12, fetch_pages: fetchPages,
        allowed_domains: allowedDomains.split(/[\s,]+/).map((value) => value.trim()).filter(Boolean),
        blocked_domains: [], force_refresh: false,
      }));
      await refreshStatus();
    } catch (error) {
      reportError(error instanceof Error ? error.message : "Web araştırması başarısız");
    } finally { setBusy(false); }
  }

  async function purge() {
    if (!confirm("Yerel web araştırma önbelleği temizlensin mi?")) return;
    setBusy(true);
    try {
      await request("POST", "/v1/research/cache/purge", {});
      setReport(null);
      await refreshStatus();
    } catch (error) { reportError(error instanceof Error ? error.message : "Önbellek temizlenemedi"); }
    finally { setBusy(false); }
  }

  const providerSummary = useMemo(() => status?.providers.filter((provider) => provider.available).length ?? 0, [status]);

  return (
    <div className="settings-screen research-screen">
      <div className="settings-header">
        <div><h1>Web Research</h1><p>Güncel kaynakları yerel önbellek, kaynak çeşitliliği ve açık citation kimlikleriyle araştır.</p></div>
        <div className="header-actions"><button className="secondary" disabled={busy} onClick={() => void refreshStatus()}>Durumu yenile</button><button className="secondary danger" disabled={busy} onClick={() => void purge()}>Önbelleği temizle</button></div>
      </div>

      <section className="research-status-grid">
        <article><strong>{status?.enabled ? "Etkin" : "Kapalı"}</strong><span>Araştırma politikası</span></article>
        <article><strong>{providerSummary}/{status?.providers.length ?? 0}</strong><span>Kullanılabilir sağlayıcı</span></article>
        <article><strong>{status?.cached_queries ?? 0}</strong><span>Önbellek sorgusu</span></article>
        <article><strong>{formatBytes(status?.cache_bytes ?? 0)}</strong><span>Yerel cache</span></article>
      </section>

      <section className="research-query-card">
        <div className="research-query-row">
          <input value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => event.key === "Enter" && void search()} placeholder="Güncel teknik doküman, API, CVE veya yaklaşım araştır…" />
          <button className="primary" disabled={!query.trim() || busy} onClick={() => void search()}>{busy ? "Araştırılıyor…" : "Araştır"}</button>
        </div>
        <div className="research-filters">
          <label>Mod<select value={mode} onChange={(event) => setMode(event.target.value as ResearchMode)}><option value="technical">Teknik</option><option value="general">Genel</option><option value="news">Haber</option><option value="academic">Akademik</option></select></label>
          <label>Tazelik<select value={freshness} onChange={(event) => setFreshness(event.target.value as ResearchFreshness)}><option value="day">24 saat</option><option value="week">1 hafta</option><option value="month">1 ay</option><option value="year">1 yıl</option><option value="any">Tümü</option></select></label>
          <label>Okunacak sayfa<input type="number" min={0} max={12} value={fetchPages} onChange={(event) => setFetchPages(Number(event.target.value))} /></label>
          <label>Yalnız alan adları<input value={allowedDomains} onChange={(event) => setAllowedDomains(event.target.value)} placeholder="docs.rs, github.com" /></label>
        </div>
      </section>

      {status?.warnings.map((warning) => <div className="inline-warning" key={warning}>{warning}</div>)}
      {status && <section className="research-provider-list">{status.providers.map((provider) => <article key={provider.provider}><span className={`status-dot ${provider.available ? "success" : "warning"}`} /><div><strong>{provider.provider}</strong><p>{provider.detail}</p></div></article>)}</section>}

      {report && (
        <section className="research-results">
          <div className="section-title-row"><div><h2>{report.query}</h2><p>{report.sources.length} kaynak · {report.provider_count} sağlayıcı · arama {report.search_millis} ms · sayfa okuma {report.fetch_millis} ms · {report.cache_hits} cache hit</p></div></div>
          {report.warnings.map((warning) => <div className="inline-warning" key={warning}>{warning}</div>)}
          <div className="research-source-list">
            {report.sources.map((source) => (
              <article className="research-source-card" key={`${source.citation_id}-${source.canonical_url}`}>
                <div className="research-source-meta"><span className="citation-badge">{source.citation_id}</span><span>{source.provider}</span><span>{hostLabel(source.canonical_url)}</span>{source.from_cache && <span>cache</span>}{source.published_at && <span>{source.published_at}</span>}</div>
                <h3>{source.title || source.canonical_url}</h3>
                <p>{source.extracted_text || source.snippet || "Özet alınamadı."}</p>
                <div className="research-source-actions"><button className="text-button" onClick={() => void window.everythingApp.openExternal(source.canonical_url)}>Kaynağı aç</button><code>{source.content_hash.slice(0, 12)}</code></div>
              </article>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
