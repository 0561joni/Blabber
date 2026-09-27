// Stable code signature for local macOS builds without a paid Apple account.
//
// Ad-hoc signatures ("-") change with every build, so macOS privacy settings
// (Microphone, Accessibility) treat each build as a new app. A self-signed
// certificate created once by `scripts/setup-local-signing.sh` keeps the
// signature's identity stable, so those permissions survive rebuilds.
import { spawnSync } from "node:child_process";

export const LOCAL_SIGNING_IDENTITY = "Blabber Local Signing";

export function resolveSigningIdentity() {
  if (process.env.APPLE_SIGNING_IDENTITY) return process.env.APPLE_SIGNING_IDENTITY;
  if (process.platform !== "darwin") return "-";
  const found = spawnSync("security", ["find-certificate", "-c", LOCAL_SIGNING_IDENTITY], {
    stdio: "ignore",
  });
  return found.status === 0 ? LOCAL_SIGNING_IDENTITY : "-";
}
