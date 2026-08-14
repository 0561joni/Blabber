import { cpSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const rootDir = process.cwd();
const tauriConfigPath = join(rootDir, "src-tauri", "tauri.conf.json");
const tauriConfig = JSON.parse(readFileSync(tauriConfigPath, "utf8"));
const productName = tauriConfig.productName;
const version = tauriConfig.version;
const architecture = process.arch === "arm64" ? "aarch64" : process.arch === "x64" ? "x86_64" : process.arch;
const bundleRoot = join(rootDir, "src-tauri", "target", "release", "bundle");
const macosBundleDir = join(bundleRoot, "macos");
const dmgBundleDir = join(bundleRoot, "dmg");
const appPath = join(macosBundleDir, `${productName}.app`);
const outputDmgName = `${productName}_${version}_${architecture}.dmg`;
const outputDmgPath = join(macosBundleDir, outputDmgName);
const outputDmgCompatPath = join(dmgBundleDir, outputDmgName);
const entitlementsPath = join(rootDir, "src-tauri", "Entitlements.plist");
const signatureResourcesPath = join(appPath, "Contents", "_CodeSignature", "CodeResources");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: rootDir,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

if (process.platform !== "darwin") {
  process.exit(0);
}

mkdirSync(macosBundleDir, { recursive: true });
mkdirSync(dmgBundleDir, { recursive: true });

if (!existsSync(signatureResourcesPath)) {
  console.log("No macOS signing identity detected; applying an ad-hoc bundle signature.");
  run("codesign", [
    "--force",
    "--deep",
    "--sign",
    "-",
    "--entitlements",
    entitlementsPath,
    appPath,
  ]);
}
run("codesign", ["--verify", "--deep", "--strict", appPath]);

const stageDir = mkdtempSync(join(tmpdir(), "blabber-dmg-stage-"));
const stageRoot = join(stageDir, productName);

try {
  mkdirSync(stageRoot, { recursive: true });
  cpSync(appPath, join(stageRoot, `${productName}.app`), { recursive: true });
  symlinkSync("/Applications", join(stageRoot, "Applications"));

  rmSync(outputDmgPath, { force: true });
  rmSync(outputDmgCompatPath, { force: true });

  run("hdiutil", [
    "create",
    "-volname",
    productName,
    "-srcfolder",
    stageRoot,
    "-ov",
    "-format",
    "UDZO",
    outputDmgPath,
  ]);

  cpSync(outputDmgPath, outputDmgCompatPath, { force: true });
} finally {
  rmSync(stageDir, { force: true, recursive: true });
}
