import { copyFile, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const [artifactArgument, outputArgument] = process.argv.slice(2);
if (!artifactArgument) {
  console.error("Usage: node scripts/prepare-updater-test-manifest.mjs <new-updater-artifact> [host-directory]");
  process.exit(2);
}

const artifact = path.resolve(artifactArgument);
const signaturePath = `${artifact}.sig`;
const outputDir = path.resolve(outputArgument ?? "src-tauri/target-updater-test-host");
const artifactStat = await stat(artifact);
if (!artifactStat.isFile() || !/\.(exe|nsis\.zip)$/i.test(artifact)) {
  throw new Error(`Expected a Windows Tauri updater artifact (.exe or .nsis.zip): ${artifact}`);
}

const signature = (await readFile(signaturePath, "utf8")).trim();
let decodedSignature;
try {
  decodedSignature = Buffer.from(signature, "base64").toString("utf8");
} catch {
  throw new Error(`The sidecar is not valid base64: ${signaturePath}`);
}
if (!decodedSignature.startsWith("untrusted comment:") || !decodedSignature.includes("trusted comment:")) {
  throw new Error(`The sidecar does not look like a Tauri/minisign signature: ${signaturePath}`);
}
if (!decodedSignature.includes("version:0.1.4-test.1")) {
  throw new Error("The signature is not bound to the expected test update version 0.1.4-test.1.");
}

await mkdir(outputDir, { recursive: true });
const filename = path.basename(artifact);
const hostedArtifact = path.join(outputDir, filename);
if (path.resolve(hostedArtifact) !== artifact) await copyFile(artifact, hostedArtifact);

const manifest = {
  version: "0.1.4-test.1",
  platforms: {
    "windows-x86_64": {
      url: `http://127.0.0.1:8765/${encodeURIComponent(filename)}`,
      signature,
    },
  },
};
const manifestPath = path.join(outputDir, "latest.json");
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
JSON.parse(await readFile(manifestPath, "utf8"));

console.log(`Prepared isolated Windows test manifest: ${manifestPath}`);
console.log(`Copied updater archive: ${hostedArtifact} (${artifactStat.size} bytes)`);
console.log(`Endpoint: http://127.0.0.1:8765/latest.json`);
console.log(`Artifact URL: ${manifest.platforms["windows-x86_64"].url}`);
