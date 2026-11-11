#!/usr/bin/env node
// sync-antigravity.mjs -- standalone CLI and re-export surface for Antigravity (.agents/) sync.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  AGY_DOC_CEILING,
  AGY_MODEL_MAP,
  AGY_SKILL_CEILING,
  AGY_TOOL_MAP,
  generateAntigravityAgents,
  generateAntigravityHooks,
  generateAntigravityReadme,
  generateAntigravityRules,
  mapAntigravityAgentTools,
  splitOversizedSkill,
} from "./antigravity-format.mjs";

export {
  AGY_DOC_CEILING,
  AGY_MODEL_MAP,
  AGY_SKILL_CEILING,
  AGY_TOOL_MAP,
  generateAntigravityAgents,
  generateAntigravityHooks,
  generateAntigravityReadme,
  generateAntigravityRules,
  mapAntigravityAgentTools,
  splitOversizedSkill,
};

const CLAUDE_SKILL_VAR = `$${"{CLAUDE_SKILL_DIR}"}`;
const code = (s) => `\`${s}\``;

// ------------------------------------------------ standalone CLI entrypoint

function runStandaloneCli() {
  const root = process.cwd();
  const check = process.argv.includes("--check");
  const withSkills = process.argv.includes("--with-skills");
  const generator = `scripts/sync-antigravity.mjs${withSkills ? " --with-skills" : ""}`;
  const syncCmd = `node ${generator}`;
  const eol = (s) => s.replace(/\r\n/g, "\n");
  const posix = (p) => p.split("\\").join("/");
  const read = (p) => eol(readFileSync(join(root, p), "utf8"));
  const unq = (v) => (/^".*"$/.test(v) ? JSON.parse(v) : /^'.*'$/.test(v) ? v.slice(1, -1).replace(/''/g, "'") : v);
  const asList = (v) => (Array.isArray(v) ? v : typeof v === "string" && v ? v.split(",").map((s) => s.trim()).filter(Boolean) : []);

  function listFiles(dir) {
    try {
      const out = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", dir], {
        cwd: root,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      });
      return out.split("\0").filter(Boolean).map(posix).filter((p) => existsSync(join(root, p))).sort();
    } catch {
      const walk = (d) =>
        existsSync(join(root, d))
          ? readdirSync(join(root, d)).flatMap((n) => {
              const relP = posix(relative(root, join(root, d, n)));
              return statSync(join(root, relP)).isDirectory() ? walk(relP) : [relP];
            })
          : [];
      return walk(dir).sort();
    }
  }

  function parseFm(text, file, optional = false) {
    const src = eol(text);
    const m = src.match(/^---\n([\s\S]*?)\n---(?:\n|$)/);
    if (!m) {
      if (optional) return { data: {}, body: src };
      throw new Error(`${file}: missing YAML frontmatter`);
    }
    const lines = m[1].split("\n");
    const data = {};
    for (let i = 0; i < lines.length; i++) {
      const kv = lines[i].match(/^([A-Za-z][\w-]*):(?:\s+(.*))?$/);
      if (!kv) continue;
      const val = (kv[2] ?? "").trim();
      if (!val) {
        const items = [];
        while (i + 1 < lines.length && /^\s+-\s+/.test(lines[i + 1])) items.push(unq(lines[++i].replace(/^\s+-\s+/, "").trim()));
        data[kv[1]] = items.length ? items : "";
      } else if (/^[>|][-+]?$/.test(val)) {
        const parts = [];
        while (i + 1 < lines.length && /^\s+\S/.test(lines[i + 1])) parts.push(lines[++i].trim());
        data[kv[1]] = val.startsWith(">") ? parts.join(" ") : parts.join("\n");
      } else if (/^\[.*\]$/.test(val)) {
        data[kv[1]] = val.slice(1, -1).split(",").map((s) => unq(s.trim())).filter(Boolean);
      } else {
        let sc = val;
        while (i + 1 < lines.length && /^\s+\S/.test(lines[i + 1]) && !/^\s+-\s/.test(lines[i + 1])) sc += ` ${lines[++i].trim()}`;
        data[kv[1]] = unq(sc);
      }
    }
    return { data, body: src.slice(m[0].length) };
  }

  const failures = [];
  const expected = new Map();
  const emit = (p, c) => expected.set(p, Buffer.isBuffer(c) ? c : Buffer.from(c, "utf8"));
  if (!existsSync(join(root, ".claude/settings.json"))) {
    console.error("FAIL: .claude/settings.json missing");
    process.exit(1);
  }
  const settings = JSON.parse(read(".claude/settings.json"));
  const agentSources = listFiles(".claude/agents")
    .filter((p) => /^\.claude\/agents\/[^/]+\.md$/.test(p))
    .map((file) => {
      const { data, body } = parseFm(read(file), file);
      const tools = data.tools === undefined ? null : asList(data.tools);
      return { file, fileName: file.split("/").pop().replace(/\.md$/, ""), data, body, tools };
    });
  generateAntigravityAgents({ agentSources, emit, failures, generator, syncCmd });

  const rules = listFiles(".claude/rules")
    .filter((p) => /^\.claude\/rules\/[^/]+\.md$/.test(p))
    .map((p) => {
      const { data, body } = parseFm(read(p), p, true);
      return { path: p, description: data.description, paths: asList(data.paths), body };
    });
  const agentsMdRaw = existsSync(join(root, "AGENTS.md")) ? read("AGENTS.md") : "";
  const agentsMdFits =
    agentsMdRaw.slice(0, 200).includes("GENERATED by scripts/sync-codex") &&
    Buffer.byteLength(agentsMdRaw, "utf8") <= AGY_SKILL_CEILING;
  const ruleCount = generateAntigravityRules({ rules, emit, generator, syncCmd, includeAlwaysOn: !agentsMdFits });

  const { mirrored, agyNotCarried } = generateAntigravityHooks({
    settings,
    bellRe: /^(printf|echo(\s+-e)?)\s+(['"])\\a\3$/,
    gitRoot: "$(git rev-parse --show-toplevel)",
    scriptOf: (c) => c.match(/(?:^|[\s"/])(\.claude\/[\w./-]+\.(?:sh|mjs|js|cjs|py))/)?.[1] ?? null,
    emit,
    failures,
  });

  let skillCount = listFiles(".agents/skills").filter((p) => /^\.agents\/skills\/[^/]+\/SKILL\.md$/.test(p)).length;
  if (withSkills) {
    const files = listFiles(".claude/skills");
    const names = [...new Set(files.filter((p) => /^\.claude\/skills\/[^/]+\/SKILL\.md$/.test(p)).map((p) => p.split("/")[2]))];
    skillCount = names.length;
    for (const name of names) {
      const src = `.claude/skills/${name}`;
      const tgt = `.agents/skills/${name}`;
      const { data, body } = parseFm(read(`${src}/SKILL.md`), `${src}/SKILL.md`);
      const rawDesc = String(data.description ?? "").replace(/[<>]/g, "");
      const explicitOnly = String(data["disable-model-invocation"]) === "true";
      const desc = explicitOnly ? `Explicit command only (/${name} or $${name}) -- never invoke implicitly. ${rawDesc}` : rawDesc;
      const notes = [`Generated from ${code(`${src}/SKILL.md`)} by ${code(generator)}. Edit the source, then run ${code(syncCmd)}.`];
      if (explicitOnly) notes.push(`Explicit only: runs when the user invokes ${code(`$${name}`)} or ${code(`/${name}`)}, never implicitly.`);
      if (data.agent) notes.push(`In Antigravity, call ${code("invoke_subagent")} with ${code(`TypeName: "${data.agent}"`)} (${code(`.agents/agents/${data.agent}.md`)}) and return its summary.`);
      if (body.includes("$ARGUMENTS")) notes.push(`${code("$ARGUMENTS")} is the text the user supplied with the skill invocation.`);
      const expand = (s) => s.replaceAll(CLAUDE_SKILL_VAR, tgt);
      const prefix = `---\nname: ${name}\ndescription: ${JSON.stringify(desc.slice(0, 1024))}\n---\n\n${notes.map((n) => `> ${n}`).join("\n>\n")}\n\n`;
      const { skillMd, overflow } = splitOversizedSkill(tgt, prefix, expand(body));
      emit(`${tgt}/SKILL.md`, skillMd);
      if (overflow) emit(overflow.path, overflow.content);
      for (const f of files.filter((p) => p.startsWith(`${src}/`) && p !== `${src}/SKILL.md`)) {
        const buf = readFileSync(join(root, f));
        emit(`${tgt}/${f.slice(src.length + 1)}`, buf.includes(0) ? buf : expand(eol(buf.toString("utf8"))));
      }
      if (explicitOnly) {
        emit(`${tgt}/agents/openai.yaml`, `# GENERATED by ${generator}\npolicy:\n  allow_implicit_invocation: false\n`);
      }
    }
  }

  generateAntigravityReadme({
    summary: { skills: skillCount, agents: agentSources.length, rules: ruleCount, agyHooks: mirrored },
    agyNotCarried,
    emit,
    generator,
    syncCmd,
  });

  if (failures.length) {
    for (const f of failures) console.error(`FAIL: ${f}`);
    process.exit(1);
  }

  const owned = [".agents/agents", ".agents/rules", ...(withSkills ? [".agents/skills"] : [])];
  let written = 0;
  let removed = 0;
  const same = (a, b) => (a.includes(0) || b.includes(0) ? a.equals(b) : eol(a.toString("utf8")) === eol(b.toString("utf8")));
  for (const [p, content] of expected) {
    const abs = join(root, p);
    const cur = existsSync(abs) ? readFileSync(abs) : null;
    if (cur && same(cur, content)) continue;
    if (check) failures.push(`${p}: ${cur ? "stale" : "missing"}`);
    else {
      mkdirSync(dirname(abs), { recursive: true });
      writeFileSync(abs, content);
      written++;
    }
  }
  for (const d of owned) {
    for (const p of listFiles(d)) {
      if (expected.has(p)) continue;
      if (check) failures.push(`${p}: unexpected (no Claude source generates it)`);
      else {
        rmSync(join(root, p));
        removed++;
        let curD = dirname(join(root, p));
        const stop = join(root, d);
        while (curD.startsWith(stop) && curD !== stop && readdirSync(curD).length === 0) {
          rmdirSync(curD);
          curD = dirname(curD);
        }
      }
    }
  }
  if (failures.length) {
    for (const f of failures) console.error(`FAIL: ${f}`);
    process.exit(1);
  }
  const msg = `${skillCount} skills, ${agentSources.length} agents, ${ruleCount} rules, ${mirrored} Antigravity hooks`;
  console.log(check ? `OK -- Antigravity mirror current (${msg}).` : `Antigravity mirror regenerated (${msg}): ${written} written, ${removed} removed.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  runStandaloneCli();
}
