import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";

const WECHAT_CLI = {
  module: "github.com/r266-tech/wechat-cli",
  version: "v1.6.20",
  url: "https://proxy.golang.org/github.com/r266-tech/wechat-cli/@v/v1.6.20.zip",
  sha256: "485da9f149894618817d5e9b5581b443d37bc6835bd6ad77170275f8bb8a579d",
};
const WXKEY = {
  module: "github.com/r266-tech/wxkey",
  version: "v1.4.8",
  url: "https://proxy.golang.org/github.com/r266-tech/wxkey/@v/v1.4.8.zip",
  sha256: "9477ab94245af1594c1fccebf0504863732832e8bf2d1f81105b12bde9fb6083",
};
const WCDB = {
  version: "v2.1.15",
  url: "https://github.com/Tencent/wcdb/releases/download/v2.1.15/wcdb-2.1.15.zip",
  sha256: "5b7294459f7f8b85ce373fcd5f3d7a3c004743feb9d51d3c39096a0815834c56",
};

const target = process.env.CHAT_INSIGHT_TARGET || process.env.TAURI_ENV_TARGET_TRIPLE || "";
const goBinary = process.env.CHAT_INSIGHT_GO || "go";
const output = resolve("src-tauri/resources/wechat-bridge");
if (!output.endsWith(join("src-tauri", "resources", "wechat-bridge"))) {
  throw new Error(`Refusing unexpected bridge output path: ${output}`);
}

mkdirSync(output, { recursive: true });
for (const entry of readdirSync(output)) {
  if (entry !== ".gitkeep") rmSync(join(output, entry), { recursive: true, force: true });
}

const targetConfig = target.includes("aarch64-apple-darwin")
  ? { goos: "darwin", goarch: "arm64", cli: "wechat-cli", library: "libWCDB.dylib", wxkey: true }
  : target.includes("x86_64-pc-windows")
    ? { goos: "windows", goarch: "amd64", cli: "wechat-cli.exe", library: "libWCDB.dll", wxkey: false }
    : undefined;

if (!targetConfig) {
  writeFileSync(join(output, "UNSUPPORTED.txt"), "This build uses Chat Insight JSONL import mode and does not bundle a WeChat bridge.\n");
  writeFileSync(
    join(output, "bridge-manifest.json"),
    `${JSON.stringify({ version: WECHAT_CLI.version, target, mode: "import-only" }, null, 2)}\n`,
  );
  process.stdout.write(`No bridge is required for ${target || "this target"}.\n`);
  process.exit(0);
}

const work = mkdtempSync(join(tmpdir(), "chat-insight-bridge-build-"));
try {
  const wechatRoot = await downloadModule(WECHAT_CLI, join(work, "wechat-cli"));
  const cliPath = join(output, targetConfig.cli);
  buildGo(wechatRoot, "./cmd/wechat-cli", cliPath, targetConfig);

  if (targetConfig.wxkey) {
    const wxkeyRoot = await downloadModule(WXKEY, join(work, "wxkey"));
    buildGo(wxkeyRoot, "./cmd/wxkey", join(output, "wxkey"), targetConfig);
    copyIfPresent(join(wxkeyRoot, "LICENSE"), join(output, "LICENSE.wxkey"));
  }

  const wcdbRoot = await downloadArchive(WCDB, join(work, "wcdb"));
  buildWcdb(wcdbRoot, join(output, targetConfig.library), targetConfig, work);
  copyIfPresent(join(wechatRoot, "LICENSE"), join(output, "LICENSE.wechat-cli"));
  copyIfPresent(join(wechatRoot, "SECURITY.md"), join(output, "SECURITY.wechat-cli.md"));
  copyIfPresent(join(wechatRoot, "THIRD_PARTY_NOTICES.md"), join(output, "THIRD_PARTY_NOTICES.wechat-cli.md"));
  copyIfPresent(join(wcdbRoot, "LICENSE"), join(output, "LICENSE.WCDB"));

  if (targetConfig.goos === "darwin") {
    for (const file of [cliPath, join(output, "wxkey"), join(output, targetConfig.library)]) {
      chmodSync(file, 0o755);
      execFileSync("codesign", ["--force", "--sign", "-", file], { stdio: "inherit" });
    }
  }

  if (
    (targetConfig.goos === "darwin" && process.platform === "darwin" && process.arch === "arm64") ||
    (targetConfig.goos === "windows" && process.platform === "win32")
  ) {
    const versionOutput = execFileSync(cliPath, ["--version"], { encoding: "utf8" });
    if (!versionOutput.includes(WECHAT_CLI.version.slice(1))) {
      throw new Error(`Built wechat-cli does not report ${WECHAT_CLI.version}`);
    }
  }

  writeFileSync(
    join(output, "bridge-manifest.json"),
    `${JSON.stringify({
      mode: "source-build",
      target,
      wechat_cli: WECHAT_CLI,
      wxkey: targetConfig.wxkey ? WXKEY : undefined,
      wcdb: WCDB,
    }, null, 2)}\n`,
  );
  process.stdout.write(`Built ${WECHAT_CLI.module}@${WECHAT_CLI.version} for ${target}.\n`);
} finally {
  rmSync(work, { recursive: true, force: true });
}

async function downloadModule(spec, destination) {
  const metadata = JSON.parse(
    execFileSync(goBinary, ["mod", "download", "-json", `${spec.module}@${spec.version}`], {
      encoding: "utf8",
    }),
  );
  if (!metadata.Zip || !metadata.Dir) {
    throw new Error(`Go did not return source paths for ${spec.module}@${spec.version}`);
  }
  assertDigest(readFileSync(metadata.Zip), spec.sha256, `${spec.module}@${spec.version}`);
  return metadata.Dir;
}

async function downloadArchive(spec, destination, rootResolver = defaultArchiveRoot) {
  const archivePath = `${destination}.zip`;
  downloadFile(spec.url, archivePath);
  assertDigest(readFileSync(archivePath), spec.sha256, spec.url);
  mkdirSync(destination, { recursive: true });
  expandArchive(archivePath, destination);
  const root = rootResolver(destination);
  if (!root || root === "." || !statSync(root).isDirectory()) {
    throw new Error(`Unable to locate extracted source root for ${spec.url}`);
  }
  return root;
}

function downloadFile(url, destination) {
  if (process.platform === "win32") {
    execFileSync(
      "powershell",
      ["-NoProfile", "-Command", "Invoke-WebRequest", "-Uri", url, "-OutFile", destination],
      { stdio: "inherit" },
    );
  } else {
    execFileSync(
      "curl",
      ["-fsSL", "--retry", "3", "--connect-timeout", "20", url, "-o", destination],
      { stdio: "inherit" },
    );
  }
}

function assertDigest(contents, expected, label) {
  const actual = createHash("sha256").update(contents).digest("hex");
  if (actual !== expected) throw new Error(`SHA-256 mismatch for ${label}`);
}

function expandArchive(archive, destination) {
  if (process.platform === "win32") {
    execFileSync(
      "powershell",
      ["-NoProfile", "-Command", "Expand-Archive", "-LiteralPath", archive, "-DestinationPath", destination, "-Force"],
      { stdio: "inherit" },
    );
  } else {
    execFileSync("unzip", ["-q", archive, "-d", destination], { stdio: "inherit" });
  }
}

function defaultArchiveRoot(directory) {
  const entries = readdirSync(directory, { withFileTypes: true }).filter((entry) => entry.isDirectory());
  return entries.length === 1 ? join(directory, entries[0].name) : directory;
}

function buildGo(source, packagePath, destination, config) {
  execFileSync(
    goBinary,
    ["build", "-trimpath", "-ldflags=-s -w", "-o", destination, packagePath],
    {
      cwd: source,
      env: { ...process.env, GOOS: config.goos, GOARCH: config.goarch, CGO_ENABLED: "0" },
      stdio: "inherit",
    },
  );
}

function buildWcdb(sourceRoot, destination, config, workRoot) {
  const source = join(sourceRoot, "src");
  const build = join(workRoot, "wcdb-build");
  const configure = ["-S", source, "-B", build, "-DBUILD_SHARED_LIBS=ON"];
  if (config.goos === "windows") {
    configure.push(
      "-G", "Visual Studio 17 2022", "-A", "x64",
      "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
      "-DCMAKE_C_FLAGS=/DSQLITE_API=__declspec(dllexport)",
    );
  } else {
    configure.push("-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_OSX_ARCHITECTURES=arm64");
  }
  execFileSync("cmake", configure, { stdio: "inherit" });
  execFileSync("cmake", ["--build", build, "--config", "Release", "--target", "WCDB", "--parallel", "2"], { stdio: "inherit" });
  const produced = config.goos === "windows"
    ? findFile(build, "WCDB.dll")
    : findFile(build, "libWCDB.dylib") || findFrameworkBinary(build, "WCDB");
  if (!produced) throw new Error(`WCDB build did not produce ${basename(destination)}`);
  cpSync(produced, destination);
}

function findFrameworkBinary(directory, fileName) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const candidate = join(directory, entry.name);
    if (entry.isFile() && entry.name === fileName && candidate.includes(`${fileName}.framework`)) {
      return candidate;
    }
    if (entry.isDirectory()) {
      const nested = findFrameworkBinary(candidate, fileName);
      if (nested) return nested;
    }
  }
  return undefined;
}

function copyIfPresent(source, destination) {
  try {
    if (statSync(source).isFile()) {
      cpSync(source, destination);
      chmodSync(destination, 0o644);
    }
  } catch {
    // Optional documentation files vary between upstream packages.
  }
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
