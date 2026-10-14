#!/usr/bin/env node
//
// commit-hooks.test.mjs -- the git-commit hooks judge the repo the commit lands in.
//
// The hook process does not run where the command does. In a worktree session the
// harness starts it in the MAIN checkout (verified 2026-10-14: pwd and
// $CLAUDE_PROJECT_DIR were the main checkout, the payload's cwd was the worktree),
// so check-changelog-staged.sh read the main checkout's index and refused a
// worktree commit whose CHANGELOG.md was staged.
//
// Every case runs the hook the way the harness does: process cwd = the main
// checkout, payload JSON on stdin. Both directions are asserted, because the
// dangerous half is the silent one: "target clean, hook's own repo staged ->
// still blocks". A suite that checks only the annoying half passes against a hook
// that still reads its own cwd whenever the two trees happen to agree.
// (LL-G kb/claude-code/hook-cwd-is-not-the-commit-target-repo.md)
//
// Paths are native to the platform, so on Windows the payload's cwd and every
// `cd "..."` carry backslashes, as the real harness sends them.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawnSync, execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, existsSync, realpathSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const SCRIPTS = resolve(dirname(fileURLToPath(import.meta.url)), "../.claude/scripts");
const CHANGELOG = "# Changelog\n\n## [Unreleased]\n";

// A fixture git must not inherit a parent repo's GIT_DIR (e.g. when run from a
// git hook), and the hooks must not inherit an opt-out from the caller's shell.
const env = Object.fromEntries(
  Object.entries(process.env).filter(([k]) => !k.startsWith("GIT_") && k !== "SKIP_CHANGELOG"),
);

// On Windows, `bash` on PATH can be WSL's launcher, which cannot see C:\ paths the
// way the harness's Git Bash does. Take the bash that ships with git itself.
const findBash = () => {
  if (process.platform !== "win32") return "bash";
  const exec = execFileSync("git", ["--exec-path"], { encoding: "utf8", env }).trim();
  const root = resolve(exec, "../../..");
  for (const candidate of [join(root, "bin", "bash.exe"), join(root, "usr", "bin", "bash.exe")]) {
    if (existsSync(candidate)) return candidate;
  }
  return "bash";
};
const BASH = findBash();

const git = (cwd, ...args) =>
  execFileSync(
    "git",
    ["-c", "user.name=Hook Test", "-c", "user.email=hook@test.invalid", "-c", "commit.gpgsign=false", ...args],
    { cwd, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
  );

let root, main, wt;

before(() => {
  // realpath: macOS's tmpdir is a symlink, and git reports the resolved path.
  root = realpathSync(mkdtempSync(join(tmpdir(), "commit-hooks-")));
  main = join(root, "main");
  // A space in the worktree path makes every quoted `cd "..."` exercise the
  // tokenizer, which is the part that has to keep the value intact.
  wt = join(root, "wt dir");

  mkdirSync(main);
  git(main, "init", "-q", "-b", "main");
  writeFileSync(join(main, "CHANGELOG.md"), CHANGELOG);
  writeFileSync(join(main, "package.json"), '{ "version": "0.0.1" }\n');
  git(main, "add", ".");
  git(main, "commit", "-q", "-m", "main: initial");

  git(main, "worktree", "add", "-q", wt, "-b", "feature");
  writeFileSync(join(wt, "feature.txt"), "x\n");
  git(wt, "add", "feature.txt");
  git(wt, "commit", "-q", "-m", "feature: worktree only");
  mkdirSync(join(wt, "sub"));
});

after(() => {
  if (root) rmSync(root, { recursive: true, force: true });
});

// Leaves exactly one state: CHANGELOG.md staged with an edit, or clean.
const setStaged = (repo, staged) => {
  if (staged) {
    writeFileSync(join(repo, "CHANGELOG.md"), `${CHANGELOG}\n- an entry\n`);
    git(repo, "add", "CHANGELOG.md");
  } else {
    git(repo, "reset", "-q", "--", "CHANGELOG.md");
    writeFileSync(join(repo, "CHANGELOG.md"), CHANGELOG);
  }
};

// Runs a hook as the harness does. `cwd: undefined` omits the field, as an older
// harness or a direct invocation would.
const runHook = (script, { command, cwd, processCwd = main, event = "PreToolUse" }) => {
  const payload = { hook_event_name: event, tool_name: "Bash", tool_input: { command } };
  if (cwd !== undefined) payload.cwd = cwd;
  const r = spawnSync(BASH, [join(SCRIPTS, script).replace(/\\/g, "/")], {
    cwd: processCwd,
    env,
    input: JSON.stringify(payload),
    encoding: "utf8",
  });
  if (r.error) throw r.error;
  return r;
};

const gate = (opts) => runHook("check-changelog-staged.sh", opts);

const assertAllows = (r, why) => assert.equal(r.status, 0, `${why}\nstderr: ${r.stderr}`);
const assertBlocks = (r, why) => {
  assert.equal(r.status, 2, `${why}\nstderr: ${r.stderr}`);
  assert.match(r.stderr, /CHANGELOG\.md is not staged/);
};

test("worktree commit with CHANGELOG.md staged passes (payload cwd = worktree)", () => {
  setStaged(main, false);
  setStaged(wt, true);
  assertAllows(gate({ command: "git commit -m x", cwd: wt }), "the worktree's own index has it staged");

  const reminder = runHook("pre-commit-changelog-reminder.sh", { command: "git commit -m x", cwd: wt });
  assert.equal(reminder.status, 0);
  assert.equal(reminder.stdout, "", "the reminder must agree with the blocker");
});

test("worktree commit without CHANGELOG.md is blocked even when the main checkout has it staged", () => {
  setStaged(main, true);
  setStaged(wt, false);
  const r = gate({ command: "git commit -m x", cwd: wt });
  assertBlocks(r, "the main checkout's index must not satisfy a worktree commit");
  assert.ok(r.stderr.includes("wt dir"), `the block names the repo it checked\nstderr: ${r.stderr}`);
});

test("main-checkout commits behave as before", () => {
  setStaged(wt, true);
  setStaged(main, true);
  assertAllows(gate({ command: "git commit -m x", cwd: main }), "main staged");
  setStaged(main, false);
  assertBlocks(gate({ command: "git commit -m x", cwd: main }), "main clean, worktree staged");
});

test("a payload with no cwd falls back to the hook process's directory", () => {
  setStaged(main, false);
  setStaged(wt, true);
  assertBlocks(gate({ command: "git commit -m x" }), "process cwd is main, which is clean");
  assertAllows(gate({ command: "git commit -m x", processCwd: wt }), "process cwd is the worktree");
});

test("`cd DIR &&` in the command retargets the check", () => {
  const command = `cd "${wt}" && git commit -m x`;
  setStaged(main, false);
  setStaged(wt, true);
  assertAllows(gate({ command, cwd: main }), "judged in the worktree");
  setStaged(main, true);
  setStaged(wt, false);
  assertBlocks(gate({ command, cwd: main }), "the session repo's staged file must not count");
});

test("`git -C DIR commit` retargets the check", () => {
  const command = `git -C "${wt}" commit -m x`;
  setStaged(main, false);
  setStaged(wt, true);
  assertAllows(gate({ command, cwd: main }), "judged in the worktree");
  setStaged(main, true);
  setStaged(wt, false);
  assertBlocks(gate({ command, cwd: main }), "the session repo's staged file must not count");
});

test("a relative cd resolves against the payload's cwd, then up to the repo root", () => {
  setStaged(main, false);
  setStaged(wt, true);
  assertAllows(gate({ command: "cd sub && git commit -m x", cwd: wt }), "sub is inside the worktree");
});

test("a subshell's cd does not leak, but a cd before it does", () => {
  setStaged(main, false);
  setStaged(wt, true);
  assertBlocks(
    gate({ command: `(cd "${wt}" && git status) && git commit -m x`, cwd: main }),
    "the commit runs back in main",
  );
  assertAllows(
    gate({ command: `cd "${wt}"; (cd sub && git status); git commit -m x`, cwd: main }),
    "the outer cd survives the subshell",
  );
});

test("a target only the shell could compute is not guessed at (allows)", () => {
  // Documented fail-open: a gate that refuses commits in repos it cannot see gets
  // bypassed wholesale. See commit_target_dir in _git-commit-filter.sh.
  setStaged(main, false);
  assertAllows(gate({ command: 'cd "$(git rev-parse --show-toplevel)" && git commit -m x', cwd: main }), "");
});

test("the post-commit summary describes the repo that was committed to", () => {
  const post = (cwd) =>
    runHook("post-commit-kb-contribute.sh", { command: "git commit -m x", cwd, event: "PostToolUse" }).stdout;
  assert.match(post(wt), /feature: worktree only/);
  assert.match(post(main), /main: initial/);
});
