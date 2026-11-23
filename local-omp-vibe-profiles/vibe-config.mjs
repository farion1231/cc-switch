import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { isMap, isScalar, isSeq, parseDocument, visit } from "yaml";

const keys = ["sonic", "task"];
const workerPath = ["task", "agentModelOverrides"];
const maxBytes = 1024 * 1024;
const unsafe = () =>
  new Error("目标配置使用了无法安全修改的 YAML 结构；未写入 OMP 配置。");
const isSelector = (s) =>
  typeof s === "string" && s.trim() && !/[\r\n]/.test(s);
const isRole = (role) => typeof role === "string" && /^[A-Za-z][\w-]*$/.test(role);
const ownValue = (record, key) => Object.hasOwn(record, key) ? record[key] : null;

function selectors(value) {
  if (value == null || isSelector(value)) return value ?? null;
  if (Array.isArray(value) && value.every(isSelector)) return value;
  throw new Error("模型必须是单行选择器或选择器列表。");
}

function workers(value) {
  if (
    !value ||
    typeof value !== "object" ||
    !keys.every((key) => Object.hasOwn(value, key))
  ) {
    throw new Error("模板缺少 Worker 配置。");
  }
  return { sonic: selectors(value.sonic), task: selectors(value.task) };
}

function roles(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("模板角色配置格式错误。");
  }
  return Object.fromEntries(
    Object.entries(value)
      .filter(([role]) => isRole(role) && role !== "default")
      .map(([role, selector]) => {
        if (selector !== null && !isSelector(selector)) {
          throw new Error("角色模型必须是单行选择器。");
        }
        return [role, selector];
      }),
  );
}

function preset(value) {
  if (!value || typeof value !== "object") throw new Error("模板格式错误。");
  return { workers: workers(value.workers), roles: roles(value.roles) };
}

function normalizePreset(value) {
  return { workers: workers(value?.workers ?? value), roles: roles(value?.roles ?? {}) };
}

function parse(text) {
  const doc = parseDocument(text, { merge: true });
  if (doc.errors.length || doc.warnings.length) throw unsafe();
  const data = doc.toJS({ maxAliasCount: 50 }) ?? {};
  if (typeof data !== "object" || Array.isArray(data)) throw unsafe();
  return { doc, data };
}

function snapshot(data) {
  const overrides = data.task?.agentModelOverrides;
  return { sonic: selectors(overrides?.sonic), task: selectors(overrides?.task) };
}

function snapshotRoles(data) {
  return Object.fromEntries(
    Object.entries(data.modelRoles ?? {}).filter(
      ([role, selector]) => isRole(role) && isSelector(selector),
    ),
  );
}

function snapshotPreset(data) {
  return { workers: snapshot(data), roles: snapshotRoles(data) };
}

function replacePath(text, segments, next, current) {
  const { doc } = parse(text);
  if (isDeepStrictEqual(current, next)) return text;
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const splice = (start, end, replacement) => text.slice(0, start) + replacement + text.slice(end);
  const lines = (depth, indent) => segments.slice(depth).map((segment, i, remaining) => `${" ".repeat(indent + i * 2)}${segment}:${i === remaining.length - 1 ? ` ${encode(next)}` : ""}${eol}`).join("");
  let map = doc.contents;
  for (let depth = 0; depth < segments.length; depth++) {
    if (depth === 0 && (!map || (isScalar(map) && map.source === ""))) {
      const index = map?.range[0] ?? text.length;
      return splice(index, index, (index && !text.slice(0, index).endsWith("\n") ? eol : "") + lines(0, 0));
    }
    if (!isMap(map) || map.anchor || map.tag || map.flow) throw unsafe();
    const pair = map.items.find(({ key }) => isScalar(key) && key.value === segments[depth]);
    if (!pair) {
      if (next === null || !map.items.length) throw unsafe();
      const last = map.items.at(-1), indent = depth === 0 ? 0 : map.range[0] - startOfLine(text, map.range[0]), end = (last.value ?? last.key).range[2];
      const tail = restOfLine(text, end), at = text[end - 1] === "\n" ? end : end + tail[0].length;
      return splice(at, at, (at === end ? "" : eol) + lines(depth, indent));
    }
    const node = pair.value;
    if (depth < segments.length - 1) {
      if (isMap(node) && !node.flow) { map = node; continue; }
      if (!((isScalar(node) && node.value === null) || (isMap(node) && !node.items.length)) || node.anchor || node.tag) throw unsafe();
      const start = pair.key.range[1] + 1, tail = restOfLine(text, node.range[1]), indent = pair.key.range[0] - startOfLine(text, pair.key.range[0]);
      if (text[start - 1] !== ":" || !/^\s*(?:#.*)?$/.test(tail[1])) throw unsafe();
      return splice(start, node.range[1] + tail[0].length, (tail[1].startsWith("#") ? " " : "") + tail[1] + (tail[2] || eol) + lines(depth + 1, indent + 2));
    }
    let unsupported = false;
    visit(node, { Alias: () => { unsupported = true; }, Node: (_, child) => { if (child.anchor || child.tag) unsupported = true; } });
    if (unsupported || !(isScalar(node) || isSeq(node))) throw unsafe();
    const [valueStart, valueEnd] = node.range;
    if (next !== null && startOfLine(text, pair.key.range[0]) === startOfLine(text, valueStart)) return splice(valueStart, valueEnd, encode(next));
    const rowStart = startOfLine(text, pair.key.range[0]), tail = text[valueEnd - 1] === "\n" ? ["", "", ""] : restOfLine(text, valueEnd), end = valueEnd + tail[0].length, indent = text.slice(rowStart, pair.key.range[0]), colon = pair.key.range[1];
    if (!/^ *$/.test(indent) || text[colon] !== ":") throw unsafe();
    const header = restOfLine(text, colon + 1)[1], comment = header.trimStart().startsWith("#") ? header.trimStart() : tail[1].trimStart(), rendered = next !== null ? `${text.slice(rowStart, colon + 1)} ${encode(next)}` : map.items.length === 1 ? `${indent}{}` : "", kept = comment.startsWith("#") ? (rendered ? ` ${comment}` : indent + comment) : "";
    return splice(rowStart, end, rendered || kept ? rendered + kept + eol : "");
  }
  throw unsafe();
}

function replaceWorker(text, key, next) {
  return replacePath(text, [...workerPath, key], next, snapshot(parse(text).data)[key]);
}

function replaceRole(text, role, next) {
  return replacePath(text, ["modelRoles", role], next, ownValue(snapshotRoles(parse(text).data), role));
}

function patchPreset(text, value) {
  const wanted = preset(value);
  const current = snapshotPreset(parse(text).data);
  let after = keys.reduce(
    (doc, key) => replaceWorker(doc, key, wanted.workers[key]),
    text,
  );
  for (const [role, selector] of Object.entries(wanted.roles)) {
    after = replaceRole(after, role, selector);
  }
  const expected = { workers: wanted.workers, roles: { ...current.roles } };
  for (const [role, selector] of Object.entries(wanted.roles)) {
    if (selector === null) delete expected.roles[role];
    else expected.roles[role] = selector;
  }
  if (!isDeepStrictEqual(snapshotPreset(parse(after).data), expected)) throw unsafe();
  return after;
}
function read(pathname) {
  let fd;
  try {
    fd = fs.openSync(pathname, "r");
    if (!fs.fstatSync(fd).isFile()) throw unsafe();
    const buffer = Buffer.allocUnsafe(maxBytes + 1);
    let size = 0;
    while (size < buffer.length) {
      const count = fs.readSync(fd, buffer, size, buffer.length - size, null);
      if (!count) break;
      size += count;
    }
    if (size > maxBytes) throw new Error("文件超过 1 MiB，未读取。");
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(
      buffer.subarray(0, size),
    );
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

const startOfLine = (text, index) => text.lastIndexOf("\n", index - 1) + 1;
const restOfLine = (text, index) =>
  /^([^\r\n]*)(\r?\n|$)/.exec(text.slice(index));
const encode = (value) =>
  JSON.stringify(value).replace(
    /\u0085|\u2028|\u2029/g,
    (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );


function atomicWrite(filename, text, mode = 0o600) {
  if (Buffer.byteLength(text) > maxBytes)
    throw new Error("文件超过 1 MiB，未写入。");
  const temp = `${filename}.${randomUUID()}.tmp`;
  try {
    fs.writeFileSync(temp, text, { flag: "wx", mode });
    fs.renameSync(temp, filename);
  } finally {
    fs.rmSync(temp, { force: true });
  }
}

function stateFile(agentDir) {
  return path.join(agentDir, ".cc-switch-vibe-profiles.json");
}

export function createStore(agentDir) {
  const getConfigPath = () => {
    const candidates = ["config.yml", "config.yaml"].map((name) =>
      path.join(agentDir, name),
    );
    return (
      candidates.find((candidate) =>
        fs.lstatSync(candidate, { throwIfNoEntry: false }),
      ) ?? candidates[0]
    );
  };
  const filename = stateFile(agentDir);
  const readStore = () => {
    const text = read(filename);
    const saved =
      text === null
        ? { revision: 0, templates: [], original: null }
        : JSON.parse(text);
    if (
      !Number.isSafeInteger(saved.revision) ||
      !Array.isArray(saved.templates)
    )
      throw new Error("模板文件格式错误。");
    for (const item of saved.templates) {
      if (typeof item.name !== "string") throw new Error("模板文件格式错误。");
      const normalized = normalizePreset(item);
      item.workers = normalized.workers;
      item.roles = normalized.roles;
    }
    if (saved.original) saved.original = normalizePreset(saved.original);
    return saved;
  };
  const readConfig = () => {
    const configPath = getConfigPath();
    const stat = fs.lstatSync(configPath, { throwIfNoEntry: false });
    const text = read(configPath);
    const revision = createHash("sha256")
      .update(JSON.stringify([configPath, text]))
      .digest("hex");
    const { data } = parse(text ?? "");
    return { configPath, stat, text, revision, data };
  };
  const get = () => {
    const { configPath, revision, data } = readConfig();
    const saved = readStore();
    const roles = snapshotRoles(data);
    const allRoles = [...new Set([
      "default", "smol", "task",
      ...Object.keys(roles),
      ...(Array.isArray(data.cycleOrder) ? data.cycleOrder : []),
      ...Object.keys(data.modelTags ?? {}),
      ...saved.templates.flatMap((item) => Object.keys(item.roles)),
      ...Object.keys(saved.original?.roles ?? {}),
    ].filter(isRole))];
    return {
      configPath,
      revision,
      savedRevision: saved.revision,
      templates: saved.templates,
      original: saved.original,
      current: snapshot(data),
      roles,
      allRoles,
      fallback: { sonic: roles.smol || "@smol", task: roles.task || "@task" },
    };
  };

  function change(body) {
    fs.mkdirSync(agentDir, { recursive: true });
    const lock = `${filename}.lock`;
    let fd;
    try {
      fd = fs.openSync(lock, "wx", 0o600);
      const saved = readStore();
      if (body.savedRevision !== saved.revision) throw new Error("模板已在别处改变，请刷新后重试。");
      if (["save", "delete", "apply"].includes(body.action)) {
        const name = typeof body.name === "string" ? body.name.trim() : "";
        if (!name || name.length > 80) throw new Error("请输入 1 到 80 个字符的模板名称。");
        const index = saved.templates.findIndex((item) => item.name === name);
        if (body.action === "save") {
          const item = { name, workers: workers(body.workers), roles: roles(body.roles) };
          if (index === -1) saved.templates.push(item);
          else saved.templates[index] = item;
        } else if (index === -1) {
          throw new Error("模板不存在，请刷新后重试。");
        } else if (body.action === "delete") {
          saved.templates.splice(index, 1);
        }
        if (body.action !== "apply") {
          saved.revision++;
          atomicWrite(filename, JSON.stringify(saved, null, 2) + "\n");
          return get();
        }
      } else if (body.action !== "restore") {
        throw new Error("未知操作。");
      }
      const target = body.action === "restore" ? saved.original : saved.templates.find((item) => item.name === body.name?.trim());
      if (!target) throw new Error("找不到要应用的配置。");
      const { configPath, stat, text, revision, data } = readConfig();
      if (revision !== body.revision) throw new Error("OMP 配置已在别处改变，请刷新后重试。");
      if (stat && !stat.isFile()) throw new Error("拒绝修改非普通文件（包括符号链接）。");
      const after = patchPreset(text ?? "", target);
      if (after === (text ?? "")) return get();
      const before = snapshotPreset(data);
      const original = saved.original ?? { workers: before.workers, roles: {} };
      const captured = body.action === "apply" ? Object.entries(target.roles).filter(([role, value]) =>
        !Object.hasOwn(original.roles, role) && ownValue(before.roles, role) !== value,
      ) : [];
      if (!saved.original || captured.length) {
        saved.original = {
          workers: original.workers,
          roles: { ...original.roles, ...Object.fromEntries(captured.map(([role]) => [role, ownValue(before.roles, role)])) },
        };
        saved.revision++;
        atomicWrite(filename, JSON.stringify(saved, null, 2) + "\n");
      }
      const latest = readConfig();
      if (latest.revision !== revision || (latest.stat && !latest.stat.isFile())) throw new Error("OMP 配置已在别处改变，请刷新后重试。");
      atomicWrite(configPath, after, stat ? stat.mode & 0o777 : undefined);
      return get();
    } catch (error) {
      if (fd === undefined && error.code === "EEXIST") throw new Error("另一实例正在保存，请稍后刷新。");
      throw error;
    } finally {
      if (fd !== undefined) {
        fs.closeSync(fd);
        fs.unlinkSync(lock);
      }
    }
  }
  return { get, change };
}
