/**
 * 终端「排队发送」：每个终端实例一条独立队列（跟随终端，互不影响）。
 *
 * 判定「上一条执行完成」采用输出静默法：
 * 1. 发出后先等待首次输出（最长 startTimeoutMs，超时视为已完成）；
 * 2. 收到输出后进入静默计时，连续 idleMs 没有新输出即判定本条完成；
 * 3. 完成后自动派发下一条。
 *
 * 之所以不用「检测提示符」：Claude Code / OpenCode 这类全屏 TUI 会不停重绘，
 * 原始字节里找不到稳定的结束标记；输出静默是唯一跨工具可靠的信号。
 */

/** 队列条目状态。 */
export type QueueItemStatus = "pending" | "sending" | "done" | "error";

export interface QueueItem {
  id: string;
  text: string;
  status: QueueItemStatus;
  error?: string;
  /** 开始发送的时间戳（毫秒） */
  sentAt?: number;
  /** 判定完成的时间戳（毫秒） */
  finishedAt?: number;
}

/** 派发阶段：空闲 / 等待首次输出 / 等待输出静止。 */
type Phase = "idle" | "awaitStart" | "awaitFinish";

export interface InstanceQueue {
  items: QueueItem[];
  /** 队列是否处于自动派发状态 */
  running: boolean;
  phase: Phase;
  /** 最近一次收到终端输出的时间戳 */
  lastOutputAt: number;
  /** 当前条目发出的时间戳 */
  sentAt: number;
  /** 当前正在执行的条目 id */
  currentId: string | null;
}

export interface QueueSettings {
  /** 判定「执行完成」所需的输出静默时长（毫秒） */
  idleMs: number;
  /** 发送后等待首次输出的最长时间（毫秒），超时视为完成 */
  startTimeoutMs: number;
  /** 输入框「发送」按钮默认走队列而非立即发送 */
  queueMode: boolean;
}

const SETTINGS_KEY = "cc-switch-terminal-queue-settings";
const QUEUE_KEY_PREFIX = "cc-switch-terminal-queue-";
const TICK_MS = 300;

const DEFAULT_SETTINGS: QueueSettings = {
  // 15s：Claude Code 跑长命令时可能十几秒没有新输出，太短会误判完成
  idleMs: 15_000,
  startTimeoutMs: 30_000,
  queueMode: true,
};

/** 只恢复未发送的条目；已完成/失败的条目不留到下一轮。 */
function loadItems(instanceId: string): QueueItem[] {
  try {
    const raw = window.localStorage.getItem(QUEUE_KEY_PREFIX + instanceId);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter(
        (item): item is QueueItem =>
          typeof item === "object" &&
          item !== null &&
          typeof (item as QueueItem).text === "string" &&
          (item as QueueItem).text.trim().length > 0,
      )
      .map((item) => ({ ...item, status: "pending" as QueueItemStatus }));
  } catch {
    return [];
  }
}

function saveItems(instanceId: string, items: QueueItem[]) {
  try {
    const pending = items.filter((item) => item.status === "pending");
    if (pending.length === 0) {
      window.localStorage.removeItem(QUEUE_KEY_PREFIX + instanceId);
      return;
    }
    window.localStorage.setItem(
      QUEUE_KEY_PREFIX + instanceId,
      JSON.stringify(pending),
    );
  } catch {
    // 忽略存储失败
  }
}

function loadSettings(): QueueSettings {
  try {
    const raw = window.localStorage.getItem(SETTINGS_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw) as Partial<QueueSettings>;
    return {
      idleMs:
        typeof parsed.idleMs === "number" && parsed.idleMs >= 1000
          ? parsed.idleMs
          : DEFAULT_SETTINGS.idleMs,
      startTimeoutMs:
        typeof parsed.startTimeoutMs === "number" && parsed.startTimeoutMs >= 1000
          ? parsed.startTimeoutMs
          : DEFAULT_SETTINGS.startTimeoutMs,
      queueMode:
        typeof parsed.queueMode === "boolean"
          ? parsed.queueMode
          : DEFAULT_SETTINGS.queueMode,
    };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings: QueueSettings) {
  try {
    window.localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // 忽略
  }
}

let idSeed = 0;
function nextId(): string {
  idSeed += 1;
  return `q-${Date.now().toString(36)}-${idSeed}`;
}

type Sender = (text: string) => Promise<void>;

/**
 * 全局单例：EmbeddedTerminal 注册发送函数并上报输出，
 * TerminalPrompt 读写队列，二者通过 instanceId 关联。
 */
class TerminalQueueStore {
  private queues = new Map<string, InstanceQueue>();
  private senders = new Map<string, Sender>();
  private listeners = new Set<() => void>();
  private timer: number | null = null;
  private settings: QueueSettings = { ...DEFAULT_SETTINGS };
  private hydrated = false;

  private hydrate() {
    if (this.hydrated) return;
    this.hydrated = true;
    if (typeof window !== "undefined") {
      this.settings = loadSettings();
    }
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private emit() {
    for (const listener of this.listeners) listener();
  }

  getSettings(): QueueSettings {
    this.hydrate();
    return this.settings;
  }

  updateSettings(patch: Partial<QueueSettings>) {
    this.hydrate();
    this.settings = { ...this.settings, ...patch };
    saveSettings(this.settings);
    this.emit();
  }

  /** 取（必要时创建）某个终端的队列。 */
  get(instanceId: string): InstanceQueue {
    this.hydrate();
    let queue = this.queues.get(instanceId);
    if (!queue) {
      queue = {
        items: loadItems(instanceId),
        running: false,
        phase: "idle",
        lastOutputAt: 0,
        sentAt: 0,
        currentId: null,
      };
      this.queues.set(instanceId, queue);
    }
    return queue;
  }

  /** 待发送 + 正在发送的数量（徽标用）。 */
  pendingCount(instanceId: string): number {
    return this.get(instanceId).items.filter(
      (item) => item.status === "pending" || item.status === "sending",
    ).length;
  }

  registerSender(instanceId: string, sender: Sender) {
    this.senders.set(instanceId, sender);
  }

  unregisterSender(instanceId: string) {
    this.senders.delete(instanceId);
  }

  hasSender(instanceId: string): boolean {
    return this.senders.has(instanceId);
  }

  enqueue(instanceId: string, text: string): boolean {
    const content = text.trim();
    if (!content) return false;
    const queue = this.get(instanceId);
    queue.items = [
      ...queue.items,
      { id: nextId(), text: content, status: "pending" },
    ];
    saveItems(instanceId, queue.items);
    this.emit();
    return true;
  }

  remove(instanceId: string, itemId: string) {
    const queue = this.get(instanceId);
    queue.items = queue.items.filter((item) => item.id !== itemId);
    if (queue.currentId === itemId) {
      queue.currentId = null;
      queue.phase = "idle";
    }
    saveItems(instanceId, queue.items);
    this.emit();
  }

  /** 上移/下移一条待发送项。 */
  move(instanceId: string, itemId: string, delta: number) {
    const queue = this.get(instanceId);
    const index = queue.items.findIndex((item) => item.id === itemId);
    if (index === -1) return;
    const target = index + delta;
    if (target < 0 || target >= queue.items.length) return;
    const next = [...queue.items];
    const [moved] = next.splice(index, 1);
    next.splice(target, 0, moved);
    queue.items = next;
    saveItems(instanceId, queue.items);
    this.emit();
  }

  clear(instanceId: string) {
    const queue = this.get(instanceId);
    queue.items = [];
    queue.currentId = null;
    queue.phase = "idle";
    queue.running = false;
    saveItems(instanceId, queue.items);
    this.emit();
  }

  /** 清除已完成/失败的条目，保留未发送与正在发送的。 */
  clearFinished(instanceId: string) {
    const queue = this.get(instanceId);
    queue.items = queue.items.filter(
      (item) => item.status === "pending" || item.status === "sending",
    );
    saveItems(instanceId, queue.items);
    this.emit();
  }

  start(instanceId: string) {
    const queue = this.get(instanceId);
    if (queue.running) return;
    queue.running = true;
    // 注意：不要把 phase 从 idle 改成别的——tick 的 idle 分支负责挑下一条并发送，
    // 提前改掉会导致刚入队的这条永远不会被派发。
    queue.lastOutputAt = Date.now();
    this.ensureTimer();
    this.emit();
  }

  pause(instanceId: string) {
    const queue = this.get(instanceId);
    queue.running = false;
    this.emit();
  }

  /** 跳过当前条目（不等待其完成，立即派发下一条）。 */
  skip(instanceId: string) {
    const queue = this.get(instanceId);
    if (queue.currentId) {
      queue.items = queue.items.map((item) =>
        item.id === queue.currentId && item.status === "sending"
          ? { ...item, status: "done" as QueueItemStatus, finishedAt: Date.now() }
          : item,
      );
    }
    queue.currentId = null;
    queue.phase = "idle";
    queue.lastOutputAt = Date.now();
    saveItems(instanceId, queue.items);
    this.emit();
  }

  /** 终端有输出：刷新静默计时（由 EmbeddedTerminal 调用）。 */
  notifyOutput(instanceId: string) {
    const queue = this.queues.get(instanceId);
    if (!queue) return;
    queue.lastOutputAt = Date.now();
  }

  /** 删除终端实例时清理其队列与落盘数据。 */
  dispose(instanceId: string) {
    this.queues.delete(instanceId);
    this.senders.delete(instanceId);
    try {
      window.localStorage.removeItem(QUEUE_KEY_PREFIX + instanceId);
    } catch {
      // 忽略
    }
    // 没有队列了就停掉定时器：否则残留的 handle 会让后续 start 误以为
    // 定时器还在跑（例如定时器被外部环境重置后），导致队列永不派发。
    this.stopTimerIfIdle();
    this.emit();
  }

  private ensureTimer() {
    if (this.timer !== null) return;
    if (typeof window === "undefined") return;
    this.timer = window.setInterval(() => void this.tick(), TICK_MS);
  }

  private stopTimerIfIdle() {
    if (this.timer === null) return;
    for (const queue of this.queues.values()) {
      if (queue.running) return;
    }
    window.clearInterval(this.timer);
    this.timer = null;
  }

  private async tick() {
    let changed = false;
    for (const [instanceId, queue] of this.queues) {
      if (!queue.running) continue;
      const now = Date.now();
      const { idleMs, startTimeoutMs } = this.getSettings();

      if (queue.phase === "idle") {
        const next = queue.items.find((item) => item.status === "pending");
        if (!next) {
          queue.running = false;
          changed = true;
          continue;
        }
        const sender = this.senders.get(instanceId);
        if (!sender) {
          // 会话还没连上（终端刚创建 / 重启中）：保持 running，等注册后再发
          continue;
        }
        queue.items = queue.items.map((item) =>
          item.id === next.id
            ? { ...item, status: "sending" as QueueItemStatus, sentAt: now }
            : item,
        );
        queue.currentId = next.id;
        queue.sentAt = now;
        queue.lastOutputAt = now;
        queue.phase = "awaitStart";
        saveItems(instanceId, queue.items);
        changed = true;
        void sender(next.text).catch((error) => {
          const failed = this.queues.get(instanceId);
          if (!failed) return;
          failed.items = failed.items.map((item) =>
            item.id === next.id
              ? {
                  ...item,
                  status: "error" as QueueItemStatus,
                  error: String(error),
                  finishedAt: Date.now(),
                }
              : item,
          );
          failed.currentId = null;
          failed.phase = "idle";
          failed.lastOutputAt = Date.now();
          saveItems(instanceId, failed.items);
          this.emit();
        });
        continue;
      }

      if (queue.phase === "awaitStart") {
        if (queue.lastOutputAt > queue.sentAt) {
          queue.phase = "awaitFinish";
          changed = true;
        } else if (now - queue.sentAt >= startTimeoutMs) {
          this.finishCurrent(instanceId, queue);
          changed = true;
        }
        continue;
      }

      // awaitFinish：输出静止 idleMs 即判定本条完成
      if (now - queue.lastOutputAt >= idleMs) {
        this.finishCurrent(instanceId, queue);
        changed = true;
      }
    }
    if (changed) {
      this.emit();
      this.stopTimerIfIdle();
    }
  }

  private finishCurrent(instanceId: string, queue: InstanceQueue) {
    if (queue.currentId) {
      const id = queue.currentId;
      queue.items = queue.items.map((item) =>
        item.id === id && item.status === "sending"
          ? { ...item, status: "done" as QueueItemStatus, finishedAt: Date.now() }
          : item,
      );
    }
    queue.currentId = null;
    queue.phase = "idle";
    queue.lastOutputAt = Date.now();
    saveItems(instanceId, queue.items);
  }
}

export const terminalQueue = new TerminalQueueStore();
