// Rewrites the download-button block in README.md (between the BEGIN/END
// markers) with the given release version. Run by the release workflow.
//
//   node scripts/update-readme-version.mjs <tag> <owner/repo>

import { readFileSync, writeFileSync } from "node:fs";

const version = (process.argv[2] ?? "").replace(/^v/, "");
const repo = process.argv[3] ?? "";

if (!version || !repo) {
  console.error("usage: update-readme-version.mjs <tag> <owner/repo>");
  process.exit(1);
}

const block = `<!-- BEGIN LATEST DOWNLOAD BUTTON -->
[![Download](https://img.shields.io/badge/Download-Epic_Quick_Switch_${version}-7c3aed?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/${repo}/releases/latest/download/EpicQuickSwitch-Setup.exe)

![Platform](https://img.shields.io/badge/platform-Windows_10%2F11-lightgrey?style=flat-square&logo=windows)
![Version](https://img.shields.io/badge/version-${version}-7c3aed?style=flat-square)
<!-- END LATEST DOWNLOAD BUTTON -->`;

const path = "README.md";
const original = readFileSync(path, "utf8");
const updated = original.replace(
  /<!-- BEGIN LATEST DOWNLOAD BUTTON -->[\s\S]*?<!-- END LATEST DOWNLOAD BUTTON -->/,
  block,
);

if (updated === original) {
  console.log("README download block unchanged.");
} else {
  writeFileSync(path, updated);
  console.log(`Updated README download button to ${version}.`);
}
