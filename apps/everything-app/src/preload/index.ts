import { contextBridge, ipcRenderer } from "electron";
import type {
  ServiceEventPayload,
  ServiceStatusSnapshot,
} from "../main/backend";

type BackendRequest = {
  method: "GET" | "POST" | "DELETE";
  path: string;
  query?: Record<string, string | number | undefined>;
  body?: unknown;
};

function registerListener<T>(
  channel: "service:status" | "service:log" | "runtime:event",
  callback: (payload: T) => void,
) {
  const handler = (_event: Electron.IpcRendererEvent, payload: T) => {
    callback(payload);
  };

  ipcRenderer.on(channel, handler);
  return () => ipcRenderer.removeListener(channel, handler);
}

contextBridge.exposeInMainWorld("everythingApp", {
  getContext: () => ipcRenderer.invoke("app:get-context") as Promise<ServiceStatusSnapshot>,
  ensureService: () => ipcRenderer.invoke("service:ensure") as Promise<ServiceStatusSnapshot>,
  restartService: () => ipcRenderer.invoke("service:restart") as Promise<ServiceStatusSnapshot>,
  stopService: () => ipcRenderer.invoke("service:stop") as Promise<ServiceStatusSnapshot>,
  request: <T>(input: BackendRequest) =>
    ipcRenderer.invoke("backend:request", input) as Promise<T>,
  selectWorkspaceDirectory: () =>
    ipcRenderer.invoke("dialog:select-workspace") as Promise<ServiceStatusSnapshot | null>,
  selectSkillDirectory: () =>
    ipcRenderer.invoke("dialog:select-skill") as Promise<string | null>,
  openExternal: (url: string) =>
    ipcRenderer.invoke("shell:open-external", url) as Promise<boolean>,
  onServiceStatus: (callback: (payload: ServiceStatusSnapshot) => void) =>
    registerListener("service:status", callback),
  onServiceLog: (callback: (payload: string) => void) =>
    registerListener("service:log", callback),
  onRuntimeEvent: (callback: (payload: ServiceEventPayload) => void) =>
    registerListener("runtime:event", callback),
});
