// Writes `latest.json`, the file installed copies of Motodesk read to learn that a newer
// version exists. It points at the signed installer attached to the same release and carries
// the changelog section shown to the user.
//
//   node updater-manifest.mjs <version> <installer.exe> <repository> <notes.md> <out.json>
//
// The signature is read from `<installer.exe>.sig`, written by `tauri build` when the signing
// key is available. The application refuses any installer whose signature does not verify
// against the public key compiled into it, so this file needs no protection of its own: the
// worst a tampered manifest can do is make an update fail.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { basename } from "node:path";

const [, , version, installer, repository, notesFile, out] = process.argv;

function fail(message) {
  console.error(`::error::${message}`);
  process.exit(1);
}

if (!version || !installer || !repository || !notesFile || !out) {
  fail("usage: updater-manifest.mjs <version> <installer> <repository> <notes> <out>");
}
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) fail(`not a version: ${version}`);
if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) fail(`not a repository: ${repository}`);

const sigFile = `${installer}.sig`;
if (!existsSync(installer)) fail(`installer not found: ${installer}`);
if (!existsSync(sigFile)) {
  fail(`${sigFile} not found: the build did not sign the installer (is TAURI_SIGNING_PRIVATE_KEY set?)`);
}

const signature = readFileSync(sigFile, "utf8").trim();
if (!signature) fail(`${sigFile} is empty`);

const notes = existsSync(notesFile) ? readFileSync(notesFile, "utf8").trim() : "";
const file = basename(installer);

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url: `https://github.com/${repository}/releases/download/v${version}/${file}`,
    },
  },
};

writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
console.log(`wrote ${out} for ${version} (${file})`);
