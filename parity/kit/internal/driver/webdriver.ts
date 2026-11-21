// A minimal W3C WebDriver client. Every call has a deadline, so a driver that stops answering
// fails the step instead of hanging the run.

export class WebDriverError extends Error {
  readonly status: number;
  readonly error: string;
  constructor(message: string, status: number, error: string) {
    super(message);
    this.status = status;
    this.error = error;
  }
}

export const ELEMENT_KEY = "element-6066-11e4-a52e-4f735466cecf";

export class WebDriver {
  readonly base: string;
  sid: string | null = null;
  constructor(base: string) {
    this.base = base;
  }

  async call<T = unknown>(method: string, path: string, body?: unknown, timeoutMs = 30000): Promise<T> {
    const res = await fetch(this.base + path, {
      method,
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
    });
    const json = (await res.json().catch(() => ({}))) as { value?: { error?: string; message?: string } };
    if (!res.ok) {
      const v = json.value ?? {};
      throw new WebDriverError(`${method} ${path} -> ${res.status} ${v.error ?? ""}: ${(v.message ?? "").slice(0, 300)}`, res.status, v.error ?? "");
    }
    return json.value as T;
  }

  private s(path: string): string {
    if (!this.sid) throw new Error("no WebDriver session");
    return `/session/${this.sid}${path}`;
  }

  async newSession(capabilities: unknown): Promise<void> {
    const v = await this.call<{ sessionId: string }>("POST", "/session", { capabilities }, 60000);
    this.sid = v.sessionId;
  }
  async deleteSession(): Promise<void> {
    if (!this.sid) return;
    await this.call("DELETE", this.s(""), undefined, 15000).catch(() => {});
    this.sid = null;
  }
  setScriptTimeout(ms: number): Promise<unknown> {
    return this.call("POST", this.s("/timeouts"), { script: ms });
  }
  exec<T = unknown>(script: string, args: unknown[] = []): Promise<T> {
    return this.call<T>("POST", this.s("/execute/sync"), { script, args });
  }
  actions(actions: unknown[]): Promise<unknown> {
    return this.call("POST", this.s("/actions"), { actions });
  }
  releaseActions(): Promise<unknown> {
    return this.call("DELETE", this.s("/actions")).catch(() => undefined);
  }
  sendKeys(elementId: string, text: string): Promise<unknown> {
    return this.call("POST", this.s(`/element/${elementId}/value`), { text });
  }
  windowRect(): Promise<{ x: number; y: number; width: number; height: number }> {
    return this.call("GET", this.s("/window/rect"));
  }
}
