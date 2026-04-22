import { defineConfig } from "tsup";

export default defineConfig({
  entry: {
    index: "src/index.ts",
    "transport/stdio": "src/transport/stdio.ts",
    "transport/grpc": "src/transport/grpc.ts",
  },
  format: ["esm"],
  target: "node20",
  platform: "node",
  dts: true,
  sourcemap: true,
  clean: true,
  splitting: false,
  treeshake: true,
});
