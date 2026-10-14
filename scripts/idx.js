#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const packageManifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"),
);
const nativeBinary = packageManifest.nativeBinary || "idx";
const packageName = packageManifest.name;
const binaryName = process.platform === "win32" ? `${nativeBinary}.exe` : nativeBinary;
const binaryPath = path.join(__dirname, "..", "bin", binaryName);

if (!fs.existsSync(binaryPath)) {
  console.error(`${packageName}: installed binary not found at ${binaryPath}`);
  console.error(`${packageName}: rerun npm install or set IDX_BINARY_URL for a local binary`);
  process.exit(1);
}

const child = spawn(binaryPath, process.argv.slice(2), { stdio: "inherit" });

// Pass termination signals from a supervisor (kill, docker stop) to the binary.
for (const signal of ["SIGTERM", "SIGHUP"]) {
  process.on(signal, () => child.kill(signal));
}
// Ctrl+C already reaches the binary through the terminal's process group.
process.on("SIGINT", () => {});

child.once("error", (error) => {
  console.error(`${packageName}: failed to start ${binaryPath}: ${error.message}`);
  process.exit(1);
});

child.once("exit", (code, signal) => {
  if (signal) {
    process.removeAllListeners(signal);
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code === null ? 1 : code);
});
