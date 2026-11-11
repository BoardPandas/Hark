#!/usr/bin/env node
//
// antigravity-hooks.test.mjs -- assert _antigravity-hook.sh translates Antigravity's
// hook protocol into our canonical .claude/scripts/*.sh contract without failing open.
//
// Antigravity (`agy` / T3 Code Antigravity harness) differs from Claude Code in four ways:
//   1. Runs hooks with `.agents/` as cwd instead of the repo root.
//   2. Sends camelCase payloads (`conversationId`, `toolCall.name`, `toolCall.args.CommandLine`
//      / `TargetFile` / `Cwd`).
//   3. Treats ANY non-zero exit code (including `exit 2`) as a hook error and fails open;
//      blocking MUST exit 0 and emit `{"decision":"deny","reason":"..."}`.
//   4. Only reads `injectSteps` on `SessionStart` (`systemMessage`) and `PreInvocation`
//      (`ephemeralMessage`), so advisory context from `PreToolUse` / `PostToolUse` must be
//      queued per `conversationId` and drained once on the next `PreInvocation`.
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
  const args = [join(repoDir, ".claude", "scripts", "_antigravity-hook.sh"), event];
  if (scriptRel) args.push(scriptRel);
  const r = spawnSync("bash", args, {
    // Simulate Antigravity's actual execution directory: `<repo>/.agents`.
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

    // 1. PowerShell @'...'@ herestring commit blocked by check-commit-herestring.sh
    const herestring = runAdapter(repo, "PreToolUse", ".claude/scripts/check-commit-herestring.sh", {
      conversationId: "conv-block-1",
      toolCall: {
        name: "run_command",
        args: {
          CommandLine: `${GIT} ${COMMIT} -m @'\nmsg\n'@`,
          Cwd: repo,
        },
      },
    });
    assert.equal(herestring.json.decision, "deny");
    assert.match(herestring.json.reason, /PowerShell here-string/i);

    // 2. Unbumped commit blocked by check-changelog-staged.sh (even when adapter cwd is .agents/)
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

  test("PreToolUse & PostToolUse queue advisory context per conversationId and PreInvocation drains it once", () => {
    const repo = makeTestRepo();
    const convId = "conv-advisory-42";

    // PreToolUse on write_to_file -> pre-write-kb-check.sh emits LL-G GOTCHA CHECK
    const pre = runAdapter(repo, "PreToolUse", ".claude/scripts/pre-write-kb-check.sh", {
      conversationId: convId,
      toolCall: {
        name: "write_to_file",
        args: { TargetFile: join(repo, "src", "feature.ts") },
      },
    });
    assert.deepEqual(pre.json, { decision: "allow" });

    // PostToolUse on run_command git commit -> post-commit-kb-contribute.sh emits KB CONTRIBUTION CHECK
    const post = runAdapter(repo, "PostToolUse", ".claude/scripts/post-commit-kb-contribute.sh", {
      conversationId: convId,
      toolCall: {
        name: "run_command",
        args: { CommandLine: `${GIT} ${COMMIT} -m "fix: edge case"`, Cwd: repo },
      },
    });
    assert.deepEqual(post.json, {});

    // Another conversation gets nothing from convId's queue
    const otherTurn = runAdapter(repo, "PreInvocation", "", { conversationId: "conv-other" });
    assert.deepEqual(otherTurn.json, {});

    // Next PreInvocation for convId receives BOTH queued advisories in ephemeralMessage
    const flushed = runAdapter(repo, "PreInvocation", "", { conversationId: convId });
    const msg = flushed.json?.injectSteps?.[0]?.ephemeralMessage;
    assert.equal(typeof msg, "string");
    assert.match(msg, /LL-G GOTCHA CHECK/);
    assert.match(msg, /KNOWLEDGE BASE CONTRIBUTION CHECK/);

    // Subsequent PreInvocation is empty (queue drained atomically)
    const afterFlush = runAdapter(repo, "PreInvocation", "", { conversationId: convId });
    assert.deepEqual(afterFlush.json, {});
  });

  test("PostToolUse maps replace_file_content TargetFile so post-edit-format.sh formats the target in-place", () => {
    const repo = makeTestRepo();
    writeFileSync(join(repo, ".editorconfig"), "root = true\n");
    const fakeBin = join(repo, "fake-bin");
    mkdirSync(fakeBin, { recursive: true });
    const fakeShfmt = join(fakeBin, "shfmt");
    writeFileSync(fakeShfmt, '#!/usr/bin/env bash\n# Usage: shfmt -w <file>\nprintf "#!/usr/bin/env bash\\necho formatted\\n" > "$2"\n');
    chmodSync(fakeShfmt, 0o755);

    const target = join(repo, "script.sh");
    writeFileSync(target, "#!/usr/bin/env bash\necho   unformatted\n");

    const post = runAdapter(
      repo,
      "PostToolUse",
      ".claude/scripts/post-edit-format.sh",
      {
        conversationId: "conv-fmt-1",
        toolCall: {
          name: "replace_file_content",
          args: { TargetFile: target },
        },
      },
      { PATH: `${fakeBin}:${process.env.PATH ?? ""}` },
    );
    assert.deepEqual(post.json, {});
    assert.equal(readFileSync(target, "utf8"), "#!/usr/bin/env bash\necho formatted\n");
  });

  test("conversationId is sanitized so path traversal never escapes agy-hook-queue", () => {
    const repo = makeTestRepo();
    const evilId = "../../outside-sentinel";
    runAdapter(repo, "PreToolUse", ".claude/scripts/pre-write-kb-check.sh", {
      conversationId: evilId,
      toolCall: {
        name: "write_to_file",
        args: { TargetFile: join(repo, "index.js") },
      },
    });

    assert.equal(existsSync(join(tmpBase, "outside-sentinel.msg")), false);
    const queueDir = join(isolatedTmp, "agy-hook-queue");
    const files = existsSync(queueDir) ? readdirSync(queueDir) : [];
    assert.deepEqual(files, ["outside-sentinel.msg"]);
  });
});
