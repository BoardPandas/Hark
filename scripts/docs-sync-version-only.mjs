/**
 * Version-only change detection for the documentation drift guard.
 *
 * Every commit bumps the release version in the root `Cargo.toml` and
 * `package.json` (`.claude/rules/commit-changelog.md`), and those files are
 * mapped to pages that document the workspace and the release process. Without
 * this exemption each bump flags those pages as stale, so the guard fails on
 * every commit and teaches everyone to ignore it. A change that touches ONLY
 * the version line carries nothing a page could be out of date about; any
 * other line in the same diff still counts as a real change.
 *
 * Kept in its own module, free of import-time side effects, so it can be
 * unit-tested (`scripts/check-docs-sync.test.mjs`) without a git repo.
 */

/** The release-version files, and the one line in each a bump touches. */
const VERSION_LINES = new Map([
	// Only [workspace.package] has an unindented `version = "x.y.z"` in the
	// root manifest; dependency tables use inline `{ version = ... }` form.
	["Cargo.toml", /^version = "\d+\.\d+\.\d+"$/],
	["package.json", /^\s*"version": "\d+\.\d+\.\d+",?$/],
]);

/**
 * True when `diff` (unified, any context size) for `path` changes nothing but
 * the release-version line. Always false for other files, and for an empty
 * diff (a pure mode change or rename is not a version bump).
 */
export const isVersionOnlyDiff = (path, diff) => {
	const pattern = VERSION_LINES.get(path);
	if (!pattern) return false;
	const changed = diff
		.split(/\r?\n/)
		.filter((line) => /^[+-]/.test(line) && !/^(\+\+\+|---)( |$)/.test(line))
		.map((line) => line.slice(1).replace(/\r$/, ""));
	return changed.length > 0 && changed.every((line) => pattern.test(line));
};
