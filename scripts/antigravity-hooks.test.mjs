#!/usr/bin/env node
//
// antigravity-hooks.test.mjs -- assert _antigravity-hook.sh translates Antigravity's
// hook protocol into our canonical .claude/scripts/*.sh contract without failing open.
//
// Antigravity (`agy` / T3 Code Antigravity harness) differs from Claude Code in four ways:
//   1. Runs hooks with `.agents/` as cwd instead of the repo root (via `sh -c` on Unix
//      and `cmd /c` on Windows).
//   2. Sends camelCase payloads (`conversationId`, top-level `cwd`, `toolCall.name`,
//      `toolCall.args.CommandLine` / `Path` / `TargetFile` / `AbsolutePath` / `Cwd`).
//   3. Treats ANY non-zero exit code (including `exit 2`) as a hook error and fails open;
//      blocking MUST exit 0 and emit `{"decision":"deny","reason":"..."}`.
//   4. Only reads `injectSteps` on `SessionStart` and `PreInvocation` (`systemMessage`),
//      so advisory context from `PreToolUse` / `PostToolUse` must be queued per
//      `conversationId` and drained once on the next `PreInvocation`.
//
// Run with: npm test

import { test, describe, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const REPO_SCRIPTS = join(dirname(fileURLToPath(import.meta.url)), "..", ".claude", "scripts");

// Assemble git commit strings dynamically so literal substrings never trip host hooks.
const GIT = "git";
const COMMIT = "commit";

let tmpBase;
let isolatedTmp;

before(() => {
  tmpBase = mkdtempSync(join(tmpdir(), "agy-hooks-test-"));
  isolatedTmp = join(tmpBase, "tmp");
  mkdirSync(isolatedTmp, { recursive: true });
});

after(() => {
  rmSync(tmpBase, { recursive: true, force: true });
});

function makeTestRepo() {
  const dir = mkdtempSync(join(tmpBase, "repo-"));
  const scriptsDir = join(dir, ".claude", "scripts");
  cpSync(REPO_SCRIPTS, scriptsDir, { recursive: true });
  writeFileSync(
    join(dir, ".claude", "settings.json"),
    JSON.stringify(
      {
        permissions: {
          deny: [
            "Bash(git clone ext::*)",
            "Bash(git clone --upload-pack*)",
            "Read(**/.env)",
            "Read(**/.env.*)",
            "Read(~/.ssh/**)",
            "Edit(**/package-lock.json)",
            "Edit(**/.git/**)",
          ],
        },
      },
      null,
      2,
    ),
  );
  const ensureStub = (name, body) => {
    const p = join(scriptsDir, name);
    if (!existsSync(p)) {
      writeFileSync(p, `#!/usr/bin/env bash\nset -euo pipefail\n${body}\n`);
      chmodSync(p, 0o755);
    }
  };
  ensureStub("session-start-kb-check.sh", 'echo "=== KNOWLEDGE BASE CHECK (BoardPandas/LL-G) ==="');
  ensureStub(
    "check-commit-herestring.sh",
    'in=$(cat); if [[ "$in" == *"@\x27"* ]]; then echo "Blocked PowerShell here-string" >&2; exit 2; fi',
  );
  ensureStub(
    "check-changelog-staged.sh",
    'cat >/dev/null; cd "$CLAUDE_PROJECT_DIR" && staged=$(git diff --cached --name-only); if [[ -n "$staged" && "$staged" != *"CHANGELOG.md"* ]]; then echo "Missing CHANGELOG.md" >&2; exit 2; fi',
  );
  ensureStub(
    "pre-write-kb-check.sh",
    'cat >/dev/null; printf \x27{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"LL-G GOTCHA CHECK"}}\x27',
  );
  ensureStub(
    "post-commit-kb-contribute.sh",
    'cat >/dev/null; printf \x27{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"KNOWLEDGE BASE CONTRIBUTION CHECK"}}\x27',
  );
  ensureStub(
    "post-edit-format.sh",
    'in=$(cat); f=$(node -e "const d=JSON.parse(process.argv[1]);process.stdout.write(d.tool_input?.file_path||\x27\x27)" "$in"); if [[ "$f" == *.sh ]] && command -v shfmt >/dev/null; then shfmt -w "$f"; fi',
  );
  mkdirSync(join(dir, ".agents"), { recursive: true });

  const git = (...args) => {
    const r = spawnSync("git", ["-c", "core.hooksPath=/dev/null", ...args], { cwd: dir, encoding: "utf8" });
    if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
  };
  git("init", "-q", ".");
  git("config", "user.email", "t@example.com");
  git("config", "user.name", "t");
  writeFileSync(join(dir, "package.json"), '{\n  "version": "1.0.0"\n}\n');
  writeFileSync(join(dir, "CHANGELOG.md"), "# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n- init\n");
  writeFileSync(join(dir, "app.js"), "export const x = 1;\n");
  git("add", "-A");
  git(COMMIT, "-qm", "initial");
  return dir;
}

function runAdapter(repoDir, event, scriptRel, payload, extraEnv = {}) {
  // Invoke via relative path from `.agents/`, matching `.agents/hooks.json`.
  const args = ["../.claude/scripts/_antigravity-hook.sh", event];
  if (scriptRel) args.push(scriptRel);
  const r = spawnSync("bash", args, {
    cwd: join(repoDir, ".agents"),
    input: typeof payload === "string" ? payload : JSON.stringify(payload),
    encoding: "utf8",
    env: { ...process.env, TMPDIR: isolatedTmp, ...extraEnv },
  });
  assert.equal(r.status, 0, `Antigravity adapter must ALWAYS exit 0 (got ${r.status}): ${r.stderr}`);
  let json;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    assert.fail(`Antigravity adapter must emit single-object JSON on stdout, got:\n${r.stdout}`);
  }
  return { json, stderr: r.stderr ?? "" };
}

describe("_antigravity-hook.sh", () => {
  test("SessionStart wraps session-start-kb-check.sh output as persistent systemMessage", () => {
    const repo = makeTestRepo();
    const { json } = runAdapter(repo, "SessionStart", ".claude/scripts/session-start-kb-check.sh", {
      conversationId: "conv-session-1",
    });
    const sysMsg = json?.injectSteps?.[0]?.systemMessage?.systemMessage;
    assert.equal(typeof sysMsg, "string");
    assert.match(sysMsg, /KNOWLEDGE BASE CHECK/);
    assert.match(sysMsg, /BoardPandas\/LL-G/);
  });

  test("PreToolUse converts blocking exit 2 (PowerShell herestring & missing changelog) into decision:deny with exit 0", () => {
    const repo = makeTestRepo();

    const herestring = runAdapter(repo, "PreToolUse", ".claude/scripts/check-commit-herestring.sh", {
      conversationId: "conv-block-1",
      cwd: repo,
      toolCall: {
        name: "run_command",
        args: {
          CommandLine: `${GIT} ${COMMIT} -m @'\nmsg\n'@`,
        },
      },
    });
    assert.equal(herestring.json.decision, "deny");
    assert.match(herestring.json.reason, /PowerShell here-string/i);

    writeFileSync(join(repo, "app.js"), "export const x = 2;\n");
    spawnSync("git", ["add", "app.js"], { cwd: repo });
    const unbumped = runAdapter(repo, "PreToolUse", ".claude/scripts/check-changelog-staged.sh", {
      conversationId: "conv-block-2",
      toolCall: {
        name: "run_command",
        args: {
          CommandLine: `${GIT} ${COMMIT} -m "feat: change x"`,
          Cwd: ".",
        },
      },
    });
    assert.equal(unbumped.json.decision, "deny");
    assert.match(unbumped.json.reason, /CHANGELOG\.md/);
  });

  test("PreToolUse honors top-level cwd when toolCall.args.Cwd is absent (CLI/ACP 1.5+)", () => {
    const sessionRepo = makeTestRepo();
    const otherRepo = makeTestRepo();
    writeFileSync(join(otherRepo, "app.js"), "export const x = 99;\n");
    spawnSync("git", ["add", "app.js"], { cwd: otherRepo });

    // Session repo is clean, but top-level `cwd` points at `otherRepo` with unbumped changes.
    const res = runAdapter(sessionRepo, "PreToolUse", ".claude/scripts/check-changelog-staged.sh", {
      conversationId: "conv-cwd-top",
      cwd: otherRepo,
      toolCall: {
        name: "run_command",
        args: { CommandLine: `${GIT} ${COMMIT} -m "fix: in other repo"` },
      },
    });
    assert.equal(res.json.decision, "deny");
    assert.match(res.json.reason, /CHANGELOG\.md/);
  });

  test("PreToolUse & PostToolUse queue advisory context for write_file (Path) and write_to_file (TargetFile), flushed once as systemMessage", () => {
    const repo = makeTestRepo();
    const convId = "conv-advisory-42";

    // Modern Antigravity CLI/ACP write_file with args.Path
    const pre = runAdapter(repo, "PreToolUse", ".claude/scripts/pre-write-kb-check.sh", {
      conversationId: convId,
      toolCall: {
        name: "write_file",
        args: { Path: join(repo, "src", "feature.ts") },
      },
    });
    assert.deepEqual(pre.json, { decision: "allow" });

    // PostToolUse on run_command git commit
    const post = runAdapter(repo, "PostToolUse", ".claude/scripts/post-commit-kb-contribute.sh", {
      conversationId: convId,
      cwd: repo,
      toolCall: {
        name: "run_command",
        args: { CommandLine: `${GIT} ${COMMIT} -m "fix: edge case"` },
      },
    });
    assert.deepEqual(post.json, {});

    // Another conversation gets nothing from convId's queue
    const otherTurn = runAdapter(repo, "PreInvocation", "", { conversationId: "conv-other" });
    assert.deepEqual(otherTurn.json, {});

    // Next PreInvocation for convId receives BOTH queued advisories via systemMessage (not deprecated ephemeralMessage)
    const flushed = runAdapter(repo, "PreInvocation", "", { conversationId: convId });
    assert.equal(flushed.json?.injectSteps?.[0]?.ephemeralMessage, undefined);
    const msg = flushed.json?.injectSteps?.[0]?.systemMessage?.systemMessage;
    assert.equal(typeof msg, "string");
    assert.match(msg, /LL-G GOTCHA CHECK/);
    assert.match(msg, /KNOWLEDGE BASE CONTRIBUTION CHECK/);

    // Subsequent PreInvocation is empty (queue drained atomically)
    const afterFlush = runAdapter(repo, "PreInvocation", "", { conversationId: convId });
    assert.deepEqual(afterFlush.json, {});
  });

  test("PostToolUse formats target in-place for BOTH modern edit_file (Path) and legacy replace_file_content (TargetFile)", () => {
    const repo = makeTestRepo();
    writeFileSync(join(repo, ".editorconfig"), "root = true\n");
    const fakeBin = join(repo, "fake-bin");
    mkdirSync(fakeBin, { recursive: true });
    const fakeShfmt = join(fakeBin, "shfmt");
    writeFileSync(fakeShfmt, '#!/usr/bin/env bash\n# Usage: shfmt -w <file>\nprintf "#!/usr/bin/env bash\\necho formatted\\n" > "$2"\n');
    chmodSync(fakeShfmt, 0o755);

    for (const [toolName, argsFor] of [
      ["edit_file", (p) => ({ Path: p })],
      ["replace_file_content", (p) => ({ TargetFile: p })],
    ]) {
      const target = join(repo, `script-${toolName}.sh`);
      writeFileSync(target, "#!/usr/bin/env bash\necho   unformatted\n");

      const post = runAdapter(
        repo,
        "PostToolUse",
        ".claude/scripts/post-edit-format.sh",
        {
          conversationId: `conv-fmt-${toolName}`,
          toolCall: { name: toolName, args: argsFor(target) },
        },
        { PATH: `${fakeBin}:${process.env.PATH ?? ""}` },
      );
      assert.deepEqual(post.json, {});
      assert.equal(readFileSync(target, "utf8"), "#!/usr/bin/env bash\necho formatted\n", `${toolName} should format`);
    }
  });

  test("PreToolUse --enforce-deny blocks denied reads, edits, and commands from .claude/settings.json and allows safe ones", () => {
    const repo = makeTestRepo();

    const deniedCases = [
      ["read_file", { Path: join(repo, ".env") }, /Read\(\*\*\/\.env\)/],
      ["view_file", { AbsolutePath: join(repo, "sub", ".env.local") }, /Read\(\*\*\/\.env\.\*\)/],
      ["read_file", { Path: "~/.ssh/id_rsa" }, /Read\(~\/\.ssh\/\*\*\)/],
      ["write_file", { Path: join(repo, ".git", "config") }, /Edit\(\*\*\/\.git\/\*\*\)/],
      ["edit_file", { Path: join(repo, "package-lock.json") }, /Edit\(\*\*\/package-lock\.json\)/],
      ["write_file", { Path: join(repo, ".env") }, /Read\(\*\*\/\.env\)/],
      ["run_command", { CommandLine: "git clone ext::sh -c id" }, /Bash\(git clone ext::\*\)/],
      ["run_command", { CommandLine: "cd /tmp && git clone --upload-pack=evil https://x" }, /Bash\(git clone --upload-pack\*\)/],
    ];
    for (const [name, args, expectRule] of deniedCases) {
      const res = runAdapter(repo, "PreToolUse", "--enforce-deny", {
        conversationId: "conv-deny",
        toolCall: { name, args },
      });
      assert.equal(res.json.decision, "deny", `${name} ${JSON.stringify(args)} should be denied`);
      assert.match(res.json.reason, expectRule);
    }

    const allowedCases = [
      ["read_file", { Path: join(repo, "app.js") }],
      ["write_file", { Path: join(repo, "app.js") }],
      ["edit_file", { Path: join(repo, "app.js") }],
      ["run_command", { CommandLine: "git status" }],
    ];
    for (const [name, args] of allowedCases) {
      const res = runAdapter(repo, "PreToolUse", "--enforce-deny", {
        conversationId: "conv-allow",
        toolCall: { name, args },
      });
      assert.deepEqual(res.json, { decision: "allow" }, `${name} ${JSON.stringify(args)} should be allowed`);
    }
  });

  test("conversationId is sanitized so path traversal never escapes agy-hook-queue", () => {
    const repo = makeTestRepo();
    const evilId = "../../outside-sentinel";
    runAdapter(repo, "PreToolUse", ".claude/scripts/pre-write-kb-check.sh", {
      conversationId: evilId,
      toolCall: {
        name: "write_file",
        args: { Path: join(repo, "index.js") },
      },
    });

    assert.equal(existsSync(join(tmpBase, "outside-sentinel.msg")), false);
    const queueDir = join(isolatedTmp, "agy-hook-queue");
    const files = existsSync(queueDir) ? readdirSync(queueDir) : [];
    assert.deepEqual(files, ["outside-sentinel.msg"]);
  });
});
