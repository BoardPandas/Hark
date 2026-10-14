#!/usr/bin/env node
//
// commit-hooks.test.mjs -- the git-commit hooks enforce the changelog contract
// against the repo the commit lands in, and their opt-outs and reminders work.
//
// The contract (.claude/rules/commit-changelog.md): CHANGELOG.md differs from
// HEAD, package.json's version differs from HEAD's, and CHANGELOG.md has a
// "## [<that version>]" section. Checked on STATE, never on the command's text.
//
// The hook process does not run where the command does. In a worktree session the
// harness starts it in the MAIN checkout (verified 2026-10-14: pwd and
// $CLAUDE_PROJECT_DIR were the main checkout, the payload's cwd was the worktree),
// so every case runs the hook the way the harness does: process cwd = the main
// checkout, payload JSON on stdin. Both directions are asserted, because the
// dangerous half is the silent one: "target not ready, hook's own repo ready ->
// still blocks". (LL-G kb/claude-code/hook-cwd-is-not-the-commit-target-repo.md)
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
const CHANGELOG = "# Changelog\n\n## [Unreleased]\n\n## [0.0.1] - 2026-01-01\n\n- First.\n";
const PKG = (v) => `{\n  "name": "fixture",\n  "version": "${v}"\n}\n`;

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

const initRepo = (dir) => {
  mkdirSync(dir);
  git(dir, "init", "-q", "-b", "main");
};

before(() => {
  // realpath: macOS's tmpdir is a symlink, and git reports the resolved path.
  root = realpathSync(mkdtempSync(join(tmpdir(), "commit-hooks-")));
  main = join(root, "main");
  // A space in the worktree path makes every quoted `cd "..."` exercise the
  // tokenizer, which is the part that has to keep the value intact.
  wt = join(root, "wt dir");

  initRepo(main);
  writeFileSync(join(main, "CHANGELOG.md"), CHANGELOG);
  writeFileSync(join(main, "package.json"), PKG("0.0.1"));
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

// Puts a repo in one of the contract's states, staged.
//   "ready"      -- new version section + matching package.json bump
//   "clean"      -- both files as in HEAD
//   "no-bump"    -- changelog edited, version unchanged
//   "no-section" -- version bumped, changelog edited but without that section
const setState = (repo, state) => {
  const log = {
    ready: CHANGELOG.replace("## [0.0.1]", "## [0.0.2] - 2026-10-14\n\n- Second.\n\n## [0.0.1]"),
    clean: CHANGELOG,
    "no-bump": `${CHANGELOG}- More.\n`,
    "no-section": `${CHANGELOG}- More.\n`,
  }[state];
  const version = state === "ready" || state === "no-section" ? "0.0.2" : "0.0.1";
  writeFileSync(join(repo, "CHANGELOG.md"), log);
  writeFileSync(join(repo, "package.json"), PKG(version));
  git(repo, "add", "CHANGELOG.md", "package.json");
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

// Advisory hooks must speak JSON additionalContext: plain stdout on a tool event
// goes to the debug log and never reaches the model.
// (LL-G kb/claude-code/advisory-tool-hook-plain-stdout-inert.md)
const contextOf = (r, event) => {
  assert.equal(r.status, 0, `advisory hooks never block\nstderr: ${r.stderr}`);
  if (r.stdout === "") return "";
  const out = JSON.parse(r.stdout);
  assert.equal(out.hookSpecificOutput.hookEventName, event);
  return out.hookSpecificOutput.additionalContext;
};

const assertAllows = (r, why) => assert.equal(r.status, 0, `${why}\nstderr: ${r.stderr}`);
const assertBlocks = (r, why, reason = /BLOCKED/) => {
  assert.equal(r.status, 2, `${why}\nstderr: ${r.stderr}`);
  assert.match(r.stderr, reason);
};

// ------------------------------------------------------------- which repo

test("worktree commit passes when the worktree is ready (payload cwd = worktree)", () => {
  setState(main, "clean");
  setState(wt, "ready");
  assertAllows(gate({ command: "git commit -m x", cwd: wt }), "the worktree satisfies the contract");

  const reminder = runHook("pre-commit-changelog-reminder.sh", { command: "git commit -m x", cwd: wt });
  assert.equal(contextOf(reminder, "PreToolUse"), "", "the reminder must agree with the blocker");
});

test("worktree commit is blocked when only the main checkout is ready", () => {
  setState(main, "ready");
  setState(wt, "clean");
  const r = gate({ command: "git commit -m x", cwd: wt });
  assertBlocks(r, "the main checkout's state must not satisfy a worktree commit");
  assert.ok(r.stderr.includes("wt dir"), `the block names the repo it checked\nstderr: ${r.stderr}`);
});

test("main-checkout commits are judged in the main checkout", () => {
  setState(wt, "ready");
  setState(main, "ready");
  assertAllows(gate({ command: "git commit -m x", cwd: main }), "main ready");
  setState(main, "clean");
  assertBlocks(gate({ command: "git commit -m x", cwd: main }), "main clean, worktree ready");
});

test("a payload with no cwd falls back to the hook process's directory", () => {
  setState(main, "clean");
  setState(wt, "ready");
  assertBlocks(gate({ command: "git commit -m x" }), "process cwd is main, which is clean");
  assertAllows(gate({ command: "git commit -m x", processCwd: wt }), "process cwd is the worktree");
});

test("`cd DIR &&` in the command retargets the check", () => {
  const command = `cd "${wt}" && git commit -m x`;
  setState(main, "clean");
  setState(wt, "ready");
  assertAllows(gate({ command, cwd: main }), "judged in the worktree");
  setState(main, "ready");
  setState(wt, "clean");
  assertBlocks(gate({ command, cwd: main }), "the session repo's state must not count");
});

test("`git -C DIR commit` retargets the check", () => {
  const command = `git -C "${wt}" commit -m x`;
  setState(main, "clean");
  setState(wt, "ready");
  assertAllows(gate({ command, cwd: main }), "judged in the worktree");
  setState(main, "ready");
  setState(wt, "clean");
  assertBlocks(gate({ command, cwd: main }), "the session repo's state must not count");
});

test("a relative cd resolves against the payload's cwd, then up to the repo root", () => {
  setState(main, "clean");
  setState(wt, "ready");
  assertAllows(gate({ command: "cd sub && git commit -m x", cwd: wt }), "sub is inside the worktree");
});

test("a subshell's cd does not leak, but a cd before it does", () => {
  setState(main, "clean");
  setState(wt, "ready");
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
  setState(main, "clean");
  assertAllows(gate({ command: 'cd "$(git rev-parse --show-toplevel)" && git commit -m x', cwd: main }), "");
});

// ------------------------------------------------------------ the contract

test("each part of the contract blocks on its own, naming what is missing", () => {
  setState(main, "clean");
  assertBlocks(gate({ command: "git commit -m x", cwd: main }), "clean", /CHANGELOG\.md is unchanged from HEAD/);
  setState(main, "no-bump");
  assertBlocks(gate({ command: "git commit -m x", cwd: main }), "no bump", /version in package\.json is unchanged/);
  setState(main, "no-section");
  assertBlocks(gate({ command: "git commit -m x", cwd: main }), "no section", /has no '## \[0\.0\.2\]' section/);
  setState(main, "ready");
  assertAllows(gate({ command: "git commit -m x", cwd: main }), "ready");
});

test("saying `git add CHANGELOG.md` in the command no longer counts", () => {
  // The old text exemption: an unmodified file stages nothing, so it waved
  // through exactly the commits it existed to stop.
  // (LL-G kb/claude-code/hook-validates-text-not-state.md)
  setState(main, "clean");
  assertBlocks(gate({ command: "git add CHANGELOG.md package.json && git commit -m x", cwd: main }), "no-op add");
});

test("SKIP_CHANGELOG=1 works as a prefix on the command, and the block message says so", () => {
  setState(main, "clean");
  const blocked = gate({ command: "git commit -m x", cwd: main });
  assertBlocks(blocked, "baseline");
  assert.ok(blocked.stderr.includes('SKIP_CHANGELOG=1 git commit -m "..."'), "the message shows the working form");

  // The hook never inherits the Bash tool's environment, so the opt-out has to
  // be read from the command text. (LL-G kb/claude-code/hook-does-not-inherit-command-env.md)
  assertAllows(gate({ command: "SKIP_CHANGELOG=1 git commit -m x", cwd: main }), "prefix");
  assertAllows(gate({ command: "export SKIP_CHANGELOG=1 && git commit -m x", cwd: main }), "export");
  assertBlocks(
    gate({ command: 'git commit -m "SKIP_CHANGELOG=1 someday"', cwd: main }),
    "a commit message that mentions the opt-out is not the opt-out",
  );
});

test("commits inside shell grouping and keywords are still commits", () => {
  setState(main, "clean");
  assertBlocks(gate({ command: "{ git commit -m x; }", cwd: main }), "brace group");
  assertBlocks(gate({ command: "if git commit -m x; then echo ok; fi", cwd: main }), "if");
  assertBlocks(gate({ command: "time git commit -m x", cwd: main }), "time");
});

test("--amend, a merge in progress, and the initial commit are exempt", () => {
  setState(main, "clean");
  assertAllows(gate({ command: "git commit --amend --no-edit", cwd: main }), "amend");

  const merging = join(root, "merging");
  initRepo(merging);
  writeFileSync(join(merging, "a.txt"), "a\n");
  git(merging, "add", ".");
  git(merging, "commit", "-q", "-m", "a");
  writeFileSync(join(merging, ".git", "MERGE_HEAD"), `${git(merging, "rev-parse", "HEAD").trim()}\n`);
  assertAllows(gate({ command: "git commit -m merge", cwd: merging }), "merge");

  const fresh = join(root, "fresh");
  initRepo(fresh);
  assertAllows(gate({ command: "git commit -m first", cwd: fresh }), "initial commit");
});

test("non-commit git commands are left alone", () => {
  setState(main, "clean");
  assertAllows(gate({ command: "git log --grep=commit", cwd: main }), "log");
  assertAllows(gate({ command: "git config --get commit.gpgsign", cwd: main }), "config");
});

// ---------------------------------------------------------- advisory hooks

test("the reminder reaches the model as additionalContext when the contract is unmet", () => {
  setState(main, "clean");
  const text = contextOf(
    runHook("pre-commit-changelog-reminder.sh", { command: "git commit -m x", cwd: main }),
    "PreToolUse",
  );
  assert.match(text, /CHANGELOG & VERSION UPDATE REQUIRED/);
  assert.match(text, /SKIP_CHANGELOG=1 git commit/);
});

test("the post-commit summary covers the commit, in the repo committed to", () => {
  const post = (cwd) =>
    contextOf(runHook("post-commit-kb-contribute.sh", { command: "git commit -m x", cwd, event: "PostToolUse" }), "PostToolUse");

  setState(wt, "clean");
  // Uncommitted edits must not be reported as committed: a bare `git diff HEAD~1`
  // compares against the working tree. (LL-G kb/claude-code/post-commit-diff-head1-includes-working-tree.md)
  writeFileSync(join(wt, "CHANGELOG.md"), `${CHANGELOG}- Uncommitted.\n`);
  const wtText = post(wt);
  assert.match(wtText, /feature: worktree only/);
  assert.match(wtText, /feature\.txt/);
  assert.doesNotMatch(wtText, /CHANGELOG\.md/, "an uncommitted edit is not part of the commit");
  setState(wt, "clean");

  // main has a single root commit: no HEAD~1, so the summary falls back to git show.
  const mainText = post(main);
  assert.match(mainText, /main: initial/);
  assert.match(mainText, /package\.json/);
});

test("the here-string block points at a message file that can exist in a worktree", () => {
  const r = runHook("check-commit-herestring.sh", { command: "git commit -F - @'\nmsg\n'@", cwd: wt });
  assertBlocks(r, "here-string syntax", /here-string/);
  assert.doesNotMatch(r.stderr, /\.git\/CLAUDE_COMMIT_MSG/, "in a worktree .git is a file");
});
