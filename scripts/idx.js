#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const packageManifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"),
);
const packageName = packageManifest.name;
const installer = path.join(__dirname, "install.js");
// Requiring the installer only reads its paths; it installs when run directly.
const { binaryLocations } = require(installer);

const findBinary = () => binaryLocations.find((candidate) => fs.existsSync(candidate));

// The running child: the first-run installer, then the binary.
let child = null;
// A termination signal received while installing; the installer cleans up
// and exits, and the launcher then stops the same way.
let stopping = null;
let installing = false;

// Pass termination signals from a supervisor (kill, docker stop) to the child.
for (const signal of ["SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    stopping = signal;
    if (child) {
      child.kill(signal);
    } else {
      process.exit(1);
    }
  });
}
// Ctrl+C already reaches the binary through the terminal's process group, so
// the launcher ignores SIGINT then. During the first-run install, forward it
// too: a supervisor may signal only the launcher's PID.
process.on("SIGINT", () => {
  if (installing) {
    stopping = "SIGINT";
    child.kill("SIGINT");
  }
});

function exitLike(code, signal) {
  if (signal) {
    process.removeAllListeners(signal);
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code === null ? 1 : code);
}

function run(binaryPath) {
  child = spawn(binaryPath, process.argv.slice(2), { stdio: "inherit" });
  child.once("error", (error) => {
    console.error(`${packageName}: failed to start ${binaryPath}: ${error.message}`);
    process.exit(1);
  });
  child.once("exit", exitLike);
}

const installed = findBinary();
if (installed) {
  run(installed);
} else {
  // postinstall never ran (npm 12 `allow-scripts`, pnpm, bun, --ignore-scripts)
  // or its download failed: install now. Its log goes to stderr, since stdout
  // is the MCP channel for idx-mcp.
  console.error(`${packageName}: binary not installed; running the installer`);
  installing = true;
  child = spawn(process.execPath, [installer], { stdio: ["ignore", 2, 2] });
  child.once("error", (error) => {
    console.error(`${packageName}: failed to run the installer: ${error.message}`);
    process.exit(1);
  });
  child.once("exit", (code, signal) => {
    child = null;
    installing = false;
    if (signal || stopping) {
      exitLike(code, signal || stopping);
      return;
    }
    const binaryPath = findBinary();
    if (!binaryPath) {
      console.error(`${packageName}: no installed binary at ${binaryLocations.join(" or ")}`);
      console.error(
        `${packageName}: check access to github.com and rerun, or set IDX_BINARY_URL to a local binary`,
      );
      process.exit(1);
    }
    run(binaryPath);
  });
}
