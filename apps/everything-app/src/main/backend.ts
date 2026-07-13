import { ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import path from "node:path";
import net from "node:net";

export type ServiceLifecycle = "idle" | "starting" | "ready" | "stopped" | "error";

export interface ServiceStatusSnapshot {
  status: ServiceLifecycle;
  detail: string;
  workspaceRoot: string;
  serviceUrl: string;
  pid: number | null;
  recentLogs: string[];
}

export interface ServiceEventPayload {
  sequence: number;
  timestamp_epoch_millis: number;
  event_kind: string;
  run_id?: string | null;
  stage?: string | null;
  detail: string;
}

const LOG_LIMIT = 120;
const HEALTH_TIMEOUT_MS = 300_000;
const HEALTH_RETRY_MS = 1_000;

interface LaunchCommand {
  command: string;
  args: string[];
  description: string;
}

function trimLogLine(value: string) {
  return value.replace(/\r/g, "").trim();
}

function findWorkspaceRoot(startingPoint: string) {
  const candidates = [startingPoint, process.cwd()];

  for (const candidate of candidates) {
    let current = path.resolve(candidate);
    while (true) {
      if (fs.existsSync(path.join(current, "everything.toml"))) {
        return current;
      }

      const parent = path.dirname(current);
      if (parent === current) {
        break;
      }
      current = parent;
    }
  }

  throw new Error("Unable to find workspace root containing everything.toml");
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function validateExternalServiceUrl(raw: string | undefined) {
  if (!raw) return null;
  const url = new URL(raw);
  const loopbackHosts = new Set(["127.0.0.1", "localhost", "[::1]"]);
  if (url.protocol !== "http:" || !loopbackHosts.has(url.hostname)) {
    throw new Error("EVERYTHINGD_URL must be an HTTP loopback URL");
  }
  url.pathname = "/";
  url.search = "";
  url.hash = "";
  return url.origin;
}

function findFreeLoopbackPort() {
  return new Promise<number>((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        reject(new Error("Unable to allocate a local service port"));
        return;
      }
      const port = address.port;
      server.close((error) => {
        if (error) reject(error);
        else resolve(port);
      });
    });
  });
}

export class EverythingBackendManager extends EventEmitter {
  private child: ChildProcessWithoutNullStreams | null = null;
  private status: ServiceLifecycle = "idle";
  private detail = "Service is idle";
  private readonly host = "127.0.0.1";
  private port = 0;
  private workspaceRoot: string;
  private externalServiceUrl: string | null;
  private readonly recentLogs: string[] = [];
  private sseAbortController: AbortController | null = null;
  private activeStart: Promise<ServiceStatusSnapshot> | null = null;

  constructor(appPath: string) {
    super();
    this.externalServiceUrl = validateExternalServiceUrl(process.env.EVERYTHINGD_URL);
    const configuredWorkspace = process.env.EVERYTHING_WORKSPACE;
    this.workspaceRoot = configuredWorkspace
      ? findWorkspaceRoot(configuredWorkspace)
      : findWorkspaceRoot(appPath);
  }

  get serviceUrl() {
    return this.externalServiceUrl ?? `http://${this.host}:${this.port}`;
  }

  snapshot(): ServiceStatusSnapshot {
    return {
      status: this.status,
      detail: this.detail,
      workspaceRoot: this.workspaceRoot,
      serviceUrl: this.serviceUrl,
      pid: this.child?.pid ?? null,
      recentLogs: [...this.recentLogs],
    };
  }

  async ensureRunning() {
    if (this.status === "ready") {
      return this.snapshot();
    }

    if (this.activeStart) {
      return this.activeStart;
    }

    this.activeStart = this.startInternal();
    try {
      return await this.activeStart;
    } finally {
      this.activeStart = null;
    }
  }

  async restart() {
    await this.stop();
    return this.ensureRunning();
  }

  async switchWorkspace(selectedPath: string) {
    const candidate = path.resolve(selectedPath);
    const metadata = fs.statSync(candidate);
    if (!metadata.isDirectory()) {
      throw new Error("Selected workspace must be a directory");
    }

    const configPath = path.join(candidate, "everything.toml");
    if (!fs.existsSync(configPath)) {
      const currentConfig = path.join(this.workspaceRoot, "everything.toml");
      if (!fs.existsSync(currentConfig)) {
        throw new Error("Cannot create workspace config because the current template is missing");
      }
      fs.copyFileSync(currentConfig, configPath, fs.constants.COPYFILE_EXCL);
    }

    await this.stop();
    this.externalServiceUrl = null;
    this.workspaceRoot = candidate;
    this.recentLogs.length = 0;
    this.port = 0;
    this.updateStatus("idle", `Workspace changed to ${candidate}`);
    return this.ensureRunning();
  }

  async stop() {
    this.stopEventStream();

    if (!this.child) {
      this.updateStatus("stopped", "Service is not running");
      return this.snapshot();
    }

    const current = this.child;
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        if (!current.killed) {
          current.kill("SIGKILL");
        }
      }, 5_000);

      current.once("exit", () => {
        clearTimeout(timer);
        resolve();
      });

      current.kill("SIGTERM");
    });

    return this.snapshot();
  }

  async request<T>(input: {
    method: "GET" | "POST" | "DELETE";
    path: string;
    query?: Record<string, string | number | undefined>;
    body?: unknown;
  }): Promise<T> {
    await this.ensureRunning();

    if (!input.path.startsWith("/v1/")) {
      throw new Error("Desktop API requests must target a versioned local /v1/ route");
    }

    const url = new URL(input.path, `${this.serviceUrl}/`);
    if (url.origin !== this.serviceUrl || !url.pathname.startsWith("/v1/")) {
      throw new Error("Desktop API requests cannot leave the local Everything service");
    }

    if (input.query) {
      for (const [key, value] of Object.entries(input.query)) {
        if (value !== undefined && value !== "") {
          url.searchParams.set(key, String(value));
        }
      }
    }

    const response = await fetch(url, {
      method: input.method,
      headers: input.body ? { "content-type": "application/json" } : undefined,
      body: input.body ? JSON.stringify(input.body) : undefined,
    });

    const text = await response.text();
    let payload: any = null;
    if (text) {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = { error: text };
      }
    }

    if (!response.ok) {
      const errorMessage =
        typeof payload?.error === "string"
          ? payload.error
          : `Request failed with status ${response.status}`;
      throw new Error(errorMessage);
    }

    return payload as T;
  }

  private async startInternal() {
    if (this.externalServiceUrl) {
      try {
        return await this.connectExternalService();
      } catch (error) {
        const message = error instanceof Error ? error.message : "unknown external service error";
        this.pushLog("stderr", `background service unavailable; starting workspace-local daemon: ${message}`);
        this.externalServiceUrl = null;
      }
    }

    this.port = await findFreeLoopbackPort();
    const launch = this.resolveLaunchCommand();
    this.updateStatus("starting", `Booting everythingd via ${launch.description}`);

    const child = spawn(launch.command, launch.args, {
      cwd: this.workspaceRoot,
      env: process.env,
      stdio: "pipe",
      windowsHide: true,
    });

    this.child = child;
    this.attachProcessListeners(child);
    await this.waitForHealthy();
    this.updateStatus("ready", "everythingd is accepting requests");
    this.startEventStream();
    return this.snapshot();
  }

  private async connectExternalService() {
    this.updateStatus("starting", `Connecting to background service at ${this.serviceUrl}`);
    const response = await fetch(`${this.serviceUrl}/v1/info`, {
      signal: AbortSignal.timeout(3_000),
    });
    if (!response.ok) {
      throw new Error(`background service health check returned ${response.status}`);
    }
    const info = (await response.json()) as { workspace?: string };
    if (!info.workspace || path.resolve(info.workspace) !== path.resolve(this.workspaceRoot)) {
      throw new Error("background service is running for a different workspace");
    }
    this.updateStatus("ready", "Connected to the persistent Everything background service");
    this.startEventStream();
    return this.snapshot();
  }

  private resolveLaunchCommand(): LaunchCommand {
    const exeName = process.platform === "win32" ? "everythingd.exe" : "everythingd";
    const candidatePaths = [
      process.env.EVERYTHINGD_BIN,
      path.join(process.resourcesPath, "bin", exeName),
      path.join(this.workspaceRoot, "target", "release", exeName),
      path.join(this.workspaceRoot, "target", "debug", exeName),
    ].filter((candidate): candidate is string => Boolean(candidate));

    for (const candidate of candidatePaths) {
      if (fs.existsSync(candidate)) {
        return {
          command: candidate,
          args: [
            "--workspace",
            this.workspaceRoot,
            "--listen",
            `${this.host}:${this.port}`,
            "--oauth-listen",
            `${this.host}:0`,
          ],
          description: candidate,
        };
      }
    }

    return {
      command: "cargo",
      args: [
        "run",
        "-p",
        "everythingd",
        "--",
        "--workspace",
        this.workspaceRoot,
        "--listen",
        `${this.host}:${this.port}`,
        "--oauth-listen",
        `${this.host}:0`,
      ],
      description: "cargo run -p everythingd",
    };
  }

  private attachProcessListeners(child: ChildProcessWithoutNullStreams) {
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");

    child.stdout.on("data", (chunk: string) => {
      this.pushLog("stdout", chunk);
    });

    child.stderr.on("data", (chunk: string) => {
      this.pushLog("stderr", chunk);
    });

    child.once("error", (error) => {
      this.pushLog("stderr", error.message);
      this.updateStatus("error", `Failed to launch everythingd: ${error.message}`);
    });

    child.once("exit", (code, signal) => {
      this.stopEventStream();
      this.child = null;
      const detail = `everythingd exited (code=${code ?? "null"}, signal=${signal ?? "null"})`;
      const nextStatus = this.status === "error" ? "error" : "stopped";
      this.updateStatus(nextStatus, detail);
    });
  }

  private pushLog(stream: "stdout" | "stderr", raw: string) {
    const lines = raw
      .split("\n")
      .map(trimLogLine)
      .filter(Boolean);

    for (const line of lines) {
      const entry = `[${stream}] ${line}`;
      this.recentLogs.unshift(entry);
      this.recentLogs.splice(LOG_LIMIT);
      this.emit("service-log", entry);
    }
  }

  private updateStatus(status: ServiceLifecycle, detail: string) {
    this.status = status;
    this.detail = detail;
    this.emit("status", this.snapshot());
  }

  private async waitForHealthy() {
    const startedAt = Date.now();

    while (Date.now() - startedAt < HEALTH_TIMEOUT_MS) {
      if (!this.child) {
        throw new Error("everythingd process terminated before becoming healthy");
      }

      try {
        const response = await fetch(`${this.serviceUrl}/v1/info`);
        if (response.ok) {
          return;
        }
      } catch {
        // Ignore retryable startup failures.
      }

      await delay(HEALTH_RETRY_MS);
    }

    this.updateStatus("error", "Timed out waiting for everythingd to become healthy");
    throw new Error("Timed out waiting for everythingd to become healthy");
  }

  private stopEventStream() {
    if (this.sseAbortController) {
      this.sseAbortController.abort();
      this.sseAbortController = null;
    }
  }

  private async startEventStream() {
    this.stopEventStream();
    const abortController = new AbortController();
    this.sseAbortController = abortController;

    try {
      const response = await fetch(`${this.serviceUrl}/v1/events`, {
        signal: abortController.signal,
        headers: {
          accept: "text/event-stream",
        },
      });

      if (!response.ok || !response.body) {
        throw new Error(`Failed to subscribe to event stream (${response.status})`);
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          break;
        }

        buffer += decoder.decode(value, { stream: true });
        buffer = buffer.replace(/\r\n/g, "\n");

        let boundaryIndex = buffer.indexOf("\n\n");
        while (boundaryIndex >= 0) {
          const payload = buffer.slice(0, boundaryIndex);
          buffer = buffer.slice(boundaryIndex + 2);
          this.dispatchServerSentEvent(payload);
          boundaryIndex = buffer.indexOf("\n\n");
        }
      }
    } catch (error) {
      if (abortController.signal.aborted) {
        return;
      }

      const message =
        error instanceof Error ? error.message : "Unknown event stream failure";
      this.pushLog("stderr", `event stream error: ${message}`);
    }
  }

  private dispatchServerSentEvent(block: string) {
    let eventKind = "message";
    const dataLines: string[] = [];

    for (const line of block.split("\n")) {
      if (line.startsWith("event:")) {
        eventKind = line.slice("event:".length).trim();
      } else if (line.startsWith("data:")) {
        dataLines.push(line.slice("data:".length).trimStart());
      }
    }

    if (dataLines.length === 0) {
      return;
    }

    try {
      const payload = JSON.parse(dataLines.join("\n")) as ServiceEventPayload;
      this.emit("runtime-event", { ...payload, event_kind: eventKind });
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown event payload failure";
      this.pushLog("stderr", `event payload parse error: ${message}`);
    }
  }
}
