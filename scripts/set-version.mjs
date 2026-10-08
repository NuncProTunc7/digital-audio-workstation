// Sets the app version everywhere a release checks it:
//   node scripts/set-version.mjs 0.2.0
// Then commit, tag v0.2.0, and push the tag to publish a release.
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("usage: node scripts/set-version.mjs <major.minor.patch>");
  process.exit(1);
}

const edit = (path, pattern, replacement) => {
  const text = readFileSync(path, "utf8");
  if (!pattern.test(text)) {
    console.error(`no version found in ${path}`);
    process.exit(1);
  }
  writeFileSync(path, text.replace(pattern, replacement));
  console.log(`${path} -> ${version}`);
};

edit("Cargo.toml", /^version = "[^"]+"/m, `version = "${version}"`);
edit("app/package.json", /"version": "[^"]+"/, `"version": "${version}"`);
edit("app/src-tauri/tauri.conf.json", /"version": "[^"]+"/, `"version": "${version}"`);
