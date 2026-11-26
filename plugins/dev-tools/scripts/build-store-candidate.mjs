#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const pluginRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const candidatesPath = process.argv[2]
  ? resolve(process.cwd(), process.argv[2])
  : resolve(pluginRoot, "dist/release-candidates.json");
const outputPath = resolve(pluginRoot, "dist/store-candidate.json");

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function isHttps(value) {
  try {
    return new URL(value).protocol === "https:";
  } catch {
    return false;
  }
}

function releaseDownloadPrefix(source) {
  const url = new URL(source);
  assert(url.protocol === "https:" && url.hostname === "github.com", "source must be an HTTPS GitHub URL");
  const parts = url.pathname.split("/").filter(Boolean);
  assert(parts.length >= 4 && parts[2] === "tree", "source must point to a GitHub tree tag");
  const [, , , ...tagParts] = parts;
  return `https://github.com/${parts[0]}/${parts[1]}/releases/download/${tagParts.join("/")}/`;
}

const release = JSON.parse(readFileSync(candidatesPath, "utf8"));
const storeMetadata = JSON.parse(readFileSync(resolve(pluginRoot, ".dbx-store.json"), "utf8"));
const publisher = JSON.parse(readFileSync(resolve(pluginRoot, "store/publishers/qiuyouyao.json"), "utf8"));

const plugin = release.plugin;
assert(plugin && typeof plugin === "object", "release-candidates.json must contain plugin identity");
assert(typeof plugin.id === "string" && plugin.id.length > 0, "plugin.id is required");
assert(typeof plugin.publisher === "string" && plugin.publisher.length > 0, "plugin.publisher is required");
assert(typeof plugin.version === "string" && plugin.version.length > 0, "plugin.version is required");
assert(plugin.publisher === publisher.id, `plugin.publisher must match publisher record '${publisher.id}'`);
assert(isHttps(storeMetadata.source), ".dbx-store.json source must use HTTPS");
assert(isHttps(storeMetadata.icon), ".dbx-store.json icon must use HTTPS");
assert(typeof storeMetadata.name === "string" && storeMetadata.name.trim().length > 0, ".dbx-store.json name is required");
assert(typeof storeMetadata.license === "string" && storeMetadata.license.trim().length > 0, ".dbx-store.json license is required");
assert(Array.isArray(release.artifacts) && release.artifacts.length > 0, "release-candidates.json must contain artifacts");

const downloadPrefix = releaseDownloadPrefix(storeMetadata.source);
const targets = release.artifacts.map((artifact) => {
  assert(/^[a-z0-9-]+$/.test(artifact.target || ""), `invalid target: ${artifact.target}`);
  assert(/^[a-fA-F0-9]{64}$/.test(artifact.sha256 || ""), `invalid sha256 for target: ${artifact.target}`);
  assert(Number.isSafeInteger(artifact.size) && artifact.size > 0 && artifact.size <= 536870912, `invalid size for target: ${artifact.target}`);

  const fileName = basename(new URL(artifact.url, downloadPrefix).pathname);
  assert(fileName.endsWith(".dbxp"), `artifact.url must point to a .dbxp file: ${artifact.url}`);
  const url = isHttps(artifact.url) ? artifact.url : `${downloadPrefix}${fileName}`;
  assert(isHttps(url), `artifact URL must be HTTPS: ${url}`);

  return {
    target: artifact.target,
    url,
    sha256: artifact.sha256,
    size: artifact.size,
  };
});

const candidate = {
  schemaVersion: 1,
  id: plugin.id,
  publisher: plugin.publisher,
  version: plugin.version,
  name: storeMetadata.name || plugin.name,
  description: storeMetadata.description || plugin.description || "",
  icon: storeMetadata.icon,
  tags: storeMetadata.tags || [],
  permissions: plugin.permissions || [],
  source: storeMetadata.source,
  homepage: storeMetadata.homepage || storeMetadata.source,
  license: storeMetadata.license,
  releaseNotes: storeMetadata.releaseNotes || "",
  localizations: storeMetadata.localizations || {},
  targets,
};

mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify(candidate, null, 2)}\n`);
console.log(outputPath);
