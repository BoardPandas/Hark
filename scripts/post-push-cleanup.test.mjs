#!/usr/bin/env node
//
// post-push-cleanup.test.mjs -- the post-push hook lists exactly what is safe to
// offer for removal, and says nothing when the default branch did not just move.
//
// Every fixture is a throwaway clone of a throwaway bare remote, so the pushes are
// real and the reflog entries the hook reads are the ones git writes.
//
// Run with: npm test

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "..", ".claude", "scripts", "post-push-cleanup.sh");
const PUSH = "git push origin main";

function git(cwd, args, env = {}) {
  const r = spawnSync("git", ["-c", "core.hooksPath=/dev/null", ...args], {
    cwd,
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
  return r.stdout.trim();
}

function runHook(payload, cwd) {
  const r = spawnSync("bash", [SCRIPT], { cwd, input: JSON.stringify(payload), encoding: "utf8" });
  return { status: r.status, stdout: r.stdout ?? "", stderr: r.stderr ?? "" };
}

function context(stdout) {
  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    assert.fail(`stdout is not a single JSON object, so Claude never sees it:\n${stdout}`);
  }
  assert.equal(parsed.hookSpecificOutput?.hookEventName, "PostToolUse");
  return parsed.hookSpecificOutput.additionalContext;
}

function commitOn(cwd, file) {
  writeFileSync(join(cwd, file), `${file}\n`);
  git(cwd, ["add", file]);
  git(cwd, ["commit", "-qm", file]);
}

// A clone whose main has had these merged, then pushed:
//   done-wt     worktree, no uncommitted changes      -> offered (worktree + branch)
//   dirty-wt    worktree with an untracked file       -> left alone, reported
//   locked-wt   locked worktree                       -> left alone, silent
//   done-plain  plain branch                          -> offered
//   fresh       created at main's tip, no commits     -> offered, flagged as maybe new
// and one branch that was never merged:
//   open-work   unmerged                              -> never mentioned
function makeFixture({ merge = true } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "post-push-")));
  const remote = join(root, "remote.git");
  const work = join(root, "work");
  git(root, ["init", "-q", "--bare", "-b", "main", remote]);
  git(root, ["clone", "-q", remote, work]);
  git(work, ["config", "user.email", "t@example.com"]);
  git(work, ["config", "user.name", "t"]);
  git(work, ["checkout", "-q", "-b", "main"]);
  commitOn(work, "init.txt");
  git(work, ["push", "-q", "origin", "main"]);
  git(work, ["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);

  const wt = (name) => join(root, `wt-${name}`);
  for (const name of ["done-wt", "dirty-wt", "locked-wt"]) {
    git(work, ["worktree", "add", "-q", "-b", name, wt(name)]);
    commitOn(wt(name), `${name}.txt`);
  }
  git(work, ["checkout", "-q", "-b", "done-plain"]);
  commitOn(work, "done-plain.txt");
  git(work, ["checkout", "-q", "-b", "open-work", "main"]);
  commitOn(work, "open-work.txt");
  git(work, ["checkout", "-q", "main"]);

  if (merge) {
    for (const name of ["done-wt", "dirty-wt", "locked-wt", "done-plain"]) {
      git(work, ["merge", "-q", "--no-ff", "-m", `merge ${name}`, name]);
    }
  }
  git(work, ["branch", "fresh"]);
  writeFileSync(join(wt("dirty-wt"), "notes.txt"), "unsaved\n");
  git(work, ["worktree", "lock", wt("locked-wt")]);
  git(work, ["push", "-q", "origin", "main"]);

  return { root, work, wt, cleanup: () => rmSync(root, { recursive: true, force: true }) };
}

describe("post-push-cleanup.sh (PostToolUse)", () => {
  test("lists merged branches and clean worktrees, and leaves the rest alone", () => {
    const f = makeFixture();
    try {
      const r = runHook({ tool_input: { command: PUSH }, cwd: f.work }, f.work);
      assert.equal(r.status, 0);
      const msg = context(r.stdout);

      assert.match(msg, /Nothing has been deleted/);
      assert.ok(msg.includes(`"${f.wt("done-wt")}"  [branch done-wt]`), msg);
      assert.ok(msg.includes(`git -C "${f.work}" worktree remove "${f.wt("done-wt")}"`), msg);
      for (const name of ["done-wt", "done-plain", "fresh"]) {
        assert.ok(msg.includes(`git -C "${f.work}" branch -d ${name}\n`), `missing branch -d ${name}:\n${msg}`);
      }
      assert.match(msg, /fresh {2}\(points at origin\/main itself/);

      // Uncommitted work is reported but never offered for removal.
      assert.match(msg, /Left alone[^\n]*\n {2}"[^"]*wt-dirty-wt" {2}\[branch dirty-wt\]/);
      assert.ok(!msg.includes(`worktree remove "${f.wt("dirty-wt")}"`), msg);
      assert.ok(!msg.includes("branch -d dirty-wt"), msg);

      // Locked and unmerged work is not mentioned at all.
      assert.ok(!msg.includes("locked-wt"), msg);
      assert.ok(!msg.includes("open-work"), msg);

      // Worktrees are removed and pruned before any branch is deleted.
      assert.ok(msg.indexOf("worktree prune") < msg.indexOf("branch -d"), msg);
      assert.match(msg, /Never add --force or -D/);
    } finally {
      f.cleanup();
    }
  });

  test("judges the repo the command targets, not the session's cwd", () => {
    const f = makeFixture();
    try {
      for (const command of [`git -C "${f.work}" push origin HEAD:main`, `cd "${f.work}" && git push`]) {
        const r = runHook({ tool_input: { command }, cwd: f.root }, f.root);
        assert.ok(context(r.stdout).includes("branch -d done-plain"), command);
      }
    } finally {
      f.cleanup();
    }
  });

  test("never offers the worktree the session itself is working in", () => {
    const f = makeFixture();
    try {
      const here = f.wt("done-wt");
      const r = runHook({ tool_input: { command: `git -C "${f.work}" push origin main` }, cwd: here }, here);
      const msg = context(r.stdout);
      assert.ok(!msg.includes(`worktree remove "${here}"`), msg);
      assert.ok(!msg.includes("branch -d done-wt"), msg);
      assert.ok(msg.includes("branch -d done-plain"), msg);
    } finally {
      f.cleanup();
    }
  });

  test("tells a subagent to report the list rather than act on it", () => {
    const f = makeFixture();
    try {
      const r = runHook({ tool_input: { command: PUSH }, cwd: f.work, agent_id: "a1", agent_type: "builder" }, f.work);
      assert.match(context(r.stdout), /inside a subagent\. Delete nothing/);
    } finally {
      f.cleanup();
    }
  });

  test("stays silent for commands that are not a push", () => {
    const f = makeFixture();
    try {
      for (const command of ["git status", "git log --grep=push", `echo "${PUSH}"`, `grep -r '${PUSH}' docs/`]) {
        const r = runHook({ tool_input: { command }, cwd: f.work }, f.work);
        assert.equal(r.status, 0);
        assert.equal(r.stdout, "", command);
      }
      // Only the tool OUTPUT mentions a push.
      const r = runHook({ tool_input: { command: "cat notes.md" }, tool_response: { stdout: PUSH }, cwd: f.work }, f.work);
      assert.equal(r.stdout, "");
    } finally {
      f.cleanup();
    }
  });

  test("stays silent when origin/main was not pushed just now", () => {
    const f = makeFixture();
    try {
      // The newest origin/main reflog entry is a push from 2001.
      const env = { GIT_COMMITTER_DATE: "@1000000000 +0000" };
      git(f.work, ["update-ref", "-m", "update by push", "refs/remotes/origin/main", "HEAD~1"], env);
      git(f.work, ["update-ref", "-m", "update by push", "refs/remotes/origin/main", "HEAD"], env);
      const r = runHook({ tool_input: { command: "git push origin done-plain" }, cwd: f.work }, f.work);
      assert.equal(r.status, 0);
      assert.equal(r.stdout, "");
    } finally {
      f.cleanup();
    }
  });

  test("stays silent when the push left nothing merged to clean up", () => {
    const f = makeFixture({ merge: false });
    try {
      // `fresh` sits at main's tip, so remove it to leave only unmerged work.
      git(f.work, ["branch", "-q", "-D", "fresh"]);
      const r = runHook({ tool_input: { command: PUSH }, cwd: f.work }, f.work);
      assert.equal(r.status, 0);
      assert.equal(r.stdout, "");
    } finally {
      f.cleanup();
    }
  });

  test("stays silent outside a git repository", () => {
    const dir = realpathSync(mkdtempSync(join(tmpdir(), "post-push-none-")));
    try {
      const r = runHook({ tool_input: { command: PUSH }, cwd: dir }, dir);
      assert.equal(r.status, 0);
      assert.equal(r.stdout, "");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
