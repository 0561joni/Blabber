// Builds the self-contained VibeVoice-ASR (MLX) worker that Blabber bundles.
//
// VibeVoice runs through the Python package `mlx-audio`. The app must never
// depend on whatever `python3` happens to be on the PATH (it will not have
// mlx-audio installed), so this script creates a private virtual environment,
// installs the locked requirements, freezes the worker with PyInstaller
// (one-folder mode) and stages the result in src-tauri/bundle/vibevoice/, which
// tauri.macos.conf.json packages into Blabber.app/Contents/Resources/workers/vibevoice/.
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { resolveSigningIdentity } from "./local-signing.mjs";
import { ensureVenv, run, signMachO } from "./python-runtime.mjs";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
if (process.platform !== "darwin" || process.arch !== "arm64") {
  throw new Error("The VibeVoice MLX worker requires Apple Silicon macOS.");
}

const workerDir = join(root, "workers/vibevoice");
const target = join(root, "src-tauri/target/vibevoice-runtime");
const bundle = join(root, "src-tauri/bundle/vibevoice");
const executableName = "blabber-vibevoice-worker";
const bundledExecutable = join(bundle, executableName, executableName);
const stampPath = join(bundle, ".blabber-build");

const stamp = createHash("sha256")
  .update(readFileSync(join(workerDir, "requirements.lock")))
  .update(readFileSync(join(workerDir, "blabber_vibevoice_worker.py")))
  .update(readFileSync(join(workerDir, "build.sh")))
  .update(readFileSync(fileURLToPath(import.meta.url)))
  .digest("hex");
if (existsSync(bundledExecutable) && existsSync(stampPath) && readFileSync(stampPath, "utf8") === stamp) {
  console.log("VibeVoice worker is up to date.");
  process.exit(0);
}

mkdirSync(target, { recursive: true });
const venvPython = ensureVenv(join(target, "venv"), join(workerDir, "requirements.lock"), { cwd: root });
// Fail fast here instead of at transcription time.
run(venvPython, ["-c", "import mlx.core, mlx_audio.stt.generate, mlx_audio.stt.utils; print('mlx-audio import OK')"]);

const distDir = join(target, "dist");
rmSync(distDir, { recursive: true, force: true });
run("bash", [join(workerDir, "build.sh"), distDir, join(target, "build")], {
  cwd: root,
  env: { ...process.env, PYTHON312: venvPython, BLABBER_CODESIGN_IDENTITY: "" },
});

// Stage the one-folder bundle. Symlinks are dereferenced because Tauri's
// resource copy does not reproduce them inside the app bundle.
rmSync(join(bundle, executableName), { recursive: true, force: true });
mkdirSync(bundle, { recursive: true });
cpSync(join(distDir, executableName), join(bundle, executableName), { recursive: true, dereference: true });
signMachO(join(bundle, executableName), bundledExecutable, resolveSigningIdentity());

// Smoke test: the frozen worker must import its whole runtime on its own.
const smoke = spawnSync(bundledExecutable, ["--self-test"], { encoding: "utf8" });
if (smoke.status !== 0 || !/"selfTest"/.test(smoke.stdout ?? "")) {
  throw new Error(`VibeVoice worker smoke test failed:\n${smoke.stdout}\n${smoke.stderr}`);
}
writeFileSync(stampPath, stamp);
console.log(`VibeVoice worker ready: ${bundledExecutable}`);
