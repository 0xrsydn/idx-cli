#!/usr/bin/env node
"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const https = require("node:https");
const path = require("node:path");
const { pipeline } = require("node:stream");
const { URL, fileURLToPath } = require("node:url");

const packageRoot = path.resolve(__dirname, "..");
const packageManifest = JSON.parse(
  fs.readFileSync(path.join(packageRoot, "package.json"), "utf8"),
);
const binaryName = process.platform === "win32" ? "idx.exe" : "idx";
const binaryDirectory = path.join(packageRoot, "bin");
const binaryPath = path.join(binaryDirectory, binaryName);
const githubRepository = "0xrsydn/idx-cli";
const maxRedirects = 5;
const requestTimeoutMs = 60_000;

const targets = {
  "linux/x64": {
    label: "linux-x64",
    triple: "x86_64-unknown-linux-gnu",
    asset: "idx-linux-x64",
  },
  "linux/arm64": {
    label: "linux-arm64",
    triple: "aarch64-unknown-linux-gnu",
    asset: "idx-linux-arm64",
  },
  "darwin/arm64": {
    label: "darwin-arm64",
    triple: "aarch64-apple-darwin",
    asset: "idx-darwin-arm64",
  },
  "darwin/x64": {
    label: "darwin-x64",
    triple: "x86_64-apple-darwin",
    asset: "idx-darwin-x64",
  },
};

function currentTarget() {
  const key = `${process.platform}/${process.arch}`;
  const target = targets[key];
  if (!target) {
    const supported = Object.values(targets)
      .map(({ label, triple }) => `${label} (${triple})`)
      .join(", ");
    throw new Error(
      `unsupported platform ${key}; supported targets are ${supported}`,
    );
  }
  return target;
}

function releaseAssetUrl(assetName) {
  return `https://github.com/${githubRepository}/releases/download/v${packageManifest.version}/${assetName}`;
}

function ensureBinaryDirectory() {
  fs.mkdirSync(binaryDirectory, { recursive: true });
}

function temporaryBinaryPath() {
  return `${binaryPath}.${process.pid}.tmp`;
}

function replaceWithLocalBinary(sourcePath) {
  const resolvedSource = path.resolve(sourcePath);
  const sourceStat = fs.statSync(resolvedSource, { throwIfNoEntry: false });
  if (!sourceStat || !sourceStat.isFile()) {
    throw new Error(`local binary does not exist or is not a file: ${resolvedSource}`);
  }

  const temporaryPath = temporaryBinaryPath();
  try {
    fs.copyFileSync(resolvedSource, temporaryPath);
    fs.chmodSync(temporaryPath, 0o755);
    fs.renameSync(temporaryPath, binaryPath);
  } finally {
    if (fs.existsSync(temporaryPath)) {
      fs.unlinkSync(temporaryPath);
    }
  }
}

function request(url, onResponse, redirects = 0) {
  if (redirects > maxRedirects) {
    return Promise.reject(new Error(`too many redirects while downloading ${url}`));
  }

  let parsedUrl;
  try {
    parsedUrl = new URL(url);
  } catch (error) {
    return Promise.reject(new Error(`invalid download URL: ${error.message}`));
  }

  if (!["http:", "https:"].includes(parsedUrl.protocol)) {
    return Promise.reject(new Error(`unsupported download protocol: ${parsedUrl.protocol}`));
  }

  const client = parsedUrl.protocol === "https:" ? https : http;
  return new Promise((resolve, reject) => {
    const req = client.get(
      parsedUrl,
      {
        headers: {
          Accept: "application/octet-stream",
          "User-Agent": `idx-cli-npm/${packageManifest.version}`,
        },
      },
      (response) => {
        const status = response.statusCode || 0;
        if (status >= 300 && status < 400 && response.headers.location) {
          const redirectedUrl = new URL(response.headers.location, parsedUrl).toString();
          response.resume();
          request(redirectedUrl, onResponse, redirects + 1).then(resolve, reject);
          return;
        }

        if (status !== 200) {
          response.resume();
          reject(new Error(`download failed with HTTP ${status} for ${url}`));
          return;
        }

        onResponse(response).then(resolve, reject);
      },
    );
    req.setTimeout(requestTimeoutMs, () => {
      req.destroy(new Error(`timed out after ${requestTimeoutMs / 1000}s downloading ${url}`));
    });
    req.once("error", reject);
  });
}

function download(url, destination) {
  return request(
    url,
    (response) =>
      new Promise((resolve, reject) => {
        const output = fs.createWriteStream(destination, { mode: 0o755 });
        pipeline(response, output, (error) => (error ? reject(error) : resolve()));
      }),
  );
}

function downloadText(url) {
  return request(
    url,
    (response) =>
      new Promise((resolve, reject) => {
        const chunks = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.once("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
        response.once("error", reject);
      }),
  );
}

function sha256File(filePath) {
  return crypto.createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

async function expectedReleaseChecksum(assetName) {
  const sums = await downloadText(releaseAssetUrl("SHA256SUMS"));
  for (const line of sums.split(/\r?\n/)) {
    const [hash, name] = line.trim().split(/\s+\*?/);
    if (name === assetName && /^[0-9a-f]{64}$/i.test(hash)) {
      return hash.toLowerCase();
    }
  }
  throw new Error(`SHA256SUMS for v${packageManifest.version} has no entry for ${assetName}`);
}

function verifyChecksum(filePath, expected, label) {
  const actual = sha256File(filePath);
  if (actual !== expected.toLowerCase()) {
    throw new Error(`checksum mismatch for ${label}: expected ${expected}, got ${actual}`);
  }
}

async function replaceWithDownloadedBinary(url, expectedSha256) {
  const temporaryPath = temporaryBinaryPath();
  try {
    await download(url, temporaryPath);
    if (expectedSha256) {
      verifyChecksum(temporaryPath, expectedSha256, url);
    }
    fs.chmodSync(temporaryPath, 0o755);
    fs.renameSync(temporaryPath, binaryPath);
  } finally {
    if (fs.existsSync(temporaryPath)) {
      fs.unlinkSync(temporaryPath);
    }
  }
}

function localPathFromOverride(value) {
  if (value.startsWith("file:")) {
    const parsedUrl = new URL(value);
    if (parsedUrl.protocol !== "file:") {
      throw new Error(`unsupported local URL protocol: ${parsedUrl.protocol}`);
    }
    return fileURLToPath(parsedUrl);
  }

  if (!/^[a-z][a-z\d+.-]*:\/\//i.test(value)) {
    const candidate = path.resolve(value);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return null;
}

async function main() {
  const override = process.env.IDX_BINARY_URL;
  const overrideSha256 = process.env.IDX_BINARY_SHA256;

  // `npm install` inside the Rust checkout should not fetch a release binary.
  if (!override && fs.existsSync(path.join(packageRoot, "Cargo.toml"))) {
    console.log("idx-cli: source checkout detected; skipping binary download (set IDX_BINARY_URL to override)");
    return;
  }

  const target = currentTarget();
  const source = override || releaseAssetUrl(target.asset);

  ensureBinaryDirectory();
  if (override) {
    const localPath = localPathFromOverride(override);
    if (localPath) {
      if (overrideSha256) {
        verifyChecksum(localPath, overrideSha256, localPath);
      }
      replaceWithLocalBinary(localPath);
      console.log(`idx-cli: installed local binary from ${path.resolve(localPath)}`);
      return;
    }
    console.log(`idx-cli: downloading binary from IDX_BINARY_URL (${override})`);
  } else {
    console.log(`idx-cli: downloading ${target.asset} for ${target.label} from ${source}`);
  }

  const expectedSha256 = override
    ? overrideSha256
    : await expectedReleaseChecksum(target.asset);
  await replaceWithDownloadedBinary(source, expectedSha256);
  console.log(`idx-cli: installed binary at ${binaryPath}`);
}

main().catch((error) => {
  console.error(`idx-cli: ${error.message}`);
  process.exit(1);
});
