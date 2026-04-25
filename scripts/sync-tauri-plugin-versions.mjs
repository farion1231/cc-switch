import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { parse as parseToml } from "smol-toml";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..");
const cargoTomlPath = path.join(repoRoot, "src-tauri", "Cargo.toml");
const cargoLockPath = path.join(repoRoot, "src-tauri", "Cargo.lock");
const packageJsonPath = path.join(repoRoot, "package.json");

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function readToml(filePath) {
  return parseToml(fs.readFileSync(filePath, "utf8"));
}

function parseVersion(version) {
  const match = String(version).trim().match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!match) {
    return null;
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
  };
}

function compareParsedVersion(a, b) {
  if (a.major !== b.major) {
    return a.major - b.major;
  }
  if (a.minor !== b.minor) {
    return a.minor - b.minor;
  }
  return a.patch - b.patch;
}

function normalizeRustVersionToNpmRange(version) {
  const raw = String(version).trim();

  if (/^\d+$/.test(raw)) {
    return `^${raw}.0.0`;
  }

  if (/^\d+\.\d+$/.test(raw)) {
    return `^${raw}.0`;
  }

  if (/^\d+\.\d+\.\d+$/.test(raw)) {
    const parsed = parseVersion(raw);
    if (parsed) {
      return `^${parsed.major}.${parsed.minor}.0`;
    }
    return raw;
  }

  if (/^[~^]/.test(raw)) {
    return raw;
  }

  return raw;
}

function getRustDepVersion(depValue) {
  if (typeof depValue === "string") {
    return depValue;
  }

  if (depValue && typeof depValue === "object" && typeof depValue.version === "string") {
    return depValue.version;
  }

  return null;
}

function collectDependencyTables(node, results = []) {
  if (!node || typeof node !== "object") {
    return results;
  }

  if (node.dependencies && typeof node.dependencies === "object") {
    results.push(node.dependencies);
  }

  for (const value of Object.values(node)) {
    collectDependencyTables(value, results);
  }

  return results;
}

function collectRustPluginVersions(cargoToml) {
  const dependencyTables = collectDependencyTables(cargoToml);
  const versionsByRustPackage = new Map();

  for (const deps of dependencyTables) {
    for (const [rustName, depValue] of Object.entries(deps)) {
      if (!rustName.startsWith("tauri-plugin-")) {
        continue;
      }

      const rustVersion = getRustDepVersion(depValue);
      if (!rustVersion) {
        continue;
      }

      versionsByRustPackage.set(rustName, rustVersion.trim());
    }
  }

  return versionsByRustPackage;
}

function collectResolvedPluginVersionsFromLock(cargoLockToml) {
  const packages = Array.isArray(cargoLockToml.package) ? cargoLockToml.package : [];
  const resolvedByRustPackage = new Map();

  for (const pkg of packages) {
    if (!pkg || typeof pkg !== "object") {
      continue;
    }

    const name = typeof pkg.name === "string" ? pkg.name : null;
    const version = typeof pkg.version === "string" ? pkg.version : null;
    if (!name || !version || !name.startsWith("tauri-plugin-")) {
      continue;
    }

    const parsed = parseVersion(version);
    if (!parsed) {
      continue;
    }

    const previous = resolvedByRustPackage.get(name);
    if (!previous || compareParsedVersion(parsed, previous.parsed) > 0) {
      resolvedByRustPackage.set(name, { version, parsed });
    }
  }

  return resolvedByRustPackage;
}

function collectResolvedCrateVersionsFromLock(cargoLockToml, crateNames) {
  const wanted = new Set(crateNames);
  const packages = Array.isArray(cargoLockToml.package) ? cargoLockToml.package : [];
  const resolved = new Map();

  for (const pkg of packages) {
    if (!pkg || typeof pkg !== "object") {
      continue;
    }

    const name = typeof pkg.name === "string" ? pkg.name : null;
    const version = typeof pkg.version === "string" ? pkg.version : null;
    if (!name || !version || !wanted.has(name)) {
      continue;
    }

    const parsed = parseVersion(version);
    if (!parsed) {
      continue;
    }

    const previous = resolved.get(name);
    if (!previous || compareParsedVersion(parsed, previous.parsed) > 0) {
      resolved.set(name, { version, parsed });
    }
  }

  return resolved;
}

function isMajorOnlyConstraint(version) {
  return /^(?:[~^])?\d+$/.test(String(version).trim());
}

function strengthenCargoConstraintIfNeeded(currentConstraint, resolvedVersion) {
  if (!isMajorOnlyConstraint(currentConstraint)) {
    return null;
  }

  const majorMatch = String(currentConstraint).trim().match(/(\d+)/);
  if (!majorMatch || !resolvedVersion) {
    return null;
  }

  const resolved = parseVersion(resolvedVersion);
  if (!resolved) {
    return null;
  }

  if (Number(majorMatch[1]) !== resolved.major) {
    return null;
  }

  return `${resolved.major}.${resolved.minor}`;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function updateCargoConstraintInText(cargoTomlText, rustDepName, nextConstraint) {
  const escapedDep = escapeRegExp(rustDepName);
  let replaced = false;
  let updatedText = cargoTomlText;

  const inlineTablePattern = new RegExp(
    `(^\\s*${escapedDep}\\s*=\\s*\\{[^\\n]*?\\bversion\\s*=\\s*")([^"]+)(")`,
    "gm",
  );
  updatedText = updatedText.replace(inlineTablePattern, (_full, prefix, current, suffix) => {
    if (current === nextConstraint) {
      return `${prefix}${current}${suffix}`;
    }

    replaced = true;
    return `${prefix}${nextConstraint}${suffix}`;
  });

  const stringPattern = new RegExp(`(^\\s*${escapedDep}\\s*=\\s*")([^"]+)(".*$)`, "gm");
  updatedText = updatedText.replace(stringPattern, (_full, prefix, current, suffix) => {
    if (current === nextConstraint) {
      return `${prefix}${current}${suffix}`;
    }

    replaced = true;
    return `${prefix}${nextConstraint}${suffix}`;
  });

  return { updatedText, replaced };
}

function strengthenCargoPluginConstraints(cargoTomlText, rustPluginConstraints, resolvedPluginVersions) {
  let nextText = cargoTomlText;
  const updates = [];
  const finalConstraints = new Map(rustPluginConstraints);

  for (const [rustDepName, currentConstraint] of rustPluginConstraints.entries()) {
    const resolvedInfo = resolvedPluginVersions.get(rustDepName);
    const strengthened = strengthenCargoConstraintIfNeeded(
      currentConstraint,
      resolvedInfo?.version ?? null,
    );
    if (!strengthened || strengthened === currentConstraint) {
      continue;
    }

    const result = updateCargoConstraintInText(nextText, rustDepName, strengthened);
    if (!result.replaced) {
      continue;
    }

    nextText = result.updatedText;
    finalConstraints.set(rustDepName, strengthened);
    updates.push({ rustDepName, from: currentConstraint, to: strengthened });
  }

  return { cargoTomlText: nextText, updates, finalConstraints };
}

function buildNpmPluginRanges(rustPluginConstraints) {
  const rangesByNpmPackage = new Map();

  for (const [rustName, rustConstraint] of rustPluginConstraints.entries()) {
    const npmName = rustName.replace(/^tauri-plugin-/, "@tauri-apps/plugin-");
    const npmRange = normalizeRustVersionToNpmRange(rustConstraint);
    rangesByNpmPackage.set(npmName, npmRange);
  }

  return rangesByNpmPackage;
}

function applyVersionUpdates(packageJson, rustPluginVersions) {
  const sections = ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"];
  const updates = [];

  for (const sectionName of sections) {
    const section = packageJson[sectionName];
    if (!section || typeof section !== "object") {
      continue;
    }

    for (const [pkgName, rustRange] of rustPluginVersions.entries()) {
      if (!(pkgName in section)) {
        continue;
      }

      const current = section[pkgName];
      if (current === rustRange) {
        continue;
      }

      section[pkgName] = rustRange;
      updates.push({ section: sectionName, pkgName, from: current, to: rustRange });
    }
  }

  return updates;
}

function runCommand(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: "inherit",
    encoding: "utf8",
  });

  if (result.error) {
    if (result.error.code === "ENOENT") {
      throw new Error(`Missing required command: ${command}`);
    }
    throw result.error;
  }

  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with exit code ${result.status}`);
  }
}

function main() {
  const cargoTomlText = fs.readFileSync(cargoTomlPath, "utf8");
  const cargoToml = parseToml(cargoTomlText);
  const cargoLockToml = readToml(cargoLockPath);
  const packageJson = readJson(packageJsonPath);

  const rustPluginConstraints = collectRustPluginVersions(cargoToml);
  const resolvedPluginVersions = collectResolvedPluginVersionsFromLock(cargoLockToml);
  const {
    cargoTomlText: nextCargoTomlText,
    updates: cargoConstraintUpdates,
    finalConstraints,
  } = strengthenCargoPluginConstraints(cargoTomlText, rustPluginConstraints, resolvedPluginVersions);

  const npmPluginRanges = buildNpmPluginRanges(finalConstraints);
  const resolvedCoreCrates = collectResolvedCrateVersionsFromLock(cargoLockToml, ["tauri"]);
  const resolvedTauriCoreVersion = resolvedCoreCrates.get("tauri")?.version ?? null;
  if (resolvedTauriCoreVersion) {
    npmPluginRanges.set("@tauri-apps/api", normalizeRustVersionToNpmRange(resolvedTauriCoreVersion));
  }

  const packageJsonUpdates = applyVersionUpdates(packageJson, npmPluginRanges);

  if (cargoConstraintUpdates.length > 0) {
    fs.writeFileSync(cargoTomlPath, nextCargoTomlText, "utf8");
  }

  if (packageJsonUpdates.length > 0) {
    fs.writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`, "utf8");
  }

  if (cargoConstraintUpdates.length === 0 && packageJsonUpdates.length === 0) {
    console.log("No tauri plugin version conflicts found.");
  } else {
    if (cargoConstraintUpdates.length > 0) {
      console.log(`Strengthened ${cargoConstraintUpdates.length} Cargo plugin constraint(s):`);
      for (const update of cargoConstraintUpdates) {
        console.log(`- ${update.rustDepName}: ${update.from} -> ${update.to}`);
      }
    }

    if (packageJsonUpdates.length > 0) {
      console.log(`Updated ${packageJsonUpdates.length} npm plugin version(s):`);
      for (const update of packageJsonUpdates) {
        console.log(`- ${update.section}.${update.pkgName}: ${update.from} -> ${update.to}`);
      }
    }
  }

  console.log("Updating pnpm lockfile...");
  runCommand("pnpm", ["install", "--lockfile-only"], repoRoot);

  console.log("Updating Cargo.lock...");
  runCommand("cargo", ["generate-lockfile", "--manifest-path", cargoTomlPath], repoRoot);
}

main();
