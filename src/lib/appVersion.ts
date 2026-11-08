import { getVersion } from "@tauri-apps/api/app";

let installedVersionRequest: Promise<string> | null = null;

/** Reads the version compiled into the running Tauri package, including prerelease suffixes. */
export function getInstalledVersion(): Promise<string> {
  if (!installedVersionRequest) {
    installedVersionRequest = getVersion()
      .then((version) => {
        if (!version.trim()) throw new Error("Tauri returned an empty application version.");
        return version;
      })
      .catch((error: unknown) => {
        console.error(
          "Could not read the installed application version via Tauri core app API (core:app:allow-version):",
          error
        );
        installedVersionRequest = null;
        throw error;
      });
  }
  return installedVersionRequest;
}
