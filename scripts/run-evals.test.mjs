#!/usr/bin/env node
//
// run-evals.test.mjs -- assert the eval corpus validator still fires.
//
// Same premise as check-claude-wiring.test.mjs: a validator narrowed to match
// nothing prints "all structurally valid" forever. The corpus is the thing most
// likely to rot silently here -- cases accumulate, targets get renamed, and an
// expectation-free case passes every run it is in.
//
// Method: build a minimal corpus that PASSES --validate, then mutate one thing
// at a time and assert the runner exits 1 naming the defect.
//
// Only --validate is exercised. The behavioural pass needs the claude CLI and
// costs tokens, so it cannot run in a unit test; the guarantee here is that a
// broken corpus never reaches it.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, cpSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const RUNNER = join(dirname(fileURLToPath(import.meta.url)), "run-evals.mjs");

let base;

// MIN_CASES is 20, so the passing fixture needs at least that many. They are
// generated rather than written out: the point here is the validator, not the
// content, and 20 hand-written stubs would obscure the mutations under test.
const caseBody = (n) =>
  `---\nid: case-${n}\nkind: guard\nseverity: high\ntargets: [CLAUDE.md]\n---\n\n` +
  `## Task\n\nDo thing ${n}.\n\n## Expect\n\n- Thing ${n} happened.\n`;

before(() => {
  base = mkdtempSync(join(tmpdir(), "evals-base-"));
  mkdirSync(join(base, ".claude/evals/cases"), { recursive: true });
  writeFileSync(join(base, "CLAUDE.md"), "# Fixture\n");
  for (let n = 1; n <= 20; n++) {
    writeFileSync(join(base, `.claude/evals/cases/${String(n).padStart(2, "0")}-case.md`), caseBody(n));
  }
});

after(() => rmSync(base, { recursive: true, force: true }));

function run(mutate) {
  const dir = mkdtempSync(join(tmpdir(), "evals-case-"));
  try {
    cpSync(base, dir, { recursive: true });
    mutate({
      write: (p, s) => {
        mkdirSync(join(dir, dirname(p)), { recursive: true });
        writeFileSync(join(dir, p), s);
      },
      remove: (p) => rmSync(join(dir, p), { force: true, recursive: true }),
      firstCase: () =>
        join(".claude/evals/cases", readdirSync(join(dir, ".claude/evals/cases")).sort()[0]),
    });
    const r = spawnSync(process.execPath, [RUNNER, "--validate"], { cwd: dir, encoding: "utf8" });
    return { code: r.status, out: `${r.stdout}${r.stderr}` };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// A mutation that "passes" tells you nothing -- assert the baseline first.
test("baseline corpus validates", () => {
  const { code, out } = run(() => {});
  assert.equal(code, 0, `baseline should pass but exited ${code}:\n${out}`);
  assert.match(out, /24 cases|20 cases|\d+ cases, all structurally valid/);
});

const cases = [
  {
    name: "case with no frontmatter",
    mutate: ({ write, firstCase }) => write(firstCase(), "## Task\n\nx\n\n## Expect\n\n- y\n"),
    expect: /no YAML frontmatter block/,
  },
  {
    name: "case missing a required frontmatter key",
    mutate: ({ write, firstCase }) =>
      write(firstCase(), "---\nid: x\nkind: guard\n---\n\n## Task\n\nx\n\n## Expect\n\n- y\n"),
    expect: /missing required key "severity"/,
  },
  {
    name: "case with an unknown kind",
    mutate: ({ write, firstCase }) =>
      write(
        firstCase(),
        "---\nid: x\nkind: vibes\nseverity: high\n---\n\n## Task\n\nx\n\n## Expect\n\n- y\n",
      ),
    expect: /kind "vibes" is not one of/,
  },
  {
    // The reason this check exists: an expectation-free case passes every run.
    name: "case with no Expect bullets",
    mutate: ({ write, firstCase }) =>
      write(firstCase(), "---\nid: x\nkind: guard\nseverity: high\n---\n\n## Task\n\nx\n\n## Expect\n"),
    expect: /no "## Expect" bullets/,
  },
  {
    name: "case with no Task section",
    mutate: ({ write, firstCase }) =>
      write(firstCase(), "---\nid: x\nkind: guard\nseverity: high\n---\n\n## Expect\n\n- y\n"),
    expect: /no "## Task" section/,
  },
  {
    // The corpus equivalent of the wiring guard's dead-glob check.
    name: "case targeting a file that does not exist",
    mutate: ({ write, firstCase }) =>
      write(
        firstCase(),
        "---\nid: x\nkind: guard\nseverity: high\ntargets: [does/not/exist.md]\n---\n\n" +
          "## Task\n\nx\n\n## Expect\n\n- y\n",
      ),
    expect: /targets "does\/not\/exist\.md", which does not exist/,
  },
  {
    name: "two cases sharing an id",
    mutate: ({ write }) => write(".claude/evals/cases/99-dupe.md", caseBody(1)),
    expect: /duplicate id "case-1"/,
  },
  {
    // A suite that shrinks below the floor stops being a regression net, and
    // that shrinkage is otherwise invisible.
    name: "corpus shrunk below the case floor",
    mutate: ({ remove }) => {
      for (let n = 1; n <= 5; n++) remove(`.claude/evals/cases/${String(n).padStart(2, "0")}-case.md`);
    },
    expect: /below the 20-case floor/,
  },
  {
    name: "corpus directory missing entirely",
    mutate: ({ remove }) => remove(".claude/evals/cases"),
    expect: /does not exist/,
  },
];

for (const c of cases) {
  test(`fires: ${c.name}`, () => {
    const { code, out } = run(c.mutate);
    assert.equal(code, 1, `validator should have failed but exited ${code}:\n${out}`);
    assert.match(out, c.expect, `validator failed, but not for the expected reason:\n${out}`);
  });
}

// The behavioural pass must fail, not skip, when it cannot run. A skip here is
// indistinguishable from a pass, which is the failure mode the suite exists to
// prevent -- so this is asserted rather than assumed.
test("a missing claude CLI is a failure, not a skip", () => {
  const dir = mkdtempSync(join(tmpdir(), "evals-nocli-"));
  try {
    cpSync(base, dir, { recursive: true });
    // Empty PATH entry: `claude` cannot resolve, so the probe must fail hard.
    const r = spawnSync(process.execPath, [RUNNER], {
      cwd: dir,
      encoding: "utf8",
      env: { ...process.env, PATH: join(dir, "no-such-bin") },
    });
    assert.equal(r.status, 1, `expected a hard failure, got ${r.status}`);
    assert.match(`${r.stdout}${r.stderr}`, /is not available|FAIL/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
