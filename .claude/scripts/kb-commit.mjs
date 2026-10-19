#!/usr/bin/env node
/**
 * kb-commit.mjs -- write SEVERAL files to a GitHub repo as ONE commit, via the git
 * data API (blobs -> tree -> commit -> ref). No local clone required.
 *
 * Replaces repeated kb-upsert.sh calls for knowledge-base writes, for two reasons.
 *
 * 1. ATOMICITY. A KB entry is three files that are only correct together: the entry,
 *    its shelf llms.txt, and the master index count. Pushed one at a time, the repo
 *    passes through two genuinely broken states -- after push 1 the entry is an
 *    orphan no index lists, after push 2 the master count disagrees with the shelf --
 *    and both now fail CI. On 2026-09-10 that produced three red builds in LL-G
 *    before the fourth went green. One commit, one CI run, no broken intermediate.
 *
 * 2. CORRECT COMPARE-AND-SWAP. kb-upsert.sh re-reads each blob's SHA immediately
 *    before its PUT "so the value is fresh", which inverts the guard: a freshly-read
 *    SHA always matches HEAD, so the write always succeeds and silently overwrites
 *    anything that landed while you were editing. Two sessions appending to one
 *    llms.txt lost a shelf line that way, with 200 OK on both sides. See LL-G
 *    kb/git/github-contents-sha-refresh-defeats-cas.md.
 *
 *    So --base is REQUIRED here: pass the commit SHA your edits were based on, read
 *    BEFORE you started editing. The new commit is parented on it and the ref is
 *    updated with force=false, so if anyone pushed in between GitHub refuses the
 *    fast-forward and you are told to re-read and re-apply. A write that cannot
 *    prove what it was based on is refused rather than warned about, because a
 *    warning on stderr in a session already moving on is not a guard.
 *
 * Usage:
 *   kb-commit.mjs --repo <owner/name> --base <commit-sha> --message <msg>
 *                 [--branch main] [--dry-run]
 *                 <repo-path> <local-file> [<repo-path> <local-file> ...]
 *
 *   --base      commit SHA the edits are based on (40 hex chars). Get it with:
 *                 gh api repos/<repo>/commits/heads/<branch> --jq .sha
 *   --dry-run   print what would be sent and exit 0 without writing anything.
 *
 * Requires: gh (authenticated). Node built-ins only.
 */
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";

const SHA40 = /^[0-9a-f]{40}$/;

function die(msg, code = 1) {
	console.error(`kb-commit: ${msg}`);
	process.exit(code);
}

// ---- arguments ---------------------------------------------------------------

const argv = process.argv.slice(2);
const opts = { branch: "main", dryRun: false };
const pairs = [];
for (let i = 0; i < argv.length; i++) {
	const a = argv[i];
	if (a === "--repo") opts.repo = argv[++i];
	else if (a === "--base") opts.base = argv[++i];
	else if (a === "--message" || a === "-m") opts.message = argv[++i];
	else if (a === "--branch") opts.branch = argv[++i];
	else if (a === "--dry-run") opts.dryRun = true;
	else if (a.startsWith("--")) die(`unknown option ${a}`, 64);
	else pairs.push(a);
}

if (!opts.repo || !opts.message || !opts.base || pairs.length === 0) {
	console.error("usage: kb-commit.mjs --repo <owner/name> --base <commit-sha> --message <msg> [--branch main] [--dry-run] <repo-path> <local-file> ...");
	process.exit(64);
}
if (pairs.length % 2 !== 0) die(`got ${pairs.length} path/file argument(s); they must come in <repo-path> <local-file> pairs.`, 64);
if (!SHA40.test(opts.base)) {
	die(`--base "${opts.base}" is not a 40-character commit SHA.\n` +
	    `  Read it BEFORE editing, with:  gh api repos/${opts.repo}/commits/heads/${opts.branch} --jq .sha\n` +
	    `  Note a 404 from gh api --jq prints the error body to STDOUT, so an unchecked capture\n` +
	    `  yields a JSON blob rather than an empty string -- which is why the shape is validated.`, 64);
}

const files = [];
for (let i = 0; i < pairs.length; i += 2) {
	const [repoPath, localFile] = [pairs[i], pairs[i + 1]];
	if (!existsSync(localFile)) die(`content file not found: ${localFile}`, 66);
	// Normalize to LF. The API stores these bytes verbatim -- no git clean filter runs
	// on this path -- so .gitattributes eol=lf does not govern it, and a CRLF scratch
	// file puts CRs straight into the knowledge base (LL-G had 24 arrive this way).
	// DELETE the CRs; translating each to a newline is the expansion itself.
	// See LL-G kb/git/crlf-expansion-doubles-file.md.
	const content = readFileSync(localFile).toString("utf8").split("\r").join("");
	files.push({ repoPath, localFile, content });
}

// ---- gh helpers -------------------------------------------------------------

const gh = (args, body) => {
	const res = execFileSync("gh", ["api", ...args], {
		encoding: "utf8",
		input: body === undefined ? undefined : JSON.stringify(body),
		maxBuffer: 64 * 1024 * 1024,
	});
	return res.trim();
};
const ghJson = (args, body) => JSON.parse(gh([...args, "--method", body ? "POST" : "GET", ...(body ? ["--input", "-"] : [])], body));

// ---- dry run ----------------------------------------------------------------

if (opts.dryRun) {
	console.log(`kb-commit (dry run): ${opts.repo}@${opts.branch}, parent ${opts.base}`);
	console.log(`  message: ${opts.message.split("\n")[0]}`);
	for (const f of files) {
		const crs = (readFileSync(f.localFile).toString("utf8").match(/\r/g) || []).length;
		console.log(`  ${f.repoPath}  <- ${f.localFile}  (${Buffer.byteLength(f.content)} bytes after normalization, ${crs} CR(s) stripped)`);
	}
	console.log("kb-commit: dry run, nothing written.");
	process.exit(0);
}

// ---- blobs -> tree -> commit -> ref ----------------------------------------

let baseTree;
try {
	baseTree = ghJson([`repos/${opts.repo}/git/commits/${opts.base}`]).tree.sha;
} catch {
	die(`could not read commit ${opts.base} in ${opts.repo}. Is the SHA from this repo and still present?`);
}

const tree = [];
for (const f of files) {
	const blob = ghJson([`repos/${opts.repo}/git/blobs`], {
		content: Buffer.from(f.content, "utf8").toString("base64"),
		encoding: "base64",
	});
	if (!SHA40.test(blob.sha || "")) die(`blob creation for ${f.repoPath} returned no usable SHA.`);
	tree.push({ path: f.repoPath, mode: "100644", type: "blob", sha: blob.sha });
	console.log(`  blob  ${f.repoPath}  ${blob.sha.slice(0, 7)}`);
}

const newTree = ghJson([`repos/${opts.repo}/git/trees`], { base_tree: baseTree, tree });
const commit = ghJson([`repos/${opts.repo}/git/commits`], {
	message: opts.message,
	tree: newTree.sha,
	parents: [opts.base],
});
console.log(`  commit ${commit.sha.slice(0, 7)} (${files.length} file(s), parent ${opts.base.slice(0, 7)})`);

// force=false: a non-fast-forward means someone pushed while we were editing. That is
// the compare-and-swap actually doing its job -- do not retry with force.
try {
	gh([`repos/${opts.repo}/git/refs/heads/${opts.branch}`, "--method", "PATCH",
	    "-f", `sha=${commit.sha}`, "-F", "force=false"]);
} catch (e) {
	die(`the ref update was refused, which means ${opts.branch} moved while you were editing.\n` +
	    `  Your commit ${commit.sha.slice(0, 7)} exists but is not on the branch; nothing was lost and nothing was clobbered.\n` +
	    `  Re-read the files, re-apply your change on top of the NEW content, take a fresh --base, and run this again.\n` +
	    `  Do not retry with force: that discards whatever landed in between.\n` +
	    `  ${String(e.stderr || e.message).trim().split("\n")[0]}`);
}

console.log(`kb-commit OK: https://github.com/${opts.repo}/commit/${commit.sha}`);
