/// <reference types="vite/client" />

import type {
  CodeGraphImpactReport,
  CodeGraphIndexReport,
  CodeGraphPath,
  CodeGraphSearchResult,
  GraphImpactReport,
  GraphQueryResult,
  GraphSummaryResponse,
  ModuleDescriptor,
  PersistentGraphStats,
  PlanResponse,
  RunJournal,
  RunSummary,
  RuntimeDoctorReport,
  RuntimeMetricsSnapshot,
  ServiceEvent,
  ServiceInfo,
  ServiceStatusSnapshot,
  BenchmarkRecord,
} from "./types";

interface BackendRequest {
  method: "GET" | "POST" | "DELETE";
  path: string;
  query?: Record<string, string | number | undefined>;
  body?: unknown;
}

interface EverythingBridge {
  getContext(): Promise<ServiceStatusSnapshot>;
  ensureService(): Promise<ServiceStatusSnapshot>;
  restartService(): Promise<ServiceStatusSnapshot>;
  stopService(): Promise<ServiceStatusSnapshot>;
  request<T = unknown>(input: BackendRequest): Promise<T>;
  selectWorkspaceDirectory(): Promise<ServiceStatusSnapshot | null>;
  selectSkillDirectory(): Promise<string | null>;
  openExternal(url: string): Promise<boolean>;
  onServiceStatus(callback: (payload: ServiceStatusSnapshot) => void): () => void;
  onServiceLog(callback: (payload: string) => void): () => void;
  onRuntimeEvent(callback: (payload: ServiceEvent) => void): () => void;
}

declare global {
  interface Window {
    everythingApp: EverythingBridge;
  }
}

export type {
  BenchmarkRecord,
  CodeGraphImpactReport,
  CodeGraphIndexReport,
  CodeGraphPath,
  CodeGraphSearchResult,
  GraphImpactReport,
  GraphQueryResult,
  GraphSummaryResponse,
  ModuleDescriptor,
  PersistentGraphStats,
  PlanResponse,
  RunJournal,
  RunSummary,
  RuntimeDoctorReport,
  RuntimeMetricsSnapshot,
  ServiceInfo,
};
