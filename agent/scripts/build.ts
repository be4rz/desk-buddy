// Compile the sidecar to one binary: `dist/buddy-agent` (gitignored).
// `BUDDY_AGENT_OUT` overrides the path, `BUDDY_AGENT_TARGET` the bun target.

import { mkdirSync, mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const root = join(import.meta.dir, "..");
// Absolute: bun runs in a scratch folder below.
const out = resolve(process.env.BUDDY_AGENT_OUT ?? join(root, "dist", "buddy-agent"));
const target = process.env.BUDDY_AGENT_TARGET;
mkdirSync(dirname(out), { recursive: true });
const args = ["build", join(root, "src", "main.ts"), "--compile", "--minify-syntax", "--outfile", out, ...(target ? [`--target=${target}`] : [])];
const started = Date.now();
// `--compile` leaves a full-size `.*.bun-build` temp file in its cwd: give it a scratch one.
const scratch = mkdtempSync(join(tmpdir(), "buddy-agent-build-"));
const p = Bun.spawnSync(["bun", ...args], { cwd: scratch, stdio: ["ignore", "inherit", "inherit"] });
rmSync(scratch, { recursive: true, force: true });
if (p.exitCode !== 0) process.exit(p.exitCode ?? 1);
console.log(`built ${out} (${(statSync(out).size / 1e6).toFixed(1)} MB) in ${Date.now() - started} ms`);
