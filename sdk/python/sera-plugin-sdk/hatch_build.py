"""Hatch custom build hook that invokes ``grpc_tools.protoc`` at build time.

SPEC-plugins §5.1 keeps the ``.proto`` files in ``rust/proto/plugin/`` as the
canonical wire contract. This hook compiles those protos into Python modules
under ``src/sera_plugin_sdk/_generated/`` during the Hatch build. The hook is
tolerant of missing protos so editable installs (``pip install -e .``) work in
a standalone checkout; in that case the gRPC transport remains importable but
the capability servicers raise ``UNIMPLEMENTED`` if invoked.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from hatchling.builders.hooks.plugin.interface import BuildHookInterface

PROTO_SRC = Path("../../../rust/proto/plugin")
OUT_DIR = Path("src/sera_plugin_sdk/_generated")


class ProtocBuildHook(BuildHookInterface):
    PLUGIN_NAME = "custom"

    def initialize(self, version: str, build_data: dict[str, Any]) -> None:
        root = Path(self.root)
        proto_dir = (root / PROTO_SRC).resolve()
        out_dir = root / OUT_DIR

        out_dir.mkdir(parents=True, exist_ok=True)
        init_file = out_dir / "__init__.py"
        if not init_file.exists():
            init_file.write_text(
                '"""Auto-generated protobuf / gRPC modules. Do not edit by hand."""\n'
            )

        if not proto_dir.is_dir():
            print(
                f"[sera-plugin-sdk] WARNING: proto source {proto_dir} absent — "
                "skipping codegen."
            )
            return

        proto_files = sorted(proto_dir.glob("*.proto"))
        if not proto_files:
            print(
                f"[sera-plugin-sdk] WARNING: no .proto files found in {proto_dir} — "
                "skipping codegen."
            )
            return

        try:
            import grpc_tools
            from grpc_tools import protoc
        except ImportError:
            print(
                "[sera-plugin-sdk] WARNING: grpc_tools is not installed — "
                "skipping codegen. Install with `pip install grpcio-tools`."
            )
            return

        # grpc_tools ships the well-known protos (google/protobuf/*.proto)
        # under its package directory; add that to the import path so that
        # ``import "google/protobuf/empty.proto"`` resolves.
        wkt_include = Path(grpc_tools.__file__).parent / "_proto"

        args = [
            "grpc_tools.protoc",
            f"--proto_path={proto_dir}",
            f"--proto_path={wkt_include}",
            f"--python_out={out_dir}",
            f"--grpc_python_out={out_dir}",
            *[str(p) for p in proto_files],
        ]
        rc = protoc.main(args)
        if rc != 0:
            raise SystemExit(f"protoc failed with exit code {rc}")

        # Rewrite `import foo_pb2` to `from . import foo_pb2` so the generated
        # modules resolve within the _generated package rather than the top
        # level import path.
        for f in out_dir.glob("*_pb2*.py"):
            text = f.read_text()
            text = re.sub(
                r"^import (\w+_pb2)(\s|$)",
                r"from . import \1\2",
                text,
                flags=re.MULTILINE,
            )
            f.write_text(text)

        print(
            f"[sera-plugin-sdk] Generated {len(proto_files)} proto module(s) "
            f"into {out_dir}"
        )
