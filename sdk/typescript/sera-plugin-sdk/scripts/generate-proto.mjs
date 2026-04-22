#!/usr/bin/env node
// Generate TypeScript stubs from rust/proto/plugin/*.proto using protoc + ts-proto.
//
// Runs at build time (`prebuild` script). If the proto directory is absent
// (e.g. the protos haven't landed yet in this branch, or an external consumer
// is installing the npm tarball which ships pre-generated stubs), we emit an
// empty placeholder and keep going — the SDK's authored types and JSON-RPC
// stdio transport still compile and work without generated gRPC stubs.
//
// When the protos are present, we shell out to `protoc`. We prefer the
// ts-proto plugin if it's on PATH; otherwise we fall back to the bundled
// descriptor-set output so at least the raw descriptor is available.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = resolve(here, "..");
const genDir = join(pkgRoot, "src", "generated");
const protoDir = resolve(pkgRoot, "..", "..", "..", "rust", "proto", "plugin");

function ensureDir(p) {
  mkdirSync(p, { recursive: true });
}

function writePlaceholder(reason) {
  ensureDir(genDir);
  const banner = `// AUTO-GENERATED placeholder — no proto stubs available.\n// Reason: ${reason}\n// The SDK's stdio transport and authored capability types still work without this.\n// Regenerate with: npm run proto\n\nexport const PROTO_STUBS_AVAILABLE = false as const;\n`;
  writeFileSync(join(genDir, "index.ts"), banner);
}

function hasBinary(name) {
  try {
    execFileSync(name, ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function main() {
  if (!existsSync(protoDir)) {
    console.warn(`[proto] ${protoDir} does not exist — writing placeholder.`);
    writePlaceholder(`${protoDir} not found`);
    return;
  }

  const protos = readdirSync(protoDir).filter((f) => f.endsWith(".proto"));
  if (protos.length === 0) {
    console.warn(`[proto] no .proto files in ${protoDir} — writing placeholder.`);
    writePlaceholder("no .proto files found");
    return;
  }

  if (!hasBinary("protoc")) {
    console.warn("[proto] protoc not found on PATH — writing placeholder.");
    writePlaceholder("protoc binary missing");
    return;
  }

  ensureDir(genDir);

  const tsProtoPlugin = process.env.TS_PROTO_PLUGIN ??
    resolve(pkgRoot, "node_modules", ".bin", "protoc-gen-ts_proto");

  if (existsSync(tsProtoPlugin)) {
    const args = [
      `--plugin=protoc-gen-ts_proto=${tsProtoPlugin}`,
      `--ts_proto_out=${genDir}`,
      "--ts_proto_opt=esModuleInterop=true,outputServices=grpc-js,useExactTypes=true,env=node",
      `--proto_path=${protoDir}`,
      ...protos.map((p) => join(protoDir, p)),
    ];
    execFileSync("protoc", args, { stdio: "inherit" });
    console.log(`[proto] generated TS stubs for ${protos.length} proto(s) via ts-proto.`);
    return;
  }

  // Fallback: emit a binary descriptor set so runtime loaders can still read it.
  console.warn("[proto] ts-proto plugin not installed — emitting descriptor set fallback.");
  const descOut = join(genDir, "plugin.desc");
  execFileSync("protoc", [
    `--descriptor_set_out=${descOut}`,
    "--include_imports",
    "--include_source_info",
    `--proto_path=${protoDir}`,
    ...protos.map((p) => join(protoDir, p)),
  ], { stdio: "inherit" });
  writePlaceholder("ts-proto plugin missing; descriptor set only — see plugin.desc");
}

main();
