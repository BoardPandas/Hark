#!/usr/bin/env node
/**
 * Claude wiring guard: fails when the .claude/ setup is silently non-functional.
 *
 * Every check here corresponds to a defect that shipped in this repo and stayed
 * invisible for months, because all four failure modes are SILENT: the rule loads
 * anyway, or the hook simply never runs, and nothing errors.
 *
 *   1. Cursor .mdc frontmatter keys      -> scoping inverted (alwaysApply:false loads ALWAYS)
 *   2. paths: globs matching nothing     -> rule never fires
 *   3. Tool(pattern) hook matchers       -> hook never fires (that is permissions syntax)
 *   4. always-on context weight          -> every rule competes with every other rule
 *   5. 2>/dev/null || true on a hook     -> failure is unfalsifiable
 *
 * Portable: node built-ins only, no dependencies, no repo-specific paths. Drop into
 * any Claude Code repo and wire as `node scripts/check-claude-wiring.mjs` in CI.
 *
 * Exit 0 = pass (warnings allowed). Exit 1 = at least one error.
 *
 * NOTE ON DURABILITY: checks 1 and 3 encode Claude Code's frontmatter and hook-matcher
 * contract as observed 2026-07-28. If that contract changes, this guard is what will
 * tell you, but update it deliberately rather than deleting the failing assertion.
 */
import { globSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const CLAUDE_DIR = ".claude";
const RULES_DIR = join(CLAUDE_DIR, "rules");
const SETTINGS = [join(CLAUDE_DIR, "settings.json"), join(CLAUDE_DIR, "settings.local.json")];
const ROOT_MD = "CLAUDE.md";

// Rough budget for text injected into EVERY session before the user types. Not a hard
// contract; it exists so growth is a deliberate decision rather than a slow accretion.
const ALWAYS_ON_TOKEN_WARN = 12_000;

const errors = [];
const warnings = [];
const notes = [];

const exists = (p) => {
	try {
		statSync(p);
		return true;
	} catch {
		return false;
	}
};
const read = (p) => {
	try {
		return readFileSync(p, "utf8");
	} catch {
		return null;
	}
};
const approxTokens = (bytes) => Math.round(bytes / 4);

if (!exists(CLAUDE_DIR)) {
	console.log("claude-wiring: no .claude/ directory; nothing to check.");
	process.exit(0);
}

/* ── 1 + 2: rule frontmatter and glob liveness ────────────────────────────── */

const ruleFiles = exists(RULES_DIR)
	? readdirSync(RULES_DIR)
			.filter((f) => f.endsWith(".md"))
			.map((f) => join(RULES_DIR, f))
	: [];

const alwaysOnRules = [];

for (const file of ruleFiles) {
	const text = read(file);
	if (text === null) continue;

	let front = null;
	if (text.startsWith("---")) {
		const end = text.indexOf("\n---", 3);
		if (end !== -1) front = text.slice(3, end);
	}

	if (front && /^\s*(globs|alwaysApply)\s*:/m.test(front)) {
		errors.push(
			`${file}: uses Cursor .mdc keys (globs:/alwaysApply:). Claude Code ignores them, so ` +
				`this file loads in EVERY session -- the opposite of alwaysApply:false. Use paths:.`,
		);
		alwaysOnRules.push(file);
		continue;
	}

	// Line-based, not a regex. An earlier version used /^paths:\s*$([\s\S]*?)(?=^\S|\Z)/m
	// and silently matched nothing, because \Z is not a JavaScript anchor -- it is a
	// literal "Z". The guard then reported every rule as unscoped. Exactly the class of
	// silent-wrong-answer bug this script exists to catch, so: no clever regex here.
	const frontLines = front ? front.split("\n") : [];
	const pathsIdx = frontLines.findIndex((l) => /^paths\s*:/.test(l));
	if (pathsIdx === -1) {
		// No paths: key at all -> unconditional load. Legitimate for genuinely global
		// rules, so this is a note, not an error. It still counts toward the budget.
		alwaysOnRules.push(file);
		continue;
	}

	const globs = [];
	for (const line of frontLines.slice(pathsIdx + 1)) {
		const t = line.trim();
		if (t.startsWith("- ")) {
			globs.push(t.slice(2).trim().replace(/^["']|["']$/g, ""));
		} else if (t !== "" && !t.startsWith("#")) {
			break; // next top-level key ends the list
		}
	}

	if (globs.length === 0) {
		errors.push(`${file}: has a paths: key with no entries, so it never loads.`);
		continue;
	}

	for (const g of globs) {
		let hits = 0;
		try {
			hits = globSync(g, { exclude: (p) => p.includes("node_modules") }).length;
		} catch {
			warnings.push(`${file}: glob ${JSON.stringify(g)} could not be evaluated.`);
			continue;
		}
		if (hits === 0) {
			errors.push(
				`${file}: glob ${JSON.stringify(g)} matches 0 files, so that path never triggers ` +
					`the rule. Fix the glob or drop it.`,
			);
		}
	}
}

/* ── 3 + 5: hook matchers and silenced hooks ──────────────────────────────── */

const TOOL_MATCHER = /^[A-Za-z][A-Za-z0-9_]*(\|[A-Za-z][A-Za-z0-9_]*)*$/;

for (const settingsPath of SETTINGS) {
	const raw = read(settingsPath);
	if (raw === null) continue;
	let cfg;
	try {
		cfg = JSON.parse(raw);
	} catch (err) {
		errors.push(`${settingsPath}: invalid JSON (${err.message}).`);
		continue;
	}

	for (const [event, blocks] of Object.entries(cfg.hooks ?? {})) {
		for (const block of blocks ?? []) {
			const m = block.matcher;
			if (m !== undefined && !TOOL_MATCHER.test(m)) {
				errors.push(
					`${settingsPath}: ${event} matcher ${JSON.stringify(m)} is not a tool name. ` +
						`Hook matchers match the TOOL NAME (e.g. "Edit", "Write|Edit", "Bash"); ` +
						`Tool(pattern) is permissions syntax and matches nothing, so the hook never runs. ` +
						`Filter on the command inside the script instead.`,
				);
			}
			for (const h of block.hooks ?? []) {
				const cmd = h.command ?? "";
				if (/2>\s*\/dev\/null/.test(cmd) && /\|\|\s*true/.test(cmd)) {
					warnings.push(
						`${settingsPath}: ${event} hook silences both stderr and exit code ` +
							`(${cmd.slice(0, 48)}...). If it breaks you will never find out. ` +
							`Let it fail loudly, or log to a file.`,
					);
				}
			}
		}
	}
}

/* ── 4: always-on context weight ──────────────────────────────────────────── */

let alwaysOnBytes = 0;
const budget = [];

if (exists(ROOT_MD)) {
	const text = read(ROOT_MD) ?? "";
	alwaysOnBytes += Buffer.byteLength(text);
	budget.push([ROOT_MD, Buffer.byteLength(text)]);

	for (const line of text.split("\n")) {
		const m = line.match(/^@(\S+)/);
		if (!m) continue;
		const target = m[1];
		if (!exists(target)) {
			errors.push(`${ROOT_MD}: @import target ${target} does not exist.`);
			continue;
		}
		const size = Buffer.byteLength(read(target) ?? "");
		alwaysOnBytes += size;
		budget.push([`@${target}`, size]);
	}
}

for (const f of alwaysOnRules) {
	const size = Buffer.byteLength(read(f) ?? "");
	alwaysOnBytes += size;
	budget.push([f, size]);
}

const alwaysOnTokens = approxTokens(alwaysOnBytes);
if (alwaysOnTokens > ALWAYS_ON_TOKEN_WARN) {
	budget.sort((a, b) => b[1] - a[1]);
	warnings.push(
		`always-on context is ~${alwaysOnTokens} tokens (budget ${ALWAYS_ON_TOKEN_WARN}). ` +
			`Every unscoped file competes with every other for attention. Largest: ` +
			budget
				.slice(0, 3)
				.map(([f, b]) => `${f} (~${approxTokens(b)})`)
				.join(", "),
	);
}
notes.push(`always-on: ~${alwaysOnTokens} tokens across ${budget.length} file(s)`);
notes.push(`rules: ${ruleFiles.length} total, ${alwaysOnRules.length} load in every session`);

/* ── report ───────────────────────────────────────────────────────────────── */

for (const n of notes) console.log(`claude-wiring: ${n}`);
for (const w of warnings) console.warn(`claude-wiring WARN: ${w}`);

if (errors.length === 0) {
	console.log("claude-wiring OK: rule scoping and hook matchers are functional.");
	process.exit(0);
}

console.error(`\nclaude-wiring FAILED: ${errors.length} error(s):`);
for (const e of errors) console.error(`  - ${e}`);
process.exit(1);
