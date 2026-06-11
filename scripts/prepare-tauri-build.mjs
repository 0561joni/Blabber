import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { basename, dirname, join } from "node:path";

const bundleRoot = join(process.cwd(), "src-tauri", "target", "release", "bundle");
const cleanupDirs = [join(bundleRoot, "dmg"), join(bundleRoot, "macos")];
const stagedCudaRuntimeDir = join(process.cwd(), "src-tauri", "bundle", "windows-cuda-runtime");
const cudaRuntimeDllPatterns = [
  /^cublas64_\d+\.dll$/i,
  /^cublasLt64_\d+\.dll$/i,
  /^cudart64_\d+\.dll$/i,
];

for (const directory of cleanupDirs) {
  let entries = [];
  try {
    entries = readdirSync(directory, { withFileTypes: true });
  } catch {
    continue;
  }

  for (const entry of entries) {
    const fullPath = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name.startsWith("dmg-stage-")) {
        rmSync(fullPath, { force: true, recursive: true });
      }
      continue;
    }

    const shouldRemove =
      entry.name.endsWith(".dmg") ||
      entry.name.endsWith(".tmp.dmg") ||
      (entry.name.startsWith("rw.") && entry.name.endsWith(".dmg"));
    if (shouldRemove) {
      rmSync(fullPath, { force: true });
    }
  }
}

function resolveCudaRootFromEnv(env) {
  const candidates = [];

  if (env.CUDA_PATH) {
    candidates.push(env.CUDA_PATH);
  }

  for (const [key, value] of Object.entries(env)) {
    if (key.startsWith("CUDA_PATH_V") && value) {
      candidates.push(value);
    }
  }

  for (const rawCandidate of candidates) {
    if (!rawCandidate) {
      continue;
    }

    const normalized = rawCandidate.trim().replace(/[\\/]+$/, "");
    if (existsSync(join(normalized, "bin")) && existsSync(join(normalized, "lib", "x64", "cudart.lib"))) {
      return normalized;
    }

    const name = basename(normalized).toLowerCase();
    if (name === "bin" || name === "libnvvp") {
      const parent = dirname(normalized);
      if (existsSync(join(parent, "bin")) && existsSync(join(parent, "lib", "x64", "cudart.lib"))) {
        return parent;
      }
    }
  }

  return null;
}

function listInstalledCudaRoots() {
  const baseDir = "C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA";
  if (!existsSync(baseDir)) {
    return [];
  }

  return readdirSync(baseDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => join(baseDir, entry.name))
    .filter((cudaRoot) => existsSync(join(cudaRoot, "bin")) && existsSync(join(cudaRoot, "lib", "x64", "cudart.lib")))
    .sort((left, right) => compareCudaRoots(right, left));
}

function compareCudaRoots(left, right) {
  const leftParts = parseCudaVersion(left);
  const rightParts = parseCudaVersion(right);

  if (!leftParts || !rightParts) {
    return left.localeCompare(right);
  }

  if (leftParts.major !== rightParts.major) {
    return leftParts.major - rightParts.major;
  }

  return leftParts.minor - rightParts.minor;
}

function parseCudaVersion(cudaRoot) {
  const match = basename(cudaRoot).match(/^v(\d+)\.(\d+)$/i);
  if (!match) {
    return null;
  }

  return {
    major: Number.parseInt(match[1], 10),
    minor: Number.parseInt(match[2], 10),
  };
}

function stageWindowsCudaRuntime() {
  if (process.platform !== "win32" && process.env.TAURI_ENV_FAMILY !== "windows") {
    rmSync(stagedCudaRuntimeDir, { force: true, recursive: true });
    return;
  }

  const cudaRoot = listInstalledCudaRoots()[0] ?? resolveCudaRootFromEnv(process.env);
  if (!cudaRoot) {
    throw new Error(
      "Windows builds enable CUDA but no CUDA Toolkit root was found. Install CUDA or set CUDA_PATH before building.",
    );
  }

  const cudaRuntimeDirs = [join(cudaRoot, "bin"), join(cudaRoot, "bin", "x64")].filter((directory) =>
    existsSync(directory),
  );
  const dlls = cudaRuntimeDirs
    .flatMap((directory) =>
      readdirSync(directory, { withFileTypes: true })
        .filter((entry) => entry.isFile() && cudaRuntimeDllPatterns.some((pattern) => pattern.test(entry.name)))
        .map((entry) => ({ directory, name: entry.name })),
    )
    .sort((left, right) => left.name.localeCompare(right.name));

  const missing = cudaRuntimeDllPatterns
    .map((pattern) => pattern.source)
    .filter((patternSource) => !dlls.some((dll) => new RegExp(patternSource, "i").test(dll.name)));

  if (missing.length > 0) {
    throw new Error(
      `CUDA Toolkit at ${cudaRoot} is missing required runtime DLLs matching: ${missing.join(", ")}`,
    );
  }

  rmSync(stagedCudaRuntimeDir, { force: true, recursive: true });
  mkdirSync(stagedCudaRuntimeDir, { recursive: true });

  for (const dll of dlls) {
    copyFileSync(join(dll.directory, dll.name), join(stagedCudaRuntimeDir, dll.name));
  }
}

stageWindowsCudaRuntime();
