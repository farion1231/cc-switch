import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { parse as parseToml } from "smol-toml";
import { parse as parseYaml } from "yaml";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..");

const config = {
  paths: {
    repoRoot,
    cargoToml: path.join(repoRoot, "src-tauri", "Cargo.toml"),
    cargoLock: path.join(repoRoot, "src-tauri", "Cargo.lock"),
    packageJson: path.join(repoRoot, "package.json"),
    pnpmLockYaml: path.join(repoRoot, "pnpm-lock.yaml"),
  },
  packageSections: ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"],
  pnpmRootImporter: ".",
  tauriCorePair: {
    rustPackage: "tauri",
    npmPackage: "@tauri-apps/api",
  },
  tauriPlugin: {
    rustPrefix: "tauri-plugin-",
    npmPrefix: "@tauri-apps/plugin-",
  },
};

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function readToml(filePath) {
  return parseToml(fs.readFileSync(filePath, "utf8"));
}

function readYaml(filePath) {
  return parseYaml(fs.readFileSync(filePath, "utf8"));
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

function parseVersionOrConstraint(version) {
  return parseVersion(version) ?? parseConstraintInto(version);
}

function formatCargoTildeRequirement(version) {
  const parsed = parseVersionOrConstraint(version);
  if (!parsed || parsed.minor === null) {
    return String(version).trim();
  }

  // Tauri plugins are only compatible across patch releases within one minor line.
  return `~${parsed.major}.${parsed.minor}`;
}

function formatNpmTildeRequirement(version) {
  const parsed = parseVersionOrConstraint(version);
  if (!parsed || parsed.minor === null) {
    return String(version).trim();
  }

  // npm needs the patch component to make ~ resolve as >=x.y.0 <x.(y+1).0.
  return `~${parsed.major}.${parsed.minor}.0`;
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
      if (!rustName.startsWith(config.tauriPlugin.rustPrefix)) {
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
    if (!name || !version || !name.startsWith(config.tauriPlugin.rustPrefix)) {
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

function getPnpmRootImporter(pnpmLockYaml) {
  return pnpmLockYaml?.importers?.[config.pnpmRootImporter] ?? null;
}

function getPnpmResolvedVersion(lockfileVersion) {
  const version = String(lockfileVersion ?? "").split("(")[0].trim();
  return parseVersion(version) ? version : null;
}

function collectResolvedNpmVersionsFromLock(pnpmLockYaml, packageNames) {
  const wanted = new Set(packageNames);
  const importer = getPnpmRootImporter(pnpmLockYaml);
  const resolved = new Map();

  if (!importer || typeof importer !== "object") {
    return resolved;
  }

  for (const sectionName of config.packageSections) {
    const section = importer[sectionName];
    if (!section || typeof section !== "object") {
      continue;
    }

    for (const [packageName, entry] of Object.entries(section)) {
      if (!wanted.has(packageName) || !entry || typeof entry !== "object") {
        continue;
      }

      const version = getPnpmResolvedVersion(entry.version);
      if (!version) {
        continue;
      }

      resolved.set(packageName, {
        section: sectionName,
        version,
        parsed: parseVersion(version),
        specifier: typeof entry.specifier === "string" ? entry.specifier : null,
      });
    }
  }

  return resolved;
}

function parseConstraintInto(version) {
  const raw = String(version).trim();
  const match = raw.match(/^([~^])?(\d+)(?:\.(\d+))?(?:\.(\d+))?$/);
  if (!match) {
    return null;
  }

  return {
    prefix: match[1] ?? null,
    major: Number(match[2]),
    minor: match[3] !== undefined ? Number(match[3]) : null,
    patch: match[4] !== undefined ? Number(match[4]) : null,
  };
}

function syncCargoConstraintIfNeeded(currentConstraint, resolvedVersion) {
  const constraint = parseConstraintInto(currentConstraint);
  if (!constraint) {
    return null;
  }

  const resolved = resolvedVersion ? parseVersion(resolvedVersion) : null;
  if (resolved && constraint.major !== resolved.major) {
    return null;
  }

  // Prefer Cargo.lock because it records the actually-compatible minor already selected.
  const versionSource = resolvedVersion ?? currentConstraint;
  const expectedConstraint = formatCargoTildeRequirement(versionSource);
  return currentConstraint === expectedConstraint ? null : expectedConstraint;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function updateCargoConstraintInText(cargoTomlText, rustDepName, nextConstraint) {
  const escapedDep = escapeRegExp(rustDepName);
  let replaced = false;
  let updatedText = cargoTomlText;

  const inlineTablePattern = new RegExp(
    `(^\\s*${escapedDep}\\s*=\\s*\\{[^\\n]*?\\bversion\\s*=\\s*")([^"]+)(".*)$`,
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

function syncCargoPluginConstraints(cargoTomlText, rustPluginConstraints, resolvedPluginVersions) {
  let nextText = cargoTomlText;
  const updates = [];
  const finalConstraints = new Map(rustPluginConstraints);

  for (const [rustDepName, currentConstraint] of rustPluginConstraints.entries()) {
    const resolvedInfo = resolvedPluginVersions.get(rustDepName);
    const syncedConstraint = syncCargoConstraintIfNeeded(
      currentConstraint,
      resolvedInfo?.version ?? null,
    );
    if (!syncedConstraint || syncedConstraint === currentConstraint) {
      continue;
    }

    const result = updateCargoConstraintInText(nextText, rustDepName, syncedConstraint);
    if (!result.replaced) {
      continue;
    }

    nextText = result.updatedText;
    finalConstraints.set(rustDepName, syncedConstraint);
    updates.push({ rustDepName, from: currentConstraint, to: syncedConstraint });
  }

  return { cargoTomlText: nextText, updates, finalConstraints };
}

function buildTauriPackagePairs(rustPluginConstraints) {
  const pairs = [...rustPluginConstraints.keys()].map((rustPackage) => ({
    rustPackage,
    npmPackage: rustPackage.replace(config.tauriPlugin.rustPrefix, config.tauriPlugin.npmPrefix),
  }));

  // @tauri-apps/api is not a plugin, but it follows the Rust `tauri` release train.
  pairs.push(config.tauriCorePair);

  return pairs;
}

function buildCargoLockVersions(cargoLockToml) {
  const resolvedPluginVersions = collectResolvedPluginVersionsFromLock(cargoLockToml);
  const resolvedCoreVersions = collectResolvedCrateVersionsFromLock(cargoLockToml, [
    config.tauriCorePair.rustPackage,
  ]);

  return new Map([
    ...resolvedPluginVersions.entries(),
    ...resolvedCoreVersions.entries(),
  ]);
}

function collectLockfileConflicts(packagePairs, cargoLockVersions, pnpmLockVersions) {
  const conflicts = [];

  for (const { rustPackage, npmPackage } of packagePairs) {
    const cargoVersion = cargoLockVersions.get(rustPackage);
    const npmVersion = pnpmLockVersions.get(npmPackage);
    if (!cargoVersion || !npmVersion) {
      continue;
    }

    if (
      cargoVersion.parsed.major !== npmVersion.parsed.major ||
      cargoVersion.parsed.minor !== npmVersion.parsed.minor
    ) {
      conflicts.push({
        rustPackage,
        cargoVersion: cargoVersion.version,
        npmPackage,
        npmVersion: npmVersion.version,
      });
    }
  }

  return conflicts;
}

function buildNpmPackageRanges(packagePairs, cargoLockVersions, pnpmLockVersions, rustPluginConstraints) {
  const rangesByNpmPackage = new Map();

  for (const { rustPackage, npmPackage } of packagePairs) {
    const versionSource =
      cargoLockVersions.get(rustPackage)?.version ??
      pnpmLockVersions.get(npmPackage)?.version ??
      rustPluginConstraints.get(rustPackage);

    if (versionSource) {
      rangesByNpmPackage.set(npmPackage, formatNpmTildeRequirement(versionSource));
    }
  }

  return rangesByNpmPackage;
}

function applyVersionUpdates(packageJson, rustPluginVersions) {
  const updates = [];

  for (const sectionName of config.packageSections) {
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

function readProjectFiles() {
  const cargoTomlText = fs.readFileSync(config.paths.cargoToml, "utf8");

  return {
    cargoTomlText,
    cargoToml: parseToml(cargoTomlText),
    cargoLockToml: readToml(config.paths.cargoLock),
    packageJson: readJson(config.paths.packageJson),
    pnpmLockYaml: readYaml(config.paths.pnpmLockYaml),
  };
}

function inspectLockfiles(cargoLockToml, pnpmLockYaml, rustPluginConstraints) {
  const packagePairs = buildTauriPackagePairs(rustPluginConstraints);
  const cargoLockVersions = buildCargoLockVersions(cargoLockToml);
  const pnpmLockVersions = collectResolvedNpmVersionsFromLock(
    pnpmLockYaml,
    packagePairs.map(({ npmPackage }) => npmPackage),
  );

  return {
    packagePairs,
    cargoLockVersions,
    pnpmLockVersions,
    lockfileConflicts: collectLockfileConflicts(packagePairs, cargoLockVersions, pnpmLockVersions),
  };
}

function warnLockfileConflicts(lockfileConflicts) {
  if (lockfileConflicts.length === 0) {
    return;
  }

  console.warn("Warning: Cargo.lock and pnpm-lock.yaml resolve different Tauri major/minor versions.");
  for (const conflict of lockfileConflicts) {
    console.warn(
      `- ${conflict.rustPackage} ${conflict.cargoVersion} != ${conflict.npmPackage} ${conflict.npmVersion}`,
    );
  }
}

function writeCargoUpdates(nextCargoTomlText, cargoConstraintUpdates) {
  console.warn("Warning: Cargo Tauri plugin constraints must use tilde major.minor requirements.");
  fs.writeFileSync(config.paths.cargoToml, nextCargoTomlText, "utf8");
  console.log(`Synced ${cargoConstraintUpdates.length} Cargo plugin constraint(s):`);
  for (const update of cargoConstraintUpdates) {
    console.log(`- ${update.rustDepName}: ${update.from} -> ${update.to}`);
  }
}

function writePackageJsonUpdates(packageJson, packageJsonUpdates) {
  console.warn("Warning: npm Tauri package constraints must use tilde major.minor requirements.");
  fs.writeFileSync(config.paths.packageJson, `${JSON.stringify(packageJson, null, 2)}\n`, "utf8");
  console.log(`Updated ${packageJsonUpdates.length} npm package version(s):`);
  for (const update of packageJsonUpdates) {
    console.log(`- ${update.section}.${update.pkgName}: ${update.from} -> ${update.to}`);
  }
}

function updateLockfiles() {
  console.log("Updating pnpm lockfile...");
  runCommand("pnpm", ["install", "--lockfile-only"], config.paths.repoRoot);

  console.log("Updating Cargo.lock...");
  runCommand("cargo", ["generate-lockfile", "--manifest-path", config.paths.cargoToml], config.paths.repoRoot);
}

function main() {
  const { cargoTomlText, cargoToml, cargoLockToml, packageJson, pnpmLockYaml } = readProjectFiles();
  const rustPluginConstraints = collectRustPluginVersions(cargoToml);
  const lockfiles = inspectLockfiles(cargoLockToml, pnpmLockYaml, rustPluginConstraints);
  const cargoSync = syncCargoPluginConstraints(
    cargoTomlText,
    rustPluginConstraints,
    lockfiles.cargoLockVersions,
  );
  const packageJsonUpdates = applyVersionUpdates(
    packageJson,
    buildNpmPackageRanges(
      lockfiles.packagePairs,
      lockfiles.cargoLockVersions,
      lockfiles.pnpmLockVersions,
      cargoSync.finalConstraints,
    ),
  );

  const hasCargoUpdates = cargoSync.updates.length > 0;
  const hasPackageJsonUpdates = packageJsonUpdates.length > 0;
  const hasLockfileConflicts = lockfiles.lockfileConflicts.length > 0;

  if (!hasCargoUpdates && !hasPackageJsonUpdates && !hasLockfileConflicts) {
    console.log("No tauri plugin version conflicts found.");
    return;
  }

  warnLockfileConflicts(lockfiles.lockfileConflicts);

  if (hasCargoUpdates) {
    writeCargoUpdates(cargoSync.cargoTomlText, cargoSync.updates);
  }

  if (hasPackageJsonUpdates) {
    writePackageJsonUpdates(packageJson, packageJsonUpdates);
  }

  updateLockfiles();
}

main();
