#!/usr/bin/env node
/**
 * Documentation drift guard.
 *
 * Docs/_toc.yaml maps source patterns to generated pages and
 * Docs/_meta/GENERATION.md records the source commit those pages were last
 * refreshed against. Every mapped source change after that baseline must be
 * followed by a change to its mapped page. Comparing commit ancestry, rather
 * than merely asking whether both files changed at some point, prevents an old
 * documentation commit from masking a newer source-only change. This is still
 * intentionally a co-change check: it catches silent drift, but it cannot
 * prove that the new prose is complete or correct.
 *
 * Node built-ins only. The CI checkout must include git history so the recorded
 * baseline commit is available.
 */
import { existsSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { posix } from "node:path";

const TOC_PATH = "Docs/_toc.yaml";
const GENERATION_PATH = "Docs/_meta/GENERATION.md";

const fail = (message) => {
	console.error(`docs-sync: ${message}`);
	process.exit(1);
};

const read = (path) => {
	try {
		return readFileSync(path, "utf8");
	} catch (error) {
		fail(`cannot read ${path}: ${error.message}`);
	}
};

const git = (args) => {
	try {
		return execFileSync("git", args, { encoding: "utf8" }).trim();
	} catch (error) {
		const detail = error.stderr?.toString().trim() || error.message;
		fail(`git ${args.join(" ")} failed: ${detail}`);
	}
};

const generation = read(GENERATION_PATH);
const ref = generation.match(/^- \*\*Commit:\*\* `([0-9a-f]{40})`/m)?.[1];
if (!ref) fail(`${GENERATION_PATH} has no 40-character Commit baseline.`);

git(["cat-file", "-e", `${ref}^{commit}`]);
try {
	execFileSync("git", ["merge-base", "--is-ancestor", ref, "HEAD"], {
		stdio: "ignore",
	});
} catch {
	fail(
		`recorded baseline ${ref.slice(0, 7)} is not an ancestor of HEAD; ` +
			"run a documentation sync after the rebase.",
	);
}

// The TOC schema is deliberately simple enough to parse without a YAML
// dependency: page records begin at two spaces, and source_files lists may
// occur at page or section depth. Capture every source pattern under its page.
const pages = [];
let page = null;
let sourceIndent = null;
for (const raw of read(TOC_PATH).split(/\r?\n/)) {
	const indent = raw.match(/^ */)[0].length;
	const text = raw.trim();

	const pageId = raw.match(/^  - id:\s+([A-Za-z0-9_-]+)\s*$/)?.[1];
	if (pageId) {
		page = { id: pageId, folder: "", filename: null, sources: [] };
		pages.push(page);
		sourceIndent = null;
		continue;
	}
	if (!page) continue;

	const folder = raw.match(/^    folder:\s+"([^"]*)"\s*$/)?.[1];
	if (folder !== undefined) page.folder = folder;
	const filename = raw.match(/^    filename:\s+"([^"]+)"\s*$/)?.[1];
	if (filename) page.filename = filename;

	if (/^source_files:\s*$/.test(text)) {
		sourceIndent = indent;
		continue;
	}
	if (sourceIndent !== null) {
		const source = raw.match(/^\s*-\s+"([^"]+)"\s*$/)?.[1];
		if (source && indent > sourceIndent) {
			page.sources.push(source.replaceAll("\\", "/"));
			continue;
		}
		if (text && indent <= sourceIndent) sourceIndent = null;
	}
}

if (pages.length === 0) fail(`${TOC_PATH} contains no page records.`);
for (const candidate of pages) {
	if (!candidate.filename) fail(`${candidate.id} has no filename.`);
	if (candidate.sources.length === 0) fail(`${candidate.id} has no source_files.`);
	candidate.doc = posix.join("Docs", candidate.folder, candidate.filename);
	if (!existsSync(candidate.doc)) fail(`${candidate.id} points to missing ${candidate.doc}.`);
	const marker = read(candidate.doc).match(/^<!-- PAGE_ID: ([^ ]+) -->/m)?.[1];
	if (marker !== candidate.id) {
		fail(`${candidate.doc} PAGE_ID is ${marker ?? "missing"}; expected ${candidate.id}.`);
	}
}

// Minimal, platform-neutral glob matcher for TOC patterns. Git paths and TOC
// paths are normalized to forward slashes before matching.
const globRegex = (pattern) => {
	let normalized = pattern.replaceAll("\\", "/");
	if (normalized.endsWith("/")) normalized += "**/*";
	let out = "^";
	for (let i = 0; i < normalized.length; i += 1) {
		const char = normalized[i];
		if (char === "*") {
			if (normalized[i + 1] === "*") {
				i += 1;
				if (normalized[i + 1] === "/") {
					i += 1;
					out += "(?:.*/)?";
				} else {
					out += ".*";
				}
			} else {
				out += "[^/]*";
			}
		} else if (char === "?") {
			out += "[^/]";
		} else {
			out += /[|\\{}()[\]^$+?.]/.test(char) ? `\\${char}` : char;
		}
	}
	return new RegExp(`${out}$`);
};

for (const candidate of pages) {
	candidate.matchers = candidate.sources.map(globRegex);
}

const committedChanges = new Set(
	git(["diff", "--name-only", "--no-renames", "--diff-filter=ACMRD", `${ref}..HEAD`, "--"])
		.split(/\r?\n/)
		.filter(Boolean)
		.map((path) => path.replaceAll("\\", "/")),
);
const worktreeChanges = new Set(
	git(["diff", "HEAD", "--name-only", "--no-renames", "--diff-filter=ACMRD", "--"])
		.split(/\r?\n/)
		.filter(Boolean)
		.map((path) => path.replaceAll("\\", "/")),
);
const untracked = git(["ls-files", "--others", "--exclude-standard"]);
for (const path of untracked.split(/\r?\n/).filter(Boolean)) {
	worktreeChanges.add(path.replaceAll("\\", "/"));
}

const commitsForPath = new Map();
const commitsAfterBaseline = (path) => {
	if (!commitsForPath.has(path)) {
		const output = git(["log", "--format=%H", `${ref}..HEAD`, "--", path]);
		commitsForPath.set(path, output.split(/\r?\n/).filter(Boolean));
	}
	return commitsForPath.get(path);
};
const isAncestor = (older, newer) => {
	try {
		execFileSync("git", ["merge-base", "--is-ancestor", older, newer], {
			stdio: "ignore",
		});
		return true;
	} catch {
		return false;
	}
};

const stale = [];
for (const candidate of pages) {
	const worktreeSources = [...worktreeChanges].filter((path) =>
		candidate.matchers.some((matcher) => matcher.test(path)),
	);
	if (worktreeSources.length > 0 && !worktreeChanges.has(candidate.doc)) {
		stale.push({ page: candidate.doc, sources: worktreeSources, reason: "working tree" });
		continue;
	}

	// A page being edited now covers committed drift too; content correctness is
	// reviewed separately. Otherwise every committed source change must be an
	// ancestor of a page commit after the recorded baseline.
	if (worktreeChanges.has(candidate.doc)) continue;
	const committedSources = [...committedChanges].filter((path) =>
		candidate.matchers.some((matcher) => matcher.test(path)),
	);
	if (committedSources.length === 0) continue;
	const pageCommits = commitsAfterBaseline(candidate.doc);
	const latestPage = pageCommits[0];
	const uncovered = committedSources.filter((path) => {
		const sourceCommits = commitsAfterBaseline(path);
		return !latestPage || sourceCommits.some((commit) => !isAncestor(commit, latestPage));
	});
	if (uncovered.length > 0) {
		stale.push({ page: candidate.doc, sources: uncovered, reason: "committed history" });
	}
}

if (stale.length > 0) {
	console.error(
		`docs-sync: ${stale.length} mapped page(s) did not change with their source since ${ref.slice(0, 7)}:`,
	);
	for (const item of stale) {
		console.error(`  ${item.page} (${item.reason})`);
		for (const source of item.sources) console.error(`    - ${source}`);
	}
	console.error(
		"Run the repo's doc-sync update workflow, then update Docs/_meta/GENERATION.md only after validation.",
	);
	process.exit(1);
}

console.log(
	`docs-sync OK: ${pages.length} mapped pages checked against ${ref.slice(0, 7)}; ` +
		`${committedChanges.size} committed and ${worktreeChanges.size} working-tree changed path(s), ` +
		"no silent mapped-source drift.",
);
