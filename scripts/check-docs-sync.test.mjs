#!/usr/bin/env node
//
// check-docs-sync.test.mjs -- the drift guard's version-bump exemption must be
// exactly that narrow. Too broad and real manifest changes (a new workspace
// member, a toolchain floor) slip past the guard silently; too narrow and it
// fails every commit again, because every commit bumps the version.
//
// Run: node --test scripts/check-docs-sync.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { isVersionOnlyDiff } from "./docs-sync-version-only.mjs";

const cargoBump = `diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -26 +26 @@ license = "MIT"
-version = "0.48.0"
+version = "0.49.0"
`;

const packageBump = `--- a/package.json
+++ b/package.json
@@ -3 +3 @@
-  "version": "0.48.0",
+  "version": "0.49.0",
`;

test("a workspace version bump is version-only", () => {
	assert.equal(isVersionOnlyDiff("Cargo.toml", cargoBump), true);
	assert.equal(isVersionOnlyDiff("Cargo.toml", cargoBump.replaceAll("\n", "\r\n")), true);
});

test("a package.json version bump is version-only", () => {
	assert.equal(isVersionOnlyDiff("package.json", packageBump), true);
});

test("a bump plus any other manifest change is a real change", () => {
	const withMember = `${cargoBump}@@ -12,0 +13 @@
+    "crates/hark-meeting",
`;
	assert.equal(isVersionOnlyDiff("Cargo.toml", withMember), false);
	const withMsrv = `${cargoBump}@@ -30 +30 @@
-rust-version = "1.97"
+rust-version = "1.98"
`;
	assert.equal(isVersionOnlyDiff("Cargo.toml", withMsrv), false);
});

test("only the release-version files are ever exempt", () => {
	const crateManifest = cargoBump.replaceAll("Cargo.toml", "crates/hark-audio/Cargo.toml");
	assert.equal(isVersionOnlyDiff("crates/hark-audio/Cargo.toml", crateManifest), false);
	assert.equal(isVersionOnlyDiff("Cargo.lock", cargoBump), false);
});

test("an indented or non-semver version line is not the release version", () => {
	const depVersion = `--- a/Cargo.toml
+++ b/Cargo.toml
@@ -40 +40 @@
-  version = "0.62.2"
+  version = "0.63.0"
`;
	assert.equal(isVersionOnlyDiff("Cargo.toml", depVersion), false);
});

test("an empty diff is not a version bump", () => {
	assert.equal(isVersionOnlyDiff("Cargo.toml", ""), false);
	assert.equal(isVersionOnlyDiff("Cargo.toml", "--- a/Cargo.toml\n+++ b/Cargo.toml\n"), false);
});
