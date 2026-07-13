import { useMemo, useState } from "react";
import type { AutomationDefinition, AutomationExecution, ConnectorDescriptor, SkillDescriptor } from "./types";
import { JsonSchemaForm } from "./JsonSchemaForm";

type Props = {
  automations: AutomationDefinition[];
  connectors: ConnectorDescriptor[];
  skills: SkillDescriptor[];
  refresh(): Promise<void>;
  reportError(message: string): void;
};

type ScheduleKind = "daily" | "weekly" | "interval" | "once";

const WEEKDAYS = [
  { value: 1, label: "Pzt" }, { value: 2, label: "Sal" }, { value: 3, label: "Çar" },
  { value: 4, label: "Per" }, { value: 5, label: "Cum" }, { value: 6, label: "Cmt" }, { value: 0, label: "Paz" },
];

const request = <T,>(method: "GET" | "POST" | "DELETE", path: string, body?: unknown) =>
  window.everythingApp.request<T>({ method, path, body });

function scheduleLabel(schedule: Record<string, unknown>) {
  const daily = (schedule.DailyLocal ?? schedule.DailyFixedOffset) as { hour?: number; minute?: number } | undefined;
  if (daily) return `Her gün ${String(daily.hour ?? 0).padStart(2, "0")}:${String(daily.minute ?? 0).padStart(2, "0")}`;
  const weekly = (schedule.WeeklyLocal ?? schedule.WeeklyFixedOffset) as { weekdays?: number[]; hour?: number; minute?: number } | undefined;
  if (weekly) {
    const days = (weekly.weekdays ?? []).map((day) => WEEKDAYS.find((item) => item.value === day)?.label ?? String(day)).join(", ");
    return `${days || "Haftalık"} · ${String(weekly.hour ?? 0).padStart(2, "0")}:${String(weekly.minute ?? 0).padStart(2, "0")}`;
  }
  const interval = schedule.Interval as { every_millis?: number } | undefined;
  if (interval) return `Her ${Math.round((interval.every_millis ?? 0) / 60_000)} dakika`;
  const once = schedule.Once as { at_epoch_millis?: number } | undefined;
  if (once?.at_epoch_millis) return new Date(once.at_epoch_millis).toLocaleString();
  return "Özel zamanlama";
}

function actionLabel(action: Record<string, unknown>) {
  if ("Connector" in action) {
    const value = action.Connector as { request?: { provider?: string; action_id?: string } };
    return `${value.request?.provider ?? "Connector"} · ${value.request?.action_id ?? "action"}`;
  }
  if ("Skill" in action) return `Skill · ${(action.Skill as { skill_id?: string }).skill_id ?? ""}`;
  if ("Doctor" in action) return "Everything öz-sağlık kontrolü";
  if ("Plan" in action) return "Repository planı";
  if ("Briefing" in action) return "Akıllı yerel özet";
  return "Görev";
}

function executionLabel(status: string) {
  const labels: Record<string, string> = {
    Claimed: "Sırada", Running: "Çalışıyor", AwaitingApproval: "Onay bekliyor",
    Approved: "Onaylandı", Completed: "Tamamlandı", Failed: "Yeniden denenecek", DeadLetter: "Kalıcı hata", Skipped: "Atlandı", Cancelled: "İptal edildi",
  };
  return labels[status] ?? status;
}

function executionSummary(execution: AutomationExecution) {
  if (execution.error) return execution.error;
  const output = execution.output as Record<string, unknown> | null;
  if (!output || typeof output !== "object") return "Sonuç kaydedildi";
  if (typeof output.reason === "string") return output.reason;
  if (typeof output.status === "string") return output.status;
  if (typeof output.summary === "string") return output.summary;
  return "Sonuç ve kanıtlar kaydedildi";
}

function parseInputObject(value: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

export function AutomationsScreen({ automations, connectors, skills, refresh, reportError }: Props) {
  const [name, setName] = useState("Sabah e-posta özeti");
  const [scheduleKind, setScheduleKind] = useState<ScheduleKind>("daily");
  const [hour, setHour] = useState("09");
  const [minute, setMinute] = useState("00");
  const [intervalMinutes, setIntervalMinutes] = useState("60");
  const [weekdays, setWeekdays] = useState<number[]>([1, 2, 3, 4, 5]);
  const [skillId, setSkillId] = useState("");
  const [onceAt, setOnceAt] = useState(() => {
    const date = new Date(Date.now() + 60 * 60 * 1000);
    date.setSeconds(0, 0);
    return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
  });
  const [provider, setProvider] = useState("Gmail");
  const [actionId, setActionId] = useState("unread_summary");
  const [input, setInput] = useState('{"query":"is:unread in:inbox","max_results":25}');
  const [autonomy, setAutonomy] = useState("Assist");
  const [busy, setBusy] = useState("");
  const [historyFor, setHistoryFor] = useState("");
  const [history, setHistory] = useState<AutomationExecution[]>([]);

  const selectedConnector = useMemo(() => connectors.find((item) => item.provider === provider), [connectors, provider]);
  const selectedAction = selectedConnector?.actions.find((item) => item.action_id === actionId);
  const selectedMissingScopes = selectedAction
    ? selectedAction.required_scopes.filter((scope) => !selectedConnector?.granted_scopes.includes(scope))
    : [];
  const enabledSkills = useMemo(() => skills.filter((item) => item.enabled && item.compatibility.compatible), [skills]);
  const selectedSkill = enabledSkills.find((item) => item.manifest.skill_id === skillId);

  function updateInputField(name: string, value: unknown) {
    setInput(`${JSON.stringify({ ...parseInputObject(input), [name]: value }, null, 2)}\n`);
  }

  function applyPreset(kind: "mail" | "spotify" | "runtime" | "health" | "dependencies") {
    setScheduleKind("daily");
    if (kind === "mail") {
      setName("Sabah e-posta özeti"); setProvider("Briefing"); setActionId("briefing");
      setInput(JSON.stringify({
        prompt: "Son 24 saatteki e-postaları önem, aciliyet ve gerekli eylemlere göre Türkçe özetle. Şüpheli içerikleri ve yanıtsız önemli iletileri belirt.",
        mode: "Balanced",
        sources: [{ provider: "Gmail", action_id: "latest_messages", input: { query: "in:inbox newer_than:1d", max_results: 20 } }],
      }, null, 2)); setHour("09"); setMinute("00"); setAutonomy("Assist");
    } else if (kind === "spotify") {
      setName("Akşam Spotify durumu"); setProvider("Spotify"); setActionId("playback"); setInput("{}"); setHour("19"); setMinute("00"); setAutonomy("Observe");
    } else if (kind === "runtime") {
      setName("Everything günlük öz-sağlık"); setProvider("Doctor"); setActionId("doctor"); setInput("{}");
      setHour("08"); setMinute("30"); setAutonomy("Observe");
    } else if (kind === "health") {
      setName("Repository sağlık kontrolü"); setProvider("Plan"); setActionId("plan");
      setInput('{"objective":"Repository sağlık durumunu, riskleri ve öncelikli düzeltmeleri kaynak kanıtlarıyla raporla","mode":"Deep"}');
      setHour("10"); setMinute("00"); setAutonomy("Assist");
    } else {
      setName("Haftalık bağımlılık ve güvenlik incelemesi"); setProvider("Plan"); setActionId("plan");
      setScheduleKind("weekly"); setWeekdays([1]); setHour("10"); setMinute("00"); setAutonomy("Assist");
      setInput('{"objective":"Bağımlılık, supply-chain, lisans ve güncelleme risklerini kaynak kanıtlarıyla incele; yalnız güvenli ve uygulanabilir öneriler üret","mode":"Deep"}');
    }
  }

  function buildSchedule() {
    if (scheduleKind === "interval") {
      const every = Math.max(1, Number(intervalMinutes));
      return { Interval: { every_millis: every * 60_000, anchor_epoch_millis: null } };
    }
    if (scheduleKind === "once") {
      const timestamp = new Date(onceAt).getTime();
      if (!Number.isFinite(timestamp) || timestamp <= Date.now()) throw new Error("Tek seferlik zaman gelecekte olmalı");
      return { Once: { at_epoch_millis: timestamp } };
    }
    const wallClock = { hour: Number(hour), minute: Number(minute) };
    if (!Number.isInteger(wallClock.hour) || wallClock.hour < 0 || wallClock.hour > 23 || !Number.isInteger(wallClock.minute) || wallClock.minute < 0 || wallClock.minute > 59) {
      throw new Error("Saat ve dakika geçerli aralıkta olmalı");
    }
    if (scheduleKind === "weekly") {
      if (weekdays.length === 0) throw new Error("Haftalık rutin için en az bir gün seç");
      return { WeeklyLocal: { weekdays: [...weekdays].sort(), ...wallClock } };
    }
    return { DailyLocal: wallClock };
  }

  async function createRoutine() {
    if (selectedMissingScopes.length > 0) {
      reportError(`Bu rutin için bağlantıyı şu izinlerle yeniden kur: ${selectedMissingScopes.join(", ")}`);
      return;
    }
    let parsed: Record<string, unknown>;
    try { parsed = input.trim() ? JSON.parse(input) : {}; }
    catch { reportError("Rutin girdisi geçerli JSON olmalı"); return; }
    let schedule: Record<string, unknown>;
    try { schedule = buildSchedule(); }
    catch (error) { reportError(error instanceof Error ? error.message : "Zamanlama geçersiz"); return; }
    const action = provider === "Doctor"
      ? { Doctor: {} }
      : provider === "Plan"
        ? { Plan: { objective: String(parsed.objective ?? "Repository durumunu incele"), mode: String(parsed.mode ?? "Balanced") } }
      : provider === "Briefing"
        ? { Briefing: {
            sources: Array.isArray(parsed.sources) ? parsed.sources : [],
            prompt: String(parsed.prompt ?? "Bağlı kaynakları önemli eylemler ve risklerle özetle"),
            mode: String(parsed.mode ?? "Balanced"),
          } }
        : provider === "Skill"
          ? { Skill: { skill_id: skillId, input: parsed } }
          : { Connector: { request: { provider, action_id: actionId, input: parsed, approval_granted: false, dry_run: false, idempotency_key: null } } };
    if (provider === "Skill" && !selectedSkill) { reportError("Etkin ve uyumlu bir skill seç"); return; }
    const externalWrite = provider !== "Doctor" && provider !== "Skill" && provider !== "Briefing" && provider !== "Plan" && (selectedAction?.risk ?? "ReadOnly") !== "ReadOnly";
    const workspaceMutation = provider === "Skill" && Boolean(selectedSkill?.manifest.permissions.some((permission) => /write|process|git/i.test(permission)));
    setBusy("create");
    try {
      await request<AutomationDefinition>("POST", "/v1/automations", {
        name: name.trim(), description: "Everything Desktop tarafından oluşturuldu", schedule, action,
        policy: {
          autonomy_level: autonomy, allow_workspace_mutation: workspaceMutation,
          allow_external_read: true,
          allow_external_write: externalWrite && autonomy === "ActWithinPolicy",
          approved_connector_actions: externalWrite && autonomy === "ActWithinPolicy" ? [`${provider.toLowerCase()}:${actionId}`] : [],
          budget: { max_runtime_millis: 600000, max_model_calls: 4, max_tool_invocations: workspaceMutation ? 32 : 16, max_external_writes: externalWrite ? 1 : 0, daily_execution_limit: 24 },
          consecutive_failure_threshold: 3,
          retry: { max_attempts: 3, initial_backoff_millis: 30000, max_backoff_millis: 900000, backoff_multiplier: 2 },
          missed_run_grace_millis: 900000,
          missed_run_policy: "RunOnce",
        }, enabled: true,
      });
      await refresh();
    } catch (error) { reportError(error instanceof Error ? error.message : "Rutin oluşturulamadı"); }
    finally { setBusy(""); }
  }

  async function loadHistory(item: AutomationDefinition) {
    setBusy(`history:${item.automation_id}`);
    try {
      const rows = await request<AutomationExecution[]>("GET", `/v1/automations/${encodeURIComponent(item.automation_id)}/executions?limit=50`);
      setHistoryFor(item.automation_id); setHistory(rows);
    } catch (error) { reportError(error instanceof Error ? error.message : "Rutin geçmişi okunamadı"); }
    finally { setBusy(""); }
  }

  async function runNow(item: AutomationDefinition) {
    const policy = item.policy as { allow_external_write?: boolean; allow_workspace_mutation?: boolean };
    const risky = Boolean(policy.allow_external_write || policy.allow_workspace_mutation);
    const approve = risky ? confirm("Bu rutin workspace veya dış hesapta değişiklik yapabilir. Şimdi açık onayla çalıştırılsın mı?") : false;
    setBusy(item.automation_id);
    try {
      await request<AutomationExecution>("POST", `/v1/automations/${encodeURIComponent(item.automation_id)}/run`, { approval_granted: approve });
      await refresh(); await loadHistory(item);
    } catch (error) { reportError(error instanceof Error ? error.message : "Rutin çalıştırılamadı"); }
    finally { setBusy(""); }
  }

  async function approvePending(execution: AutomationExecution) {
    if (!historyFor) return;
    if (!confirm("Bu bekleyen rutin eylemi şimdi açık onayla çalıştırılsın mı?")) return;
    setBusy(`approve:${execution.execution_id}`);
    try {
      await request<AutomationExecution>(
        "POST",
        `/v1/automations/${encodeURIComponent(historyFor)}/executions/${encodeURIComponent(execution.execution_id)}/approve`,
      );
      const rows = await request<AutomationExecution[]>(
        "GET",
        `/v1/automations/${encodeURIComponent(historyFor)}/executions?limit=50`,
      );
      setHistory(rows);
      await refresh();
    } catch (error) {
      reportError(error instanceof Error ? error.message : "Bekleyen rutin onaylanamadı");
    } finally {
      setBusy("");
    }
  }

  async function toggle(item: AutomationDefinition) {
    setBusy(item.automation_id);
    try {
      await request("POST", "/v1/automations", { automation_id: item.automation_id, name: item.name, description: item.description, schedule: item.schedule, action: item.action, policy: item.policy, enabled: !item.enabled });
      await refresh();
    } catch (error) { reportError(error instanceof Error ? error.message : "Rutin güncellenemedi"); }
    finally { setBusy(""); }
  }

  async function remove(item: AutomationDefinition) {
    if (!confirm(`“${item.name}” rutini silinsin mi?`)) return;
    try {
      await request("DELETE", `/v1/automations/${encodeURIComponent(item.automation_id)}`);
      if (historyFor === item.automation_id) { setHistoryFor(""); setHistory([]); }
      await refresh();
    } catch (error) { reportError(error instanceof Error ? error.message : "Rutin silinemedi"); }
  }

  return (
    <div className="settings-screen">
      <div className="settings-header"><div><h1>Rutinler</h1><p>Yerel scheduler görevleri yalnızca tanımladığın izin ve bütçeler içinde çalıştırır.</p></div><button className="secondary" onClick={() => void refresh()}>Yenile</button></div>
      <div className="preset-row">
        <button onClick={() => applyPreset("mail")}><strong>Sabah e-posta</strong><span>Gmail unread özeti</span></button>
        <button onClick={() => applyPreset("spotify")}><strong>Spotify snapshot</strong><span>Salt-okunur oynatma durumu</span></button>
        <button onClick={() => applyPreset("runtime")}><strong>Everything öz-sağlık</strong><span>9 alt sistemi yerelde denetler</span></button>
        <button onClick={() => applyPreset("health")}><strong>Repo sağlık kontrolü</strong><span>Kaynak kanıtlı derin plan</span></button>
        <button onClick={() => applyPreset("dependencies")}><strong>Haftalık güvenlik</strong><span>Bağımlılık ve supply-chain incelemesi</span></button>
      </div>
      <div className="automation-layout">
        <section className="automation-list">
          {automations.map((item) => (
            <article className={`automation-card ${historyFor === item.automation_id ? "selected" : ""}`} key={item.automation_id}>
              <div className="automation-main"><div className="automation-title-row"><h3>{item.name}</h3><span className={item.enabled ? "success" : "muted"}>{item.enabled ? "Aktif" : "Kapalı"}</span></div><p>{actionLabel(item.action)}</p><div className="skill-meta"><span>{scheduleLabel(item.schedule)}</span><span>Sonraki: {item.next_run_at_epoch_millis ? new Date(item.next_run_at_epoch_millis).toLocaleString() : "—"}</span><span>Hata: {item.consecutive_failures}</span>{Boolean(item.retry_attempt) && <span>Retry: {item.retry_attempt}/3</span>}</div>{item.suspended_reason && <div className="inline-warning">{item.suspended_reason}</div>}</div>
              <div className="automation-actions"><button className={`toggle ${item.enabled ? "on" : ""}`} onClick={() => void toggle(item)} aria-label={item.enabled ? "Rutini kapat" : "Rutini aç"}><span /></button><button className="secondary" disabled={busy !== ""} onClick={() => void runNow(item)}>Şimdi</button><button className="secondary" disabled={busy !== ""} onClick={() => void loadHistory(item)}>Geçmiş</button><button className="icon-button danger" onClick={() => void remove(item)}>×</button></div>
            </article>
          ))}
          {automations.length === 0 && <div className="panel-empty">Henüz rutin yok. Sağdaki basit formdan oluştur.</div>}
          {historyFor && <div className="automation-history"><div className="section-title-row"><div><h2>Çalışma geçmişi</h2><p>Son 50 çalıştırma, hata ve onay kaydı.</p></div><button className="text-button" onClick={() => { setHistoryFor(""); setHistory([]); }}>Kapat</button></div>{history.map((execution) => <article className="execution-row" key={execution.execution_id}><span className={`execution-status status-${execution.status.toLowerCase()}`}>{executionLabel(execution.status)}</span><div><strong>{new Date(execution.started_at_epoch_millis).toLocaleString()}</strong><p>{executionSummary(execution)}</p>{execution.next_retry_at_epoch_millis && <small>Sonraki deneme: {new Date(execution.next_retry_at_epoch_millis).toLocaleString()}</small>}{execution.status === "AwaitingApproval" && <button className="primary compact" disabled={busy !== ""} onClick={() => void approvePending(execution)}>Onayla ve çalıştır</button>}</div><details><summary>Kanıt</summary><pre>{JSON.stringify(execution, null, 2)}</pre></details></article>)}{history.length === 0 && <div className="panel-empty">Bu rutin henüz çalışmadı.</div>}</div>}
        </section>
        <section className="automation-editor">
          <h2>Yeni rutin</h2>
          <label>Ad<input value={name} onChange={(event) => setName(event.target.value)} /></label>
          <label>Zamanlama<select value={scheduleKind} onChange={(event) => setScheduleKind(event.target.value as ScheduleKind)}><option value="daily">Her gün</option><option value="weekly">Haftanın belirli günleri</option><option value="interval">Belirli aralıkla</option><option value="once">Tek sefer</option></select></label>
          {scheduleKind === "weekly" && <div className="weekday-picker">{WEEKDAYS.map((day) => <button type="button" key={day.value} className={weekdays.includes(day.value) ? "active" : ""} onClick={() => setWeekdays((current) => current.includes(day.value) ? current.filter((value) => value !== day.value) : [...current, day.value])}>{day.label}</button>)}</div>}
          {(scheduleKind === "daily" || scheduleKind === "weekly") && <><div className="time-row"><label>Saat<input type="number" min="0" max="23" value={hour} onChange={(event) => setHour(event.target.value)} /></label><label>Dakika<input type="number" min="0" max="59" value={minute} onChange={(event) => setMinute(event.target.value)} /></label></div><small className="field-hint">Yerel saat dilimi: {Intl.DateTimeFormat().resolvedOptions().timeZone || "sistem saati"}. Yaz/kış saati değişimleri otomatik izlenir.</small></>}
          {scheduleKind === "interval" && <label>Kaç dakikada bir<input type="number" min="1" max="10080" value={intervalMinutes} onChange={(event) => setIntervalMinutes(event.target.value)} /></label>}
          {scheduleKind === "once" && <label>Çalışma zamanı<input type="datetime-local" value={onceAt} onChange={(event) => setOnceAt(event.target.value)} /></label>}
          <label>Kaynak<select value={provider} onChange={(event) => { setProvider(event.target.value); const next = connectors.find((item) => item.provider === event.target.value)?.actions[0]; if (next) setActionId(next.action_id); }}><option value="Doctor">Everything öz-sağlık</option><option value="Plan">Repository planı</option><option value="Briefing">Akıllı yerel özet</option><option value="Skill">Everything skill</option>{connectors.filter((item) => item.connected).map((item) => <option key={item.provider}>{item.provider}</option>)}</select></label>
          {provider === "Skill" && <label>Skill<select value={skillId} onChange={(event) => setSkillId(event.target.value)}><option value="">Skill seç</option>{enabledSkills.map((item) => <option key={item.manifest.skill_id} value={item.manifest.skill_id}>{item.manifest.name} · {item.source}</option>)}</select></label>}
          {provider !== "Doctor" && provider !== "Plan" && provider !== "Briefing" && provider !== "Skill" && <label>Eylem<select value={actionId} onChange={(event) => setActionId(event.target.value)}>{selectedConnector?.actions.map((action) => <option key={action.action_id} value={action.action_id}>{action.title} · {action.risk}</option>)}</select></label>}
          <label>Çalışma biçimi<select value={autonomy} onChange={(event) => setAutonomy(event.target.value)}><option value="Observe">Observe · yalnız önizle</option><option value="Assist">Assist · risklide onay bekle</option><option value="ActWithApproval">ActWithApproval · her yazmada onay</option><option value="ActWithinPolicy">ActWithinPolicy · bu dar izin içinde otonom</option></select></label>
          {provider === "Doctor" && <div className="field-hint">Ek ayar gerekmez. Native doctor dokuz runtime alt sistemini model çağırmadan denetler.</div>}
          {provider === "Plan" && <>
            <label>İnceleme hedefi<textarea rows={5} value={String(parseInputObject(input).objective ?? "")} onChange={(event) => updateInputField("objective", event.target.value)} /></label>
            <label>Derinlik<select value={String(parseInputObject(input).mode ?? "Balanced")} onChange={(event) => updateInputField("mode", event.target.value)}><option>Fast</option><option>Balanced</option><option>Deep</option></select></label>
            <details className="advanced-json"><summary>Gelişmiş JSON</summary><label>Ham görev girdisi<textarea rows={8} value={input} onChange={(event) => setInput(event.target.value)} /></label></details>
          </>}
          {provider === "Briefing" && <>
            <label>Özet talimatı<textarea rows={5} value={String(parseInputObject(input).prompt ?? "")} onChange={(event) => updateInputField("prompt", event.target.value)} /></label>
            <label>Yerel model modu<select value={String(parseInputObject(input).mode ?? "Balanced")} onChange={(event) => updateInputField("mode", event.target.value)}><option>Fast</option><option>Balanced</option><option>Deep</option></select></label>
            <div className="field-hint">Kaynak sayısı: {Array.isArray(parseInputObject(input).sources) ? (parseInputObject(input).sources as unknown[]).length : 0}. Hazır e-posta şablonu güvenli salt-okunur Gmail kaynağını otomatik ekler.</div>
            <details className="advanced-json"><summary>Kaynakları ve gelişmiş JSON'u düzenle</summary><label>Ham briefing girdisi<textarea rows={10} value={input} onChange={(event) => setInput(event.target.value)} /></label></details>
          </>}
          {provider === "Skill" && <>
            <JsonSchemaForm schema={selectedSkill?.manifest.input_schema} value={input} onChange={setInput} />
            <details className="advanced-json"><summary>Gelişmiş JSON</summary><label>Ham skill girdisi<textarea rows={8} value={input} onChange={(event) => setInput(event.target.value)} /></label></details>
          </>}
          {provider !== "Doctor" && provider !== "Plan" && provider !== "Briefing" && provider !== "Skill" && <>
                {selectedMissingScopes.length > 0 && <div className="inline-warning">Bu eylem için eksik bağlantı izinleri: <code>{selectedMissingScopes.join(", ")}</code>. Önce Bağlantılar ekranından bu kapsamlarla yeniden bağlan.</div>}
                <JsonSchemaForm schema={selectedAction?.input_schema} value={input} onChange={setInput} />
                <details className="advanced-json"><summary>Gelişmiş JSON</summary><label>Ham rutin girdisi<textarea rows={8} value={input} onChange={(event) => setInput(event.target.value)} /></label></details>
              </>}
          <div className="inline-warning">Dış paylaşım rutinleri varsayılan olarak onay bekler. ActWithinPolicy yalnızca seçtiğin sağlayıcı ve eylem çiftine dar izin verir.</div>
          <button className="primary wide" disabled={!name.trim() || busy !== "" || selectedMissingScopes.length > 0} onClick={() => void createRoutine()}>Rutini oluştur</button>
        </section>
      </div>
    </div>
  );
}
