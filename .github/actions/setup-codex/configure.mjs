import { appendFileSync, existsSync, readdirSync } from "node:fs";
import path from "node:path";

const prefix = path.resolve(process.env.CODEX_PREFIX);
const launcher = process.env.CODEX_LAUNCHER;
let bin;
if (launcher === "npm") {
  bin = process.platform === "win32" ? prefix : path.join(prefix, "bin");
} else if (launcher === "native") {
  const modules = process.platform === "win32"
    ? path.join(prefix, "node_modules")
    : path.join(prefix, "lib", "node_modules");
  const codex = path.join(modules, "@openai", "codex");
  const platformPackage = `codex-${process.platform}-${process.arch}`;
  // npm may nest or hoist optional platform packages. Older Codex packages
  // instead keep the native binary in the main package's vendor directory.
  const vendors = [
    path.join(codex, "node_modules", "@openai", platformPackage, "vendor"),
    path.join(modules, "@openai", platformPackage, "vendor"),
    path.join(codex, "vendor"),
  ];
  const filename = process.platform === "win32" ? "codex.exe" : "codex";
  const binaries = [];
  function find(directory) {
    if (!existsSync(directory)) return;
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) find(file);
      else if (entry.isFile() && entry.name === filename) binaries.push(file);
    }
  }
  for (const vendor of vendors) find(vendor);
  if (binaries.length !== 1) {
    throw new Error(`Expected one packaged native Codex binary, found ${binaries.length}.`);
  }
  bin = path.dirname(binaries[0]);
} else {
  throw new Error("Codex launcher must be npm or native.");
}

const filename = process.platform === "win32" && launcher === "npm" ? "codex.cmd"
  : process.platform === "win32" ? "codex.exe" : "codex";
if (!existsSync(path.join(bin, filename))) throw new Error("Codex launcher was not installed.");
appendFileSync(process.env.GITHUB_PATH, `${bin}\n`);
appendFileSync(process.env.GITHUB_OUTPUT, `bin-path=${bin}\n`);
