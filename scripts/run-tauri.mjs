import { existsSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { spawnSync } from "node:child_process";

const rootDir = process.cwd();
const tauriBinary = join(
  rootDir,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tauri.cmd" : "tauri",
);
const args = process.argv.slice(2);

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
    const name = basename(normalized).toLowerCase();
    const directLib = join(normalized, "lib", "x64", "cudart.lib");
    if (existsSync(directLib)) {
      return normalized;
    }

    if (name === "bin" || name === "libnvvp") {
      const parent = dirname(normalized);
      if (existsSync(join(parent, "lib", "x64", "cudart.lib"))) {
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
    .filter((cudaRoot) => existsSync(join(cudaRoot, "lib", "x64", "cudart.lib")))
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

function inferCudaVersionKey(cudaRoot) {
  const version = parseCudaVersion(cudaRoot);
  if (!version) {
    return null;
  }

  return `CUDA_PATH_V${version.major}_${version.minor}`;
}

function withNativeBuildEnv() {
  const nextEnv = { ...process.env };

  if (process.platform === "win32") {
    const installedCudaRoot = listInstalledCudaRoots()[0] ?? null;
    const envCudaRoot = resolveCudaRootFromEnv(nextEnv);
    const cudaRoot = installedCudaRoot ?? envCudaRoot;
    if (cudaRoot) {
      const cudaBin = join(cudaRoot, "bin");
      const nvccPath = join(cudaBin, "nvcc.exe");
      nextEnv.CUDA_PATH = cudaRoot;
      nextEnv.CUDA_BIN_PATH = cudaBin;
      nextEnv.CUDAToolkit_ROOT = cudaRoot;
      nextEnv.CUDACXX = nvccPath;
      nextEnv.CMAKE_CUDA_COMPILER = nvccPath;

      for (const key of Object.keys(nextEnv)) {
        if (key.startsWith("CUDA_PATH_V")) {
          nextEnv[key] = cudaRoot;
        }
      }

      const versionKey = inferCudaVersionKey(cudaRoot);
      if (versionKey) {
        nextEnv[versionKey] = cudaRoot;
      }

      if (!nextEnv.PATH?.toLowerCase().includes(cudaBin.toLowerCase())) {
        nextEnv.PATH = nextEnv.PATH ? `${cudaBin};${nextEnv.PATH}` : cudaBin;
      }
    }

    if (!nextEnv.LIBCLANG_PATH) {
      const llvmBin = "C:\\Program Files\\LLVM\\bin";
      if (existsSync(join(llvmBin, "libclang.dll"))) {
        nextEnv.LIBCLANG_PATH = llvmBin;
      }
    }
  }

  return nextEnv;
}

function run(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    cwd: rootDir,
    env: withNativeBuildEnv(),
    stdio: "inherit",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const isBuildCommand = args[0] === "build";
const hasBundleOverride = args.includes("--bundles");

if (process.platform === "darwin" && isBuildCommand && !hasBundleOverride) {
  run(tauriBinary, ["build", "--bundles", "app", ...args.slice(1)]);
  run(process.execPath, [join(rootDir, "scripts", "build-macos-dmg.mjs")]);
} else {
  run(tauriBinary, args);
}
