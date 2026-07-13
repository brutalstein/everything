import { lazy, Suspense, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type {
  ArtifactContentResponse,
  AutomationDefinition,
  ConnectorDescriptor,
  DiscoveredModel,
  EditProposalResponse,
  ExecutionMode,
  MemorySearchResult,
  ModelCapabilityProfile,
  PatchExecutionResponse,
  RunJournal,
  RunSummary,
  RuntimeDoctorReport,
  ServiceStatusSnapshot,
  SkillDescriptor,
  SkillExecutionResponse,
  VerificationReport,
} from "./types";

const ConnectionsScreen = lazy(() => import("./ConnectionsScreen").then((module) => ({ default: module.ConnectionsScreen })));
const AutomationsScreen = lazy(() => import("./AutomationsScreen").then((module) => ({ default: module.AutomationsScreen })));
const ResearchScreen = lazy(() => import("./ResearchScreen").then((module) => ({ default: module.ResearchScreen })));

type Screen = "chat" | "skills" | "memory" | "research" | "connections" | "automations" | "runtime";
type TaskKind = "ask" | "edit";

type Conversation = {
  run: RunJournal;
  content: string;
  verification: VerificationReport | null;
};

const suggestions = [
  "Bu repository'nin mimarisini kaynak kanıtlarıyla açıkla",
  "En riskli teknik borçları ve düzeltme sırasını çıkar",
  "Kurulum ve local Ollama hazırlığını kontrol et",
  "Bu projeyi refactor etmek için aşamalı bir plan hazırla",
];

function apiGet<T>(path: string, query?: Record<string, string | number | undefined>) {
  return window.everythingApp.request<T>({ method: "GET", path, query });
}

function apiPost<T>(path: string, body?: unknown) {
  return window.everythingApp.request<T>({ method: "POST", path, body });
}

function apiDelete<T>(path: string) {
  return window.everythingApp.request<T>({ method: "DELETE", path });
}

function basename(value: string) {
  const parts = value.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts.at(-1) ?? value;
}

function formatDate(value?: number | null) {
  if (!value) return "";
  const date = new Date(value);
  const today = new Date();
  if (date.toDateString() === today.toDateString()) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

function displayObjective(value: string) {
  const match = value.match(/<user_objective>\s*([\s\S]*?)\s*<\/user_objective>/i);
  if (match?.[1]) return match[1].trim();
  return value
    .replace(/^Investigate this repository using graph and source evidence:\s*/i, "")
    .replace(/^Produce a source-grounded architecture summary:\s*/i, "")
    .replace(/^Assess installer and runtime readiness, including concrete blockers:\s*/i, "")
    .replace(/^Produce a deep, non-mutating refactor plan:\s*/i, "")
    .trim();
}

function statusTone(status?: string) {
  const normalized = status?.toLowerCase() ?? "";
  if (normalized.includes("complete") || normalized === "healthy" || normalized === "ready") {
    return "success";
  }
  if (normalized.includes("fail") || normalized === "error" || normalized === "unavailable") {
    return "danger";
  }
  if (normalized === "degraded" || normalized.includes("awaiting") || normalized.includes("warning")) {
    return "warning";
  }
  if (normalized.includes("running") || normalized.includes("start")) return "active";
  return "muted";
}

function extractSkillContent(response: SkillExecutionResponse): string {
  const output = response.output as Record<string, unknown> | null;
  const document = output?.document as Record<string, unknown> | undefined;
  if (typeof document?.content === "string") return document.content;
  if (typeof output?.content === "string") return output.content;
  return JSON.stringify(response.output, null, 2);
}

function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
  };
  const paths: Record<string, ReactNode> = {
    plus: <><path d="M12 5v14"/><path d="M5 12h14"/></>,
    send: <><path d="m22 2-7 20-4-9-9-4Z"/><path d="M22 2 11 13"/></>,
    skill: <><path d="M12 2 9.5 8.5 3 11l6.5 2.5L12 20l2.5-6.5L21 11l-6.5-2.5Z"/></>,
    memory: <><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 7h8M8 11h8M8 15h5"/></>,
    settings: <><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></>,
    refresh: <><path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/></>,
    trash: <><path d="M4 7h16M9 7V4h6v3M7 7l1 13h8l1-13"/></>,
    chevron: <path d="m9 18 6-6-6-6"/>,
    check: <path d="m5 12 4 4L19 6"/>,
    close: <path d="m6 6 12 12M18 6 6 18"/>,
    search: <><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></>,
    folder: <><path d="M3 6h6l2 2h10v10H3Z"/></>,
    link: <><path d="M10 13a5 5 0 0 0 7.1.1l2-2a5 5 0 0 0-7.1-7.1l-1.1 1.1"/><path d="M14 11a5 5 0 0 0-7.1-.1l-2 2A5 5 0 0 0 12 20l1.1-1.1"/></>,
    clock: <><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></>,
  };
  return <svg {...common}>{paths[name]}</svg>;
}

function EmptyState({ onChoose }: { onChoose: (value: string) => void }) {
  return (
    <div className="empty-state">
      <div className="mark large">E</div>
      <h1>Ne üzerinde çalışalım?</h1>
      <p>Everything repository’yi indeksler, yerel modeline doğru bağlamı verir ve sonucu kanıtlarıyla saklar.</p>
      <div className="suggestion-grid">
        {suggestions.map((suggestion) => (
          <button key={suggestion} onClick={() => onChoose(suggestion)}>{suggestion}</button>
        ))}
      </div>
    </div>
  );
}

export function App() {
  const [screen, setScreen] = useState<Screen>("chat");
  const [service, setService] = useState<ServiceStatusSnapshot | null>(null);
  const [doctor, setDoctor] = useState<RuntimeDoctorReport | null>(null);
  const [models, setModels] = useState<DiscoveredModel[]>([]);
  const [capabilities, setCapabilities] = useState<ModelCapabilityProfile | null>(null);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [skills, setSkills] = useState<SkillDescriptor[]>([]);
  const [memories, setMemories] = useState<MemorySearchResult[]>([]);
  const [connectors, setConnectors] = useState<ConnectorDescriptor[]>([]);
  const [automations, setAutomations] = useState<AutomationDefinition[]>([]);
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [prompt, setPrompt] = useState("");
  const [mode, setMode] = useState<ExecutionMode>("Balanced");
  const [taskKind, setTaskKind] = useState<TaskKind>("ask");
  const [skillId, setSkillId] = useState("repository-investigation");
  const [editSkillId, setEditSkillId] = useState("scoped-edit");
  const [pendingEdit, setPendingEdit] = useState<EditProposalResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadingRun, setLoadingRun] = useState(false);
  const [error, setError] = useState("");
  const [memoryQuery, setMemoryQuery] = useState("");
  const [newMemoryTitle, setNewMemoryTitle] = useState("");
  const [newMemoryContent, setNewMemoryContent] = useState("");
  const [newMemoryScope, setNewMemoryScope] = useState("Workspace");
  const [logs, setLogs] = useState<string[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const safeSkills = useMemo(
    () => skills.filter((skill) =>
      skill.enabled &&
      skill.compatibility.compatible &&
      !skill.manifest.permissions.includes("WorkspaceWrite") &&
      !skill.manifest.permissions.includes("workspace.write")
    ),
    [skills],
  );

  const editSkills = useMemo(
    () => skills.filter((skill) =>
      skill.enabled &&
      skill.compatibility.compatible &&
      (skill.manifest.permissions.includes("WorkspaceWrite") ||
        skill.manifest.permissions.includes("workspace.write") ||
        skill.manifest.workflow === "Prompt")
    ),
    [skills],
  );

  const selectedSkill = skills.find((skill) => skill.manifest.skill_id === skillId) ?? null;
  const selectedEditSkill = skills.find((skill) => skill.manifest.skill_id === editSkillId) ?? null;
  const selectedModel = models.find((model) => model.configured) ?? models[0] ?? null;

  async function refreshAll(selectNewest = false) {
    setError("");
    try {
      const snapshot = await window.everythingApp.ensureService();
      setService(snapshot);
      setLogs(snapshot.recentLogs);
      const requests = await Promise.allSettled([
        apiGet<RuntimeDoctorReport>("/v1/doctor"),
        apiGet<DiscoveredModel[]>("/v1/models"),
        apiGet<ModelCapabilityProfile>("/v1/models/capabilities"),
        apiGet<RunSummary[]>("/v1/runs"),
        apiGet<SkillDescriptor[]>("/v1/skills"),
        apiGet<MemorySearchResult[]>("/v1/memory", { limit: 50 }),
        apiGet<ConnectorDescriptor[]>("/v1/connectors"),
        apiGet<AutomationDefinition[]>("/v1/automations"),
      ]);

      const [doctorResult, modelsResult, profileResult, runsResult, skillsResult, memoryResult, connectorResult, automationResult] = requests;
      if (doctorResult.status === "fulfilled") setDoctor(doctorResult.value);
      if (modelsResult.status === "fulfilled") setModels(modelsResult.value);
      if (profileResult.status === "fulfilled") setCapabilities(profileResult.value);
      if (runsResult.status === "fulfilled") setRuns(runsResult.value);
      if (skillsResult.status === "fulfilled") setSkills(skillsResult.value);
      if (memoryResult.status === "fulfilled") setMemories(memoryResult.value);
      if (connectorResult.status === "fulfilled") setConnectors(connectorResult.value);
      if (automationResult.status === "fulfilled") setAutomations(automationResult.value);

      const failures = requests
        .filter((result): result is PromiseRejectedResult => result.status === "rejected")
        .map((result) => result.reason instanceof Error ? result.reason.message : String(result.reason));
      if (failures.length > 0) {
        setError(`Bazı runtime verileri yüklenemedi: ${[...new Set(failures)].join(" · ")}`);
      }

      if (selectNewest && runsResult.status === "fulfilled" && runsResult.value[0]) {
        await openRun(runsResult.value[0].run_id);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Runtime verileri yüklenemedi");
    }
  }

  useEffect(() => {
    void refreshAll();
    const disposeStatus = window.everythingApp.onServiceStatus(setService);
    const disposeLog = window.everythingApp.onServiceLog((line) => {
      setLogs((current) => [line, ...current].slice(0, 120));
    });
    const disposeEvent = window.everythingApp.onRuntimeEvent((event) => {
      if (
        event.event_kind === "run.event"
        || event.event_kind.startsWith("skill.")
        || event.event_kind.startsWith("edit.")
        || event.event_kind.startsWith("execution.")
      ) {
        void apiGet<RunSummary[]>("/v1/runs").then(setRuns).catch(() => undefined);
      }
      if (event.event_kind.startsWith("automation.")) {
        void apiGet<AutomationDefinition[]>("/v1/automations").then(setAutomations).catch(() => undefined);
      }
    });
    return () => {
      disposeStatus();
      disposeLog();
      disposeEvent();
    };
  }, []);

  async function openRun(runId: string) {
    setLoadingRun(true);
    setError("");
    setPendingEdit(null);
    try {
      const run = await apiGet<RunJournal>(`/v1/runs/${encodeURIComponent(runId)}`);
      const planArtifact = [...run.artifacts].reverse().find((artifact) => artifact.kind === "Plan");
      const diffArtifact = [...run.artifacts].reverse().find((artifact) => artifact.kind === "Diff");
      const patchArtifact = [...run.artifacts].reverse().find((artifact) => artifact.kind === "Patch");
      const verificationArtifact = [...run.artifacts]
        .reverse()
        .find((artifact) => artifact.kind === "VerificationReport");
      let content = run.events.at(-1)?.detail ?? "Bu çalışma için görüntülenebilir çıktı bulunamadı.";
      let verification: VerificationReport | null = null;
      if (planArtifact || diffArtifact || patchArtifact) {
        const primaryArtifact = planArtifact ?? diffArtifact ?? patchArtifact;
        const artifact = await apiGet<ArtifactContentResponse>(
          `/v1/artifacts/${encodeURIComponent(primaryArtifact!.artifact_id)}`,
        );
        if (primaryArtifact?.kind === "Patch") {
          try {
            const proposal = JSON.parse(artifact.content) as Omit<
              EditProposalResponse,
              "run_id" | "status" | "artifact"
            >;
            content = `${proposal.summary ?? "Kod değişikliği önerisi"}\n\n${proposal.diff ?? artifact.content}`;
            if (run.status === "AwaitingApproval" && proposal.patch) {
              setPendingEdit({
                ...proposal,
                run_id: run.run_id,
                status: "awaiting_approval",
                artifact: primaryArtifact,
              });
            } else {
              setPendingEdit(null);
            }
          } catch {
            content = artifact.content;
            setPendingEdit(null);
          }
        } else {
          setPendingEdit(null);
          content = artifact.content;
        }
      }
      if (verificationArtifact) {
        const artifact = await apiGet<ArtifactContentResponse>(
          `/v1/artifacts/${encodeURIComponent(verificationArtifact.artifact_id)}`,
        );
        try {
          verification = JSON.parse(artifact.content) as VerificationReport;
        } catch {
          verification = null;
        }
      }
      setConversation({ run, content, verification });
      setScreen("chat");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Çalışma açılamadı");
    } finally {
      setLoadingRun(false);
    }
  }

  function newTask(initial = "") {
    setConversation(null);
    setPendingEdit(null);
    setPrompt(initial);
    setScreen("chat");
    requestAnimationFrame(() => textareaRef.current?.focus());
  }

  async function submitTask() {
    const objective = prompt.trim();
    if (!objective || busy) return;
    setBusy(true);
    setError("");
    try {
      if (taskKind === "edit") {
        const response = await apiPost<EditProposalResponse>("/v1/edits/propose", {
          objective,
          mode,
          skill_id: editSkillId === "auto" ? null : editSkillId,
        });
        const run = await apiGet<RunJournal>(`/v1/runs/${encodeURIComponent(response.run_id)}`);
        setPrompt("");
        setPendingEdit(response);
        setConversation({
          run,
          content: `${response.summary}\n\n${response.diff}`,
          verification: null,
        });
        await refreshAll();
      } else if (skillId === "auto") {
        const response = await apiPost<{
          run_id: string;
          document: { content: string };
        }>("/v1/plan", { objective, mode });
        setPrompt("");
        await refreshAll();
        await openRun(response.run_id);
      } else {
        const response = await apiPost<SkillExecutionResponse>(
          `/v1/skills/${encodeURIComponent(skillId)}/execute`,
          { input: { objective, mode }, approval_granted: false },
        );
        if (!response.run_id) throw new Error("Skill bir run üretmedi");
        setPrompt("");
        await refreshAll();
        const run = await apiGet<RunJournal>(`/v1/runs/${encodeURIComponent(response.run_id)}`);
        setConversation({
          run,
          content: extractSkillContent(response),
          verification: response.verification_report ?? null,
        });
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Görev çalıştırılamadı");
    } finally {
      setBusy(false);
    }
  }

  async function approvePendingEdit() {
    if (!pendingEdit || busy) return;
    setBusy(true);
    setError("");
    try {
      const response = await apiPost<PatchExecutionResponse>("/v1/executions/patch", {
        ...pendingEdit.patch,
        approval_granted: true,
      });
      setPendingEdit(null);
      await refreshAll();
      await openRun(response.run_id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Değişiklik uygulanamadı");
    } finally {
      setBusy(false);
    }
  }

  async function selectWorkspace() {
    setError("");
    try {
      const snapshot = await window.everythingApp.selectWorkspaceDirectory();
      if (!snapshot) return;
      setService(snapshot);
      setConversation(null);
      setPendingEdit(null);
      setPrompt("");
      await refreshAll();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Workspace değiştirilemedi");
    }
  }

  async function toggleSkill(skill: SkillDescriptor) {
    setError("");
    try {
      const action = skill.enabled ? "disable" : "enable";
      await apiPost(`/v1/skills/${encodeURIComponent(skill.manifest.skill_id)}/${action}`);
      setSkills(await apiGet<SkillDescriptor[]>("/v1/skills"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Skill durumu değiştirilemedi");
    }
  }

  async function installSkill() {
    const sourcePath = await window.everythingApp.selectSkillDirectory();
    if (!sourcePath) return;
    setError("");
    try {
      await apiPost("/v1/skills/install", { source_path: sourcePath });
      setSkills(await apiPost<SkillDescriptor[]>("/v1/skills/refresh"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Skill kurulamadı");
    }
  }

  async function uninstallSkill(skill: SkillDescriptor) {
    setError("");
    try {
      await apiDelete(`/v1/skills/${encodeURIComponent(skill.manifest.skill_id)}`);
      const updated = await apiPost<SkillDescriptor[]>("/v1/skills/refresh");
      setSkills(updated);
      if (skillId === skill.manifest.skill_id) setSkillId("repository-investigation");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Skill kaldırılamadı");
    }
  }

  async function searchMemory() {
    try {
      setMemories(await apiGet<MemorySearchResult[]>("/v1/memory", {
        query: memoryQuery,
        limit: 100,
      }));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Memory aranamadı");
    }
  }

  async function createMemory() {
    if (!newMemoryTitle.trim() || !newMemoryContent.trim()) return;
    try {
      await apiPost("/v1/memory", {
        scope: newMemoryScope,
        title: newMemoryTitle.trim(),
        content: newMemoryContent.trim(),
        source: "operator",
        workspace_key: service?.workspaceRoot,
        confidence: 0.95,
        tags: ["operator"],
        editable: true,
        forgettable: true,
      });
      setNewMemoryTitle("");
      setNewMemoryContent("");
      await searchMemory();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Memory kaydedilemedi");
    }
  }

  async function forgetMemory(memoryId: string) {
    try {
      await apiDelete(`/v1/memory/${encodeURIComponent(memoryId)}`);
      await searchMemory();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Memory silinemedi");
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-row">
          <div className="mark">E</div>
          <span>Everything</span>
        </div>

        <button className="new-task" onClick={() => newTask()}>
          <Icon name="plus" /> Yeni görev
        </button>

        <button
          className="workspace-label"
          title="Proje klasörünü değiştir"
          onClick={() => void selectWorkspace()}
        >
          <Icon name="folder" size={15} />
          <span title={service?.workspaceRoot}>{basename(service?.workspaceRoot ?? "workspace")}</span>
          <Icon name="chevron" size={13} />
        </button>

        <div className="history-heading">Geçmiş</div>
        <div className="run-list">
          {runs.map((run) => (
            <button
              key={run.run_id}
              className={`run-row ${conversation?.run.run_id === run.run_id ? "selected" : ""}`}
              onClick={() => void openRun(run.run_id)}
            >
              <span className="run-title">{displayObjective(run.objective)}</span>
              <span className="run-meta">
                <span className={`status-dot ${statusTone(run.status)}`} />
                {formatDate(run.updated_at_epoch_millis ?? 0)}
              </span>
            </button>
          ))}
          {runs.length === 0 && <div className="sidebar-empty">Henüz görev yok</div>}
        </div>

        <div className="sidebar-bottom">
          <button className={screen === "skills" ? "active" : ""} onClick={() => setScreen("skills")}>
            <Icon name="skill" /> Skills
          </button>
          <button className={screen === "memory" ? "active" : ""} onClick={() => setScreen("memory")}>
            <Icon name="memory" /> Memory
          </button>
          <button className={screen === "research" ? "active" : ""} onClick={() => setScreen("research")}>
            <Icon name="search" /> Web Research
          </button>
          <button className={screen === "connections" ? "active" : ""} onClick={() => setScreen("connections")}>
            <Icon name="link" /> Bağlantılar
          </button>
          <button className={screen === "automations" ? "active" : ""} onClick={() => setScreen("automations")}>
            <Icon name="clock" /> Rutinler
          </button>
          <button className={screen === "runtime" ? "active" : ""} onClick={() => setScreen("runtime")}>
            <Icon name="settings" /> Runtime
          </button>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <div className="topbar-title">
            {screen === "chat" && (conversation ? displayObjective(conversation.run.objective) : "Yeni görev")}
            {screen === "skills" && "Skills"}
            {screen === "memory" && "Memory"}
            {screen === "research" && "Web Research"}
            {screen === "connections" && "Bağlantılar"}
            {screen === "automations" && "Rutinler"}
            {screen === "runtime" && "Runtime"}
          </div>
          <div className="runtime-pill" title={service?.detail}>
            <span className={`status-dot ${statusTone(service?.status)}`} />
            <span>{selectedModel?.model_name ?? capabilities?.model_name ?? "model"}</span>
          </div>
        </header>

        {error && (
          <div className="error-banner">
            <span>{error}</span>
            <button onClick={() => setError("")}><Icon name="close" size={16} /></button>
          </div>
        )}

        {screen === "chat" && (
          <div className="chat-screen">
            <div className="conversation-scroll">
              {!conversation && !loadingRun && <EmptyState onChoose={newTask} />}
              {loadingRun && <div className="loading-state"><span className="spinner" /> Çalışma açılıyor…</div>}
              {conversation && (
                <div className="conversation">
                  <div className="message user-message">
                    <div className="message-label">Sen</div>
                    <div>{displayObjective(conversation.run.objective)}</div>
                  </div>
                  <div className="message assistant-message">
                    <div className="assistant-head">
                      <div className="mark small">E</div>
                      <div>
                        <strong>Everything</strong>
                        <span>{conversation.run.generated_by}</span>
                      </div>
                      <span className={`run-status ${statusTone(conversation.run.status)}`}>
                        {conversation.run.status}
                      </span>
                    </div>
                    <pre className="assistant-output">{conversation.content}</pre>
                  </div>
                  {pendingEdit && conversation.run.run_id === pendingEdit.run_id && (
                    <section className="approval-card">
                      <div>
                        <h3>Değişikliği uygula?</h3>
                        <p>
                          <code>{pendingEdit.patch.relative_path}</code> dosyası hash kontrolüyle
                          değiştirilecek. Zorunlu doğrulama başarısız olursa değişiklik otomatik geri alınır.
                        </p>
                        {pendingEdit.impact_analysis && (
                          <div className={`impact-summary impact-${pendingEdit.impact_analysis.risk_tier}`}>
                            <strong>Etki: {pendingEdit.impact_analysis.risk_tier} · {pendingEdit.impact_analysis.aggregate_risk_score.toFixed(1)}</strong>
                            <span>{pendingEdit.impact_analysis.affected_files.length} dosya · {pendingEdit.impact_analysis.affected_entities.length} sembol · {pendingEdit.impact_analysis.verification_targets.length} doğrulama hedefi</span>
                            <details><summary>Blast radius</summary><div className="impact-files">{pendingEdit.impact_analysis.affected_files.slice(0, 24).map((file) => <code key={file}>{file}</code>)}</div></details>
                          </div>
                        )}
                        <div className="approval-meta">
                          <span>{pendingEdit.patch.verification_commands.length} doğrulama komutu</span>
                          <span>{pendingEdit.generated_by}</span>
                          {pendingEdit.skill_id && <span>{pendingEdit.skill_id}</span>}
                          {pendingEdit.fallback_used && <span className="danger">fallback</span>}
                        </div>
                      </div>
                      <div className="approval-actions">
                        <button className="secondary" onClick={() => setPendingEdit(null)} disabled={busy}>
                          Şimdi değil
                        </button>
                        <button className="primary" onClick={() => void approvePendingEdit()} disabled={busy}>
                          {busy ? <span className="spinner" /> : <Icon name="check" size={16} />} Uygula ve doğrula
                        </button>
                      </div>
                    </section>
                  )}
                  {conversation.verification && (
                    <section className="verification-card">
                      <div className="section-title-row">
                        <h3>Doğrulama</h3>
                        <span className={`run-status ${statusTone(conversation.verification.status)}`}>
                          {conversation.verification.status}
                        </span>
                      </div>
                      <div className="verification-score">
                        Güven: %{Math.round(conversation.verification.confidence * 100)}
                      </div>
                      {conversation.verification.checks.map((check) => (
                        <div className="check-row" key={check.check_id}>
                          <span className={statusTone(check.status)}>
                            <Icon name={check.status === "Passed" ? "check" : "close"} size={15} />
                          </span>
                          <div><strong>{check.kind}</strong><span>{check.summary}</span></div>
                        </div>
                      ))}
                      {conversation.verification.unresolved_risks.length > 0 && (
                        <div className="risk-box">
                          {conversation.verification.unresolved_risks.map((risk) => <div key={risk}>{risk}</div>)}
                        </div>
                      )}
                    </section>
                  )}
                  <details className="activity-details">
                    <summary>Çalışma ayrıntıları · {conversation.run.events.length} olay</summary>
                    <div className="event-list">
                      {conversation.run.events.map((event) => (
                        <div key={event.event_id || event.sequence}>
                          <span>{event.stage}</span><p>{event.summary || event.detail}</p>
                        </div>
                      ))}
                    </div>
                  </details>
                </div>
              )}
            </div>

            <div className="composer-wrap">
              <div className={`composer ${busy ? "busy" : ""}`}>
                <textarea
                  ref={textareaRef}
                  value={prompt}
                  placeholder="Everything'e bir görev ver…"
                  rows={3}
                  onChange={(event) => setPrompt(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !event.shiftKey) {
                      event.preventDefault();
                      void submitTask();
                    }
                  }}
                />
                <div className="composer-tools">
                  <div className="tool-selects">
                    <select value={taskKind} onChange={(event) => setTaskKind(event.target.value as TaskKind)}>
                      <option value="ask">İncele</option>
                      <option value="edit">Kodla</option>
                    </select>
                    {taskKind === "ask" ? (
                      <select value={skillId} onChange={(event) => setSkillId(event.target.value)}>
                        <option value="auto">Auto</option>
                        {safeSkills.map((skill) => (
                          <option key={skill.manifest.skill_id} value={skill.manifest.skill_id}>
                            {skill.manifest.name}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <select value={editSkillId} onChange={(event) => setEditSkillId(event.target.value)}>
                        <option value="auto">Auto</option>
                        {editSkills.map((skill) => (
                          <option key={skill.manifest.skill_id} value={skill.manifest.skill_id}>
                            {skill.manifest.name}
                          </option>
                        ))}
                      </select>
                    )}
                    <select value={mode} onChange={(event) => setMode(event.target.value as ExecutionMode)}>
                      <option value="Fast">Fast</option>
                      <option value="Balanced">Balanced</option>
                      <option value="Deep">Deep</option>
                    </select>
                  </div>
                  <button className="send-button" disabled={!prompt.trim() || busy} onClick={() => void submitTask()}>
                    {busy ? <span className="spinner" /> : <Icon name="send" size={17} />}
                  </button>
                </div>
              </div>
              <div className="composer-hint">
                {taskKind === "edit"
                  ? `${selectedEditSkill ? selectedEditSkill.manifest.description : "Graph + memory tabanlı dar edit"} · Önce diff, sonra açık onay`
                  : `${selectedSkill ? selectedSkill.manifest.description : "Graph-first plan"} · Enter gönderir`}
              </div>
            </div>
          </div>
        )}

        {screen === "skills" && (
          <div className="settings-screen">
            <div className="settings-header">
              <div><h1>Skills</h1><p>Yerel workflow paketlerini kur, aç veya kapat.</p></div>
              <div className="header-actions">
                <button className="secondary" onClick={() => void apiPost<SkillDescriptor[]>("/v1/skills/refresh").then(setSkills)}>
                  <Icon name="refresh" /> Yenile
                </button>
                <button className="primary" onClick={() => void installSkill()}>
                  <Icon name="plus" /> Skill kur
                </button>
              </div>
            </div>
            <div className="skill-list">
              {skills.map((skill) => (
                <article className="skill-card" key={skill.manifest.skill_id}>
                  <div className="skill-icon"><Icon name="skill" /></div>
                  <div className="skill-copy">
                    <div className="skill-title-row">
                      <h3>{skill.manifest.name}</h3>
                      <span>{skill.source}</span>
                      {!skill.compatibility.compatible && <span className="danger">Uyumsuz</span>}
                    </div>
                    <p>{skill.manifest.description}</p>
                    <div className="skill-meta">
                      <span>{skill.manifest.skill_id}@{skill.manifest.version}</span>
                      <span>{skill.manifest.workflow}</span>
                      <span>{skill.manifest.permissions.join(" · ")}</span>
                    </div>
                    {skill.instructions_preview && <div className="skill-preview">{skill.instructions_preview}</div>}
                  </div>
                  <div className="skill-actions">
                    <button
                      className={`toggle ${skill.enabled ? "on" : ""}`}
                      onClick={() => void toggleSkill(skill)}
                      aria-label={`${skill.manifest.name} ${skill.enabled ? "disable" : "enable"}`}
                    ><span /></button>
                    {!skill.manifest.permissions.includes("WorkspaceWrite") && !skill.manifest.permissions.includes("workspace.write") && (
                      <button className="icon-button" title="Bu skill ile görev oluştur" onClick={() => { setSkillId(skill.manifest.skill_id); newTask(); }}>
                        <Icon name="chevron" />
                      </button>
                    )}
                    {skill.source === "Workspace" && (
                      <button className="icon-button danger" title="Kaldır" onClick={() => void uninstallSkill(skill)}>
                        <Icon name="trash" />
                      </button>
                    )}
                  </div>
                </article>
              ))}
            </div>
            <div className="format-note">
              <strong>Skill formatı</strong>
              <p>Bir klasör içinde <code>SKILL.md</code> kullan. İsteğe bağlı <code>skill.toml</code> ile id, sürüm, izin ve workflow tanımlayabilirsin.</p>
            </div>
          </div>
        )}

        {screen === "memory" && (
          <div className="settings-screen memory-screen">
            <div className="settings-header">
              <div><h1>Memory</h1><p>Everything’in sonraki görevlerde kullanacağı yapılandırılmış bilgiler.</p></div>
            </div>
            <div className="memory-layout">
              <section className="memory-list-panel">
                <div className="search-row">
                  <Icon name="search" />
                  <input value={memoryQuery} onChange={(event) => setMemoryQuery(event.target.value)} onKeyDown={(event) => event.key === "Enter" && void searchMemory()} placeholder="Memory ara" />
                  <button onClick={() => void searchMemory()}>Ara</button>
                </div>
                <div className="memory-list">
                  {memories.map(({ entry, score }) => (
                    <article className="memory-card" key={entry.memory_id}>
                      <div className="memory-title-row">
                        <div><h3>{entry.title}</h3><span>{entry.scope} · %{Math.round(entry.confidence * 100)} güven</span></div>
                        {entry.forgettable && <button className="icon-button danger" onClick={() => void forgetMemory(entry.memory_id)}><Icon name="trash" /></button>}
                      </div>
                      <p>{entry.content}</p>
                      <div className="skill-meta"><span>{entry.source}</span><span>score {score.toFixed(2)}</span>{entry.tags.map((tag) => <span key={tag}>#{tag}</span>)}</div>
                    </article>
                  ))}
                  {memories.length === 0 && <div className="panel-empty">Memory bulunamadı.</div>}
                </div>
              </section>
              <section className="memory-editor">
                <h2>Yeni memory</h2>
                <label>Başlık<input value={newMemoryTitle} onChange={(event) => setNewMemoryTitle(event.target.value)} placeholder="Örn. Test tercihi" /></label>
                <label>Kapsam<select value={newMemoryScope} onChange={(event) => setNewMemoryScope(event.target.value)}><option>Workspace</option><option>Preference</option><option>Session</option><option>Task</option><option>Graph</option></select></label>
                <label>İçerik<textarea value={newMemoryContent} onChange={(event) => setNewMemoryContent(event.target.value)} rows={8} placeholder="Hatırlanmasını istediğin açık ve kalıcı bilgi" /></label>
                <button className="primary wide" onClick={() => void createMemory()} disabled={!newMemoryTitle.trim() || !newMemoryContent.trim()}>Kaydet</button>
              </section>
            </div>
          </div>
        )}

        {screen === "connections" && (
          <Suspense fallback={<div className="panel-empty">Bağlantılar yükleniyor…</div>}>
            <ConnectionsScreen
              connectors={connectors}
              refresh={async () => {
                setConnectors(await apiGet<ConnectorDescriptor[]>("/v1/connectors"));
              }}
              reportError={setError}
            />
          </Suspense>
        )}

        {screen === "automations" && (
          <Suspense fallback={<div className="panel-empty">Rutinler yükleniyor…</div>}>
            <AutomationsScreen
              automations={automations}
              connectors={connectors}
              skills={skills}
              refresh={async () => {
                const [nextAutomations, nextConnectors, nextSkills] = await Promise.all([
                  apiGet<AutomationDefinition[]>("/v1/automations"),
                  apiGet<ConnectorDescriptor[]>("/v1/connectors"),
                  apiGet<SkillDescriptor[]>("/v1/skills"),
                ]);
                setAutomations(nextAutomations);
                setConnectors(nextConnectors);
                setSkills(nextSkills);
              }}
              reportError={setError}
            />
          </Suspense>
        )}

        {screen === "runtime" && (
          <div className="settings-screen">
            <div className="settings-header">
              <div><h1>Runtime</h1><p>Yerel daemon, model ve repository durumu.</p></div>
              <button className="secondary" onClick={() => void window.everythingApp.restartService().then(() => refreshAll())}><Icon name="refresh" /> Yeniden başlat</button>
            </div>
            <div className="runtime-grid">
              <section className="runtime-card">
                <h3>Servis</h3>
                <div className="metric"><span>Durum</span><strong className={statusTone(service?.status)}>{service?.status ?? "unknown"}</strong></div>
                <div className="metric"><span>PID</span><strong>{service?.pid ?? "—"}</strong></div>
                <div className="metric"><span>Adres</span><strong>{service?.serviceUrl ?? "—"}</strong></div>
                <div className="metric"><span>Workspace</span><strong title={service?.workspaceRoot}>{service?.workspaceRoot ?? "—"}</strong></div>
              </section>
              <section className="runtime-card">
                <h3>Model</h3>
                <div className="metric"><span>Model</span><strong>{selectedModel?.model_name ?? capabilities?.model_name ?? "—"}</strong></div>
                <div className="metric"><span>Provider</span><strong>{selectedModel?.provider ?? capabilities?.provider ?? "—"}</strong></div>
                <div className="metric"><span>Model health</span><strong className={statusTone(doctor?.model_health.status)}>{doctor?.model_health.status ?? "—"}</strong></div>
                <div className="metric"><span>Genel durum</span><strong className={statusTone(doctor?.overall_status)}>{doctor?.overall_status ?? "—"}</strong></div>
                <div className="metric"><span>Safe context</span><strong>{capabilities?.safe_context_tokens?.toLocaleString() ?? "—"}</strong></div>
              </section>
              <section className="runtime-card">
                <h3>Repository</h3>
                <div className="metric"><span>Dosya</span><strong>{doctor?.project.file_count.toLocaleString() ?? "—"}</strong></div>
                <div className="metric"><span>Graph node</span><strong>{doctor?.project.graph_node_count.toLocaleString() ?? "—"}</strong></div>
                <div className="metric"><span>Graph edge</span><strong>{doctor?.project.graph_edge_count.toLocaleString() ?? "—"}</strong></div>
                <div className="metric"><span>Cache hit</span><strong>{doctor?.snapshot_stats.cache_hits.toLocaleString() ?? "—"}</strong></div>
              </section>
            </div>
            <section className="logs-card">
              <div className="section-title-row"><h3>Sağlık kontrolleri</h3><span>{doctor?.checks.length ?? 0} kontrol</span></div>
              <div className="doctor-checks">
                {doctor?.checks.map((check) => (
                  <article className="doctor-check" key={check.check_id}>
                    <span className={`doctor-dot ${statusTone(check.status)}`} />
                    <div><strong>{check.label}</strong><p>{check.detail}</p>{check.remediation && <small>{check.remediation}</small>}</div>
                  </article>
                ))}
                {!doctor?.checks.length && <div className="panel-empty">Sağlık raporu henüz yüklenmedi.</div>}
              </div>
            </section>
            <section className="logs-card">
              <div className="section-title-row"><h3>Son loglar</h3><button className="text-button" onClick={() => setLogs([])}>Temizle</button></div>
              <pre>{logs.length ? logs.join("\n") : "Log yok."}</pre>
            </section>
          </div>
        )}
      </main>
    </div>
  );
}
