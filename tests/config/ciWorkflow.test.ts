import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import picomatch from "picomatch";
import { describe, expect, it } from "vitest";
import { parse } from "yaml";

interface Step {
  id?: string;
  name: string;
  run: string;
  if?: string;
  env?: Record<string, string>;
  with: Record<string, string>;
}

const root = path.resolve(__dirname, "../..");
const workflow = parse(
  readFileSync(path.join(root, ".github/workflows/ci.yml"), "utf8"),
) as {
  on: { pull_request: { types?: string[] } };
  jobs: {
    changes: { outputs: Record<string, string>; steps: Step[] };
    backend: { if: string; steps: Step[] };
    "frontend-contracts": { if: string; steps: Step[] };
  };
};
const filterStep = workflow.jobs.changes.steps.find((s) => s.id === "filter")!;
const filters = parse(filterStep.with.filters) as Record<string, string[]>;
const scopeStep = workflow.jobs.changes.steps.find((s) => s.id === "scope")!;
const metadataStep = workflow.jobs.changes.steps.find(
  (s) => s.id === "pr_meta",
)!;
const commandStep = workflow.jobs.backend.steps.find((s) => s.id === "scope")!;

// Exercise the workflow's scripts and the same matcher as dorny/paths-filter.
// No copied scope implementation: these cases also cover job/step gate wiring.
function executeStep(step: Step, env: Record<string, string>, prefix = "") {
  const dir = mkdtempSync(path.join(tmpdir(), "cc-switch-ci-scope-"));
  const output = path.join(dir, "outputs");
  try {
    execFileSync("bash", ["-e", "-c", `${prefix}\n${step.run}`], {
      cwd: root,
      env: { ...process.env, ...env, GITHUB_OUTPUT: output },
      stdio: "pipe",
    });
    return Object.fromEntries(
      readFileSync(output, "utf8")
        .trim()
        .split("\n")
        .map((line) => {
          const separator = line.indexOf("=");
          return [line.slice(0, separator), line.slice(separator + 1)];
        }),
    ) as Record<string, string>;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function runStep(step: Step, env: Record<string, string>) {
  return Object.fromEntries(
    Object.entries(executeStep(step, env)).map(([key, value]) => [
      key,
      value === "true",
    ]),
  );
}

function metadataFor(patches: string, env: Record<string, string> = {}) {
  return executeStep(
    metadataStep,
    {
      GH_REPO: "example/repo",
      PR_NUMBER: "1",
      BACKEND_CHANGED: "true",
      MACOS_FULL_FILTER: "false",
      MOCK_LABELS: '["full-ci"]',
      MOCK_PATCHES: patches,
      MOCK_LABEL_STATUS: "0",
      MOCK_PATCH_STATUS: "0",
      ...env,
    },
    `gh() {
      case "$*" in
        *"/files"*) printf '%s\\n' "$MOCK_PATCHES"; return "$MOCK_PATCH_STATUS" ;;
        *) printf '%s\\n' "$MOCK_LABELS"; return "$MOCK_LABEL_STATUS" ;;
      esac
    }`,
  );
}

function scopeFor(
  files: string[],
  labels: string[] = [],
  event = "pull_request",
  patchTouchesMacos = false,
) {
  const changed = ["pull_request", "push"].includes(event)
    ? Object.fromEntries(
        Object.entries(filters).map(([area, patterns]) => [
          area,
          String(files.some(picomatch(patterns, { dot: true }))),
        ]),
      )
    : {};
  return runStep(scopeStep, {
    EVENT_NAME: event,
    CHANGED_AREAS: JSON.stringify(changed),
    PR_LABELS: JSON.stringify(labels),
    PATCH_TOUCHES_MACOS: String(patchTouchesMacos),
  });
}

function commandsFor(scope: Record<string, boolean>, os: string) {
  return runStep(commandStep, {
    RUNNER_OS: os,
    MACOS_FULL: String(scope.macos_full),
  });
}

const ordinaryBackend = ["src-tauri/src/proxy/thinking_optimizer.rs"];
const fullScope = {
  backend: true,
  macos_full: true,
  wsl: true,
};

describe("CI backend scope", () => {
  it("uses live labels before path gating without adding labeler-triggered runs", () => {
    expect(metadataStep.if).toBe("github.event_name == 'pull_request'");
    expect(scopeStep.env?.PR_LABELS).toBe(
      "${{ steps.pr_meta.outputs.labels || '[]' }}",
    );
    expect(workflow.on.pull_request.types ?? []).not.toContain("labeled");
    const metadata = metadataFor("", {
      BACKEND_CHANGED: "false",
      MOCK_PATCH_STATUS: "1",
    });
    expect(scopeFor(["src/App.tsx"], JSON.parse(metadata.labels))).toEqual(
      fullScope,
    );
  });

  it("keeps ordinary backend PRs on the lighter platform checks", () => {
    const scope = scopeFor(ordinaryBackend);
    expect(scope).toEqual({
      backend: true,
      macos_full: false,
      wsl: false,
    });
    expect(commandsFor(scope, "Linux")).toEqual({
      run_clippy: true,
      run_tests: true,
      run_check_only: false,
    });
    expect(commandsFor(scope, "Windows")).toEqual({
      run_clippy: true,
      run_tests: true,
      run_check_only: false,
    });
    expect(commandsFor(scope, "macOS")).toEqual({
      run_clippy: false,
      run_tests: false,
      run_check_only: true,
    });
  });

  it.each([
    "src-tauri/src/lib.rs",
    "src-tauri/src/claude_desktop_config.rs",
    "src-tauri/src/services/dsh_account.rs",
    "src-tauri/tests/integration/profile_roundtrip.rs",
    "src-tauri/.config/nextest.toml",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
    "src-tauri/build.rs",
    "rust-toolchain.toml",
    ".github/workflows/ci.yml",
  ])("upgrades platform checks for %s", (file) => {
    const scope = scopeFor([file]);
    expect(scope.backend).toBe(true);
    for (const os of ["macOS", "Windows"]) {
      expect(commandsFor(scope, os), os).toEqual({
        run_clippy: true,
        run_tests: true,
        run_check_only: false,
      });
    }
  });

  it.each([ordinaryBackend, ["src/App.tsx"], ["README.md"]])(
    "full-ci overrides path filtering for %j, including WSL",
    (...files) => {
      expect(scopeFor(files, ["full-ci"])).toEqual(fullScope);
    },
  );

  it.each([
    ["test:macos", true, false],
    ["test:windows", false, true],
  ] as const)(
    "%s can enable backend checks on a frontend-only PR",
    (label, macos, wsl) => {
      expect(scopeFor(["src/App.tsx"], [label])).toEqual({
        backend: true,
        macos_full: macos,
        wsl,
      });
    },
  );

  it("returns to path filtering after removing a label", () => {
    expect(scopeFor(["src/App.tsx"], ["full-ci"])).toEqual(fullScope);
    expect(scopeFor(["src/App.tsx"])).toEqual({
      backend: false,
      macos_full: false,
      wsl: false,
    });
    expect(scopeFor(ordinaryBackend, ["bug,full-ci"]).macos_full).toBe(false);
  });

  it.each(["push", "schedule", "workflow_dispatch"])(
    "%s runs all backend platform checks",
    (event) => {
      expect(scopeFor(ordinaryBackend, [], event)).toEqual(fullScope);
    },
  );

  it("still skips backend on a main push with no backend changes", () => {
    expect(scopeFor(["README.md"], [], "push").backend).toBe(false);
  });

  it("uses the resolved scope for the backend job and every WSL step", () => {
    for (const area of Object.keys(fullScope)) {
      expect(workflow.jobs.changes.outputs[area]).toBe(
        `\${{ steps.scope.outputs.${area} }}`,
      );
    }
    expect(workflow.jobs.backend.if).toBe(
      "needs.changes.outputs.backend == 'true'",
    );
    const wslSteps = workflow.jobs.backend.steps.filter((s) =>
      s.name.includes("WSL2"),
    );
    expect(wslSteps).toHaveLength(4);
    for (const step of wslSteps) {
      expect(step.if, step.name).toBe(
        "runner.os == 'Windows' && needs.changes.outputs.wsl == 'true'",
      );
    }
  });
});

describe("live PR metadata and macOS diff guard", () => {
  it("upgrades new macOS code outside the static path list", () => {
    const metadata = metadataFor('+#[cfg(target_os = "macos")]');
    expect(metadata.patch_touches_macos).toBe("true");
    const scope = scopeFor(
      ordinaryBackend,
      [],
      "pull_request",
      metadata.patch_touches_macos === "true",
    );
    expect(commandsFor(scope, "macOS").run_tests).toBe(true);
  });

  it("consumes a large diff without a grep SIGPIPE false negative", () => {
    expect(
      metadataFor(
        `+use objc2::rc::Retained;\n${"+ordinary change\n".repeat(6000)}`,
      ).patch_touches_macos,
    ).toBe("true");
  });

  it("keeps unrelated backend diffs on the lightweight macOS check", () => {
    expect(metadataFor("+ordinary change").patch_touches_macos).toBe("false");
  });

  it("upgrades when GitHub cannot provide a Rust patch", () => {
    expect(
      metadataFor("CC_SWITCH_MISSING_RUST_PATCH").patch_touches_macos,
    ).toBe("true");
  });

  it.each(["MOCK_LABEL_STATUS", "MOCK_PATCH_STATUS"])(
    "fails instead of silently downgrading when %s is nonzero",
    (key) => {
      expect(() => metadataFor("", { [key]: "1" })).toThrow();
    },
  );

  it("does not fetch a diff when static paths already require full macOS checks", () => {
    expect(
      metadataFor("", { MACOS_FULL_FILTER: "true", MOCK_PATCH_STATUS: "1" })
        .patch_touches_macos,
    ).toBe("false");
  });
});

function rustFiles(dir: string): string[] {
  return readdirSync(path.join(root, dir), { withFileTypes: true }).flatMap(
    (entry) => {
      const file = `${dir}/${entry.name}`;
      return entry.isDirectory()
        ? rustFiles(file)
        : file.endsWith(".rs")
          ? [file]
          : [];
    },
  );
}

describe("CI platform path coverage", () => {
  const sources = [
    ...rustFiles("src-tauri/src"),
    ...rustFiles("src-tauri/tests"),
  ].map((file) => ({
    file,
    source: readFileSync(path.join(root, file), "utf8"),
  }));

  it("runs the coverage check for Rust-only PRs, including new modules", () => {
    const matches = picomatch(filters.frontend_contracts, { dot: true });
    expect(matches("src-tauri/src/new_platform_module.rs")).toBe(true);
    expect(sources.filter(({ file }) => !matches(file))).toEqual([]);
    const job = workflow.jobs["frontend-contracts"];
    expect(job.if).toBe(
      "needs.changes.outputs.frontend_contracts == 'true' && needs.changes.outputs.frontend != 'true'",
    );
    expect(
      job.steps.some((step) =>
        step.run?.includes("tests/config/ciWorkflow.test.ts"),
      ),
    ).toBe(true);
  });

  it("covers existing macOS code even when a diff does not include its cfg", () => {
    const matches = picomatch(filters.macos_full, { dot: true });
    const platform =
      /target_os\s*=\s*"macos"|target_vendor\s*=\s*"apple"|std::os::macos/;
    const uncovered = sources
      .filter(({ source }) => platform.test(source))
      .map(({ file }) => file)
      .filter((file) => !matches(file));
    expect(uncovered).toEqual([]);
  });
});
