import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";

const VERSION = "v1.6.20";
const REPOSITORY = "r266-tech/wechat-cli";
const target = process.env.CHAT_INSIGHT_TARGET || process.env.TAURI_ENV_TARGET_TRIPLE || "";
const output = resolve("src-tauri/resources/wechat-bridge");

if (!output.endsWith(join("src-tauri", "resources", "wechat-bridge"))) {
  throw new Error(`Refusing unexpected bridge output path: ${output}`);
}

mkdirSync(output, { recursive: true });
for (const entry of readdirSync(output)) {
  if (entry !== ".gitkeep") rmSync(join(output, entry), { recursive: true, force: true });
}

const targetConfig = target.includes("aarch64-apple-darwin")
  ? { asset: `wechat-cli-${VERSION}-darwin-arm64.zip`, required: ["wechat-cli", "wxkey"] }
  : target.includes("x86_64-pc-windows")
    ? { asset: `wechat-cli-${VERSION}-windows-amd64.zip`, required: ["wechat-cli.exe", "libWCDB.dll"] }
    : undefined;

if (!targetConfig) {
  writeFileSync(join(output, "UNSUPPORTED.txt"), "This build uses Chat Insight JSONL import mode and does not bundle a WeChat bridge.\n");
  writeFileSync(join(output, "bridge-manifest.json"), `${JSON.stringify({ version: VERSION, target, mode: "import-only" }, null, 2)}\n`);
  process.stdout.write(`No bridge is required for ${target || "this target"}.\n`);
  process.exit(0);
}

const headers = { Accept: "application/vnd.github+json", "User-Agent": "chat-insight-build" };
const releaseResponse = await fetch(`https://api.github.com/repos/${REPOSITORY}/releases/tags/${VERSION}`, { headers });
if (!releaseResponse.ok) throw new Error(`Unable to read pinned release metadata (${releaseResponse.status})`);
const release = await releaseResponse.json();
if (release.tag_name !== VERSION) throw new Error(`Expected ${VERSION}, received ${release.tag_name}`);
const asset = release.assets.find((item) => item.name === targetConfig.asset);
if (!asset) throw new Error(`Pinned asset ${targetConfig.asset} is missing`);
if (!asset.digest?.startsWith("sha256:")) throw new Error(`Pinned asset ${targetConfig.asset} has no GitHub SHA-256 digest`);

const archiveResponse = await fetch(asset.browser_download_url, { headers });
if (!archiveResponse.ok) throw new Error(`Unable to download ${targetConfig.asset} (${archiveResponse.status})`);
const archive = Buffer.from(await archiveResponse.arrayBuffer());
const actualDigest = createHash("sha256").update(archive).digest("hex");
const expectedDigest = asset.digest.slice("sha256:".length);
if (actualDigest !== expectedDigest) throw new Error(`SHA-256 mismatch for ${targetConfig.asset}`);

const temp = mkdtempSync(join(tmpdir(), "chat-insight-bridge-"));
const archivePath = join(temp, basename(targetConfig.asset));
writeFileSync(archivePath, archive);
const extracted = join(temp, "extracted");
mkdirSync(extracted);
try {
  if (process.platform === "win32") {
    execFileSync("powershell", ["-NoProfile", "-Command", "Expand-Archive", "-LiteralPath", archivePath, "-DestinationPath", extracted], { stdio: "inherit" });
  } else {
    execFileSync("unzip", ["-q", archivePath, "-d", extracted], { stdio: "inherit" });
  }
  const runtimeRoot = dirname(findFile(extracted, targetConfig.required[0]) || "");
  if (!runtimeRoot || runtimeRoot === ".") throw new Error(`Unable to locate the runtime root in ${targetConfig.asset}`);
  for (const entry of readdirSync(runtimeRoot)) {
    cpSync(join(runtimeRoot, entry), join(output, entry), { recursive: true });
  }
  for (const fileName of targetConfig.required) {
    const destination = join(output, fileName);
    if (!findFile(output, fileName)) throw new Error(`${fileName} is missing from ${targetConfig.asset}`);
    if (!fileName.endsWith(".dll") && !fileName.endsWith(".exe")) chmodSync(destination, 0o755);
  }
  writeFileSync(join(output, "bridge-manifest.json"), `${JSON.stringify({ repository: REPOSITORY, version: VERSION, target, asset: targetConfig.asset, sha256: expectedDigest }, null, 2)}\n`);
  process.stdout.write(`Prepared ${targetConfig.asset} (${actualDigest}).\n`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}

function findFile(directory, fileName) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const candidate = join(directory, entry.name);
    if (entry.isFile() && entry.name.toLowerCase() === fileName.toLowerCase()) return candidate;
    if (entry.isDirectory()) {
      const nested = findFile(candidate, fileName);
      if (nested) return nested;
    }
  }
  return undefined;
}
