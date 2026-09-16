import { createHash, randomUUID } from "node:crypto";
import { readFile, mkdir, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const repository = "Rajaniraiyn/actuate";
export function target(platform = process.platform, arch = process.arch) {
  const targets = { "win32-x64": "x86_64-pc-windows-msvc", "darwin-x64": "x86_64-apple-darwin",
    "darwin-arm64": "aarch64-apple-darwin", "linux-x64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu" };
  const result = targets[`${platform}-${arch}`];
  if (!result) throw new Error(`No Actuate release binary for ${platform}/${arch}`);
  return result;
}
export async function checkedAsset(name, manifest, download) {
  const entry = manifest.assets[name];
  if (!entry || !/^[a-f0-9]{64}$/.test(entry.sha256) || !Number.isSafeInteger(entry.size) || entry.size < 1 || entry.size > 256 * 1024 * 1024) {
    throw new Error(`Invalid or missing release metadata for ${name}`);
  }
  const bytes = await download(name, entry.size);
  if (bytes.length !== entry.size || createHash("sha256").update(bytes).digest("hex") !== entry.sha256) {
    throw new Error(`Checksum mismatch for ${name}`);
  }
  return bytes;
}
async function responseBytes(response, limit) {
  if (!response.ok) throw new Error(`Release download failed: HTTP ${response.status}`);
  const chunks = []; let total = 0;
  for await (const chunk of response.body) {
    total += chunk.length;
    if (total > limit) throw new Error("Release asset exceeds its expected size");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}
export async function install({ destination = join(root, "native", "actuate.node"), download, version, selectedTarget } = {}) {
  version ??= JSON.parse(await readFile(join(root, "package.json"), "utf8")).version;
  if (!/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(version)) throw new Error("Invalid Actuate package version");
  selectedTarget ??= target();
  if (!download) {
    if (process.platform === "linux" && spawnSync("getconf", ["GNU_LIBC_VERSION"], { encoding: "utf8" }).status !== 0) {
      throw new Error("Actuate Linux releases require glibc. Build from source for other C libraries.");
    }
    const mirror = process.env.ACTUATE_RELEASE_BASE_URL;
    if (mirror && new URL(mirror).protocol !== "https:") throw new Error("Release mirrors must use HTTPS");
    const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
    let assets;
    if (token && !mirror) {
      const response = await fetch(`https://api.github.com/repos/${repository}/releases/tags/v${version}`, {
        headers: { Authorization: `Bearer ${token}`, Accept: "application/vnd.github+json" }, signal: AbortSignal.timeout(30000),
      });
      if (!response.ok) throw new Error(`Release lookup failed: HTTP ${response.status}`);
      assets = (await response.json()).assets;
    }
    download = async (name, limit) => {
      const asset = assets?.find(asset => asset.name === name);
      if (assets && !asset) throw new Error(`Release v${version} has no ${name}`);
      const url = asset?.url ?? `${mirror ?? `https://github.com/${repository}/releases/download`}/v${version}/${name}`;
      const response = await fetch(url, { headers: asset ? { Authorization: `Bearer ${token}`, Accept: "application/octet-stream" } : {}, signal: AbortSignal.timeout(120000) });
      return responseBytes(response, limit);
    };
  }
  const manifest = JSON.parse((await download("manifest.json", 1024 * 1024)).toString("utf8"));
  if (manifest.version !== version) throw new Error("Release manifest version does not match the package");
  const bytes = await checkedAsset(`actuate-napi-v${version}-${selectedTarget}.node`, manifest, download);
  await mkdir(dirname(destination), { recursive: true });
  const temporary = `${destination}.${randomUUID()}.tmp`;
  try {
    await writeFile(temporary, bytes, { flag: "wx" });
    await rename(temporary, destination);
  } finally { await rm(temporary, { force: true }); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.env.ACTUATE_SKIP_DOWNLOAD !== "1") await install();
}
