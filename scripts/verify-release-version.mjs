import { readFileSync } from "node:fs";

const tag = process.env.GITHUB_REF_NAME;
const packageVersion = JSON.parse(readFileSync("package.json", "utf8")).version;
const tauriVersion = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
if (!tag?.startsWith("v")) throw new Error("Release must be triggered by a v* tag");
if (tag.slice(1) !== packageVersion || packageVersion !== tauriVersion) {
  throw new Error(`Version mismatch: tag=${tag}, package=${packageVersion}, tauri=${tauriVersion}`);
}
process.stdout.write(`Release versions agree on ${packageVersion}.\n`);
