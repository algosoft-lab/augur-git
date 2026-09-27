#!/usr/bin/env node
// Build the `augurgit-tauri` CLI companion and place it where the Tauri
// bundler expects a sidecar for the host target.
//
// Tauri runs this as `beforeBuildCommand` / `beforeDevCommand`, so both
// `tauri build` and `tauri dev` ship the real companion binary. A bare
// `cargo build` skips it and gets a clearly labelled placeholder instead.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const profile = process.env.TAURI_ENV_DEBUG ? "debug" : "release";
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host:"))
  ?.split(" ")[1];

if (!triple) {
  console.error("sidecar: could not determine the host target triple");
  process.exit(1);
}

const manifest = join(root, "src-tauri", "Cargo.toml");
const cargoArgs = ["build", "--manifest-path", manifest, "--bin", "augurgit-tauri"];
if (profile === "release") {
  cargoArgs.push("--release");
}

try {
  execFileSync("cargo", cargoArgs, { stdio: "inherit", cwd: root });
} catch {
  console.error("sidecar: the CLI companion failed to build");
  process.exit(1);
}

const source = join(
  root,
  "src-tauri",
  "target",
  profile,
  process.platform === "win32" ? "augurgit-tauri.exe" : "augurgit-tauri",
);
if (!existsSync(source)) {
  console.error(`sidecar: expected the binary at ${source}`);
  process.exit(1);
}

const targetDir = join(root, "src-tauri", "binaries");
mkdirSync(targetDir, { recursive: true });
const destination = join(
  targetDir,
  process.platform === "win32"
    ? `augurgit-tauri-${triple}.exe`
    : `augurgit-tauri-${triple}`,
);
copyFileSync(source, destination);
console.log(`sidecar: installed ${destination}`);
