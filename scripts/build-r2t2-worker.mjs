import { createHash } from "node:crypto";
import { createReadStream, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const manifest = JSON.parse(readFileSync(join(root, "workers/r2t2/manifest.json"), "utf8"));
if (process.platform !== "darwin" || process.arch !== "arm64") {
  throw new Error("The experimental R2T2 helper requires Apple Silicon macOS.");
}
const env = { ...process.env };
if (!env.DEVELOPER_DIR && existsSync("/Library/Developer/CommandLineTools/usr/bin/clang"))
  env.DEVELOPER_DIR = "/Library/Developer/CommandLineTools";
const target = join(root, "src-tauri/target/r2t2-runtime");
const archive = join(target, "audio.tar.gz");
const source = join(target, `audio.cpp-${manifest.runtimeRevision}`);
const build = join(target, "build");
const bundle = join(root, "src-tauri/bundle/r2t2");
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: "inherit", ...options });
  if (result.status !== 0) throw new Error(`${command} failed (${result.status ?? result.error})`);
}
async function sha256(path) {
  const hash = createHash("sha256");
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest("hex");
}
mkdirSync(target, { recursive: true });
if (!existsSync(archive)) run("curl", ["-fLsS", "--retry", "3", `https://codeload.github.com/0xShug0/audio.cpp/tar.gz/${manifest.runtimeRevision}`, "-o", archive]);
if (await sha256(archive) !== manifest.runtimeArchiveSha256)
  throw new Error("R2T2 runtime archive checksum mismatch; remove the cached archive and rebuild.");
const patch = join(root, "workers/r2t2/runtime.patch");
const rolling = join(root, "workers/r2t2/rolling.hpp");
const stamp = `${manifest.runtimeRevision}:${await sha256(patch)}:${await sha256(rolling)}`;
const stampPath = join(source, ".blabber-patch");
if (!existsSync(stampPath) || readFileSync(stampPath, "utf8") !== stamp) {
  // Only replace our ignored, verified source cache, never the working tree.
  rmSync(source, { recursive: true, force: true });
  run("tar", ["-xzf", archive, "-C", target]);
  run("patch", ["--batch", "-p1", "-i", patch], { cwd: source });
  copyFileSync(rolling, join(source, "include/blabber_rolling.hpp"));
  writeFileSync(stampPath, stamp);
}
run("cmake", ["-S", "workers/r2t2", "-B", build, `-DAUDIOCPP_SOURCE=${source}`, "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_OSX_DEPLOYMENT_TARGET=14.0", "-DENGINE_ENABLE_METAL=ON"]);
run("cmake", ["--build", build, "--target", "blabber-r2t2-worker", "blabber-r2t2-rolling-test", "audiocpp_cli", "-j", "6"]);
run("ctest", ["--test-dir", build, "-R", "^rolling-transcript$", "--output-on-failure"]);
mkdirSync(bundle, { recursive: true });
copyFileSync(join(build, "blabber-r2t2-worker"), join(bundle, "blabber-r2t2-worker"));
run("codesign", ["--force", "--sign", process.env.APPLE_SIGNING_IDENTITY || "-", join(bundle, "blabber-r2t2-worker")]);
copyFileSync(join(source, "LICENSE"), join(bundle, "audio.cpp-LICENSE"));
for (const license of ["ggml/LICENSE", "cJSON/LICENSE", "sentencepiece/LICENSE", "sentencepiece/third_party/protobuf-lite/LICENSE", "sentencepiece/third_party/absl/LICENSE", "sentencepiece/third_party/esaxx/LICENSE", "sentencepiece/third_party/darts_clone/LICENSE"]) {
  copyFileSync(join(source, "external", license), join(bundle, license.replaceAll("/", "-")));
}
copyFileSync(join(root, "workers/r2t2/MODEL_LICENSE"), join(bundle, "MODEL_LICENSE"));
copyFileSync(join(root, "workers/r2t2/libyaml-MIT.txt"), join(bundle, "libyaml-MIT.txt"));
copyFileSync(join(root, "src-tauri/licenses/llama.cpp-MIT.txt"), join(bundle, "llama-tokenizer-MIT.txt"));
copyFileSync(join(root, "workers/r2t2/manifest.json"), join(bundle, "manifest.json"));
