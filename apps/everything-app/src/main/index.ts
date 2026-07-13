import { app, BrowserWindow, dialog, ipcMain, Notification, shell } from "electron";
import path from "node:path";
import { EverythingBackendManager, type ServiceEventPayload } from "./backend";

const backend = new EverythingBackendManager(__dirname);
let mainWindow: BrowserWindow | null = null;

const notificationDedup = new Map<string, number>();

function notifyAutomationEvent(event: ServiceEventPayload) {
  if (!Notification.isSupported() || !event.event_kind.startsWith("automation.")) return;
  if (!["automation.finished", "automation.failed"].includes(event.event_kind)) return;

  const key = `${event.event_kind}:${event.sequence}`;
  if (notificationDedup.has(key)) return;
  const now = Date.now();
  notificationDedup.set(key, now);
  for (const [entry, timestamp] of notificationDedup) {
    if (now - timestamp > 10 * 60_000) notificationDedup.delete(entry);
  }

  const awaitingApproval = /status=AwaitingApproval\b/.test(event.detail);
  const failed = event.event_kind === "automation.failed" || /status=Failed\b/.test(event.detail);
  const notification = new Notification({
    title: awaitingApproval ? "Everything · Onay gerekiyor" : failed ? "Everything · Rutin başarısız" : "Everything · Rutin tamamlandı",
    body: awaitingApproval
      ? "Bir otonom rutin güvenli biçimde durdu ve açık onayını bekliyor."
      : failed
        ? "Bir rutin tamamlanamadı. Ayrıntılar ve kanıtlar Rutinler ekranında."
        : "Bir rutin tamamlandı. Sonuç ve kanıtlar Rutinler ekranında hazır.",
    silent: false,
  });
  notification.on("click", () => {
    if (!mainWindow) return;
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.show();
    mainWindow.focus();
  });
  notification.show();
}

function createWindow() {
  const window = new BrowserWindow({
    width: 1380,
    height: 900,
    minWidth: 980,
    minHeight: 680,
    backgroundColor: "#0d0e10",
    titleBarStyle: "hiddenInset",
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, "../preload/index.js"),
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    },
  });

  window.webContents.setWindowOpenHandler(({ url }) => {
    try {
      const target = new URL(url);
      if (target.protocol === "https:" || target.protocol === "http:") {
        void shell.openExternal(target.toString());
      }
    } catch {
      // Ignore malformed or unsupported external navigation targets.
    }
    return { action: "deny" };
  });

  const statusListener = (snapshot: unknown) => {
    window.webContents.send("service:status", snapshot);
  };

  const logListener = (line: unknown) => {
    window.webContents.send("service:log", line);
  };

  const eventListener = (event: unknown) => {
    window.webContents.send("runtime:event", event);
    if (event && typeof event === "object") {
      notifyAutomationEvent(event as ServiceEventPayload);
    }
  };

  backend.on("status", statusListener);
  backend.on("service-log", logListener);
  backend.on("runtime-event", eventListener);

  window.on("closed", () => {
    backend.off("status", statusListener);
    backend.off("service-log", logListener);
    backend.off("runtime-event", eventListener);
    mainWindow = null;
  });

  if (process.env.ELECTRON_RENDERER_URL) {
    void window.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    void window.loadFile(path.join(__dirname, "../renderer/index.html"));
  }

  return window;
}

function registerIpc() {
  ipcMain.handle("app:get-context", async () => backend.snapshot());
  ipcMain.handle("service:ensure", async () => backend.ensureRunning());
  ipcMain.handle("service:restart", async () => backend.restart());
  ipcMain.handle("service:stop", async () => backend.stop());
  ipcMain.handle("backend:request", async (_event, input) => backend.request(input));
  ipcMain.handle("dialog:select-workspace", async () => {
    const result = await dialog.showOpenDialog({
      title: "Select a project workspace",
      properties: ["openDirectory", "createDirectory"],
    });
    if (result.canceled || !result.filePaths[0]) return null;
    return backend.switchWorkspace(result.filePaths[0]);
  });
  ipcMain.handle("dialog:select-skill", async () => {
    const result = await dialog.showOpenDialog({
      title: "Select an Everything skill package",
      properties: ["openDirectory"],
    });
    return result.canceled ? null : result.filePaths[0] ?? null;
  });
  ipcMain.handle("shell:open-external", async (_event, value: unknown) => {
    if (typeof value !== "string") return false;
    let target: URL;
    try { target = new URL(value); } catch { return false; }
    if (target.protocol !== "https:") return false;
    await shell.openExternal(target.toString());
    return true;
  });
}

app.whenReady().then(() => {
  if (process.platform === "win32") app.setAppUserModelId("dev.everything.app");
  registerIpc();
  mainWindow = createWindow();
  void backend.ensureRunning();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      mainWindow = createWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    void backend.stop();
    app.quit();
  }
});

app.on("before-quit", () => {
  void backend.stop();
});
