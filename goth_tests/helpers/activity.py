"""Activity helpers."""

import os
from pathlib import Path

from goth.runner import Runner


wasi_task_package: str = (
    "hash://sha3:d5e31b2eed628572a5898bf8c34447644bfc4b5130cfc1e4f10aeaa1:"
    "http://yacn2.dev.golem.network:8000/rust-wasi-tutorial.zip"
)

vm_task_package: str = (
    "hash:sha3:9a3b5d67b0b27746283cb5f287c13eab1beaa12d92a9f536b747c7ae:"
    "http://yacn2.dev.golem.network:8000/local-image-c76719083b.gvmi"
)


def vm_exe_script(runner: Runner, output_file: str = "output.png"):
    """VM exe script builder."""
    """Create a VM exe script for running a Blender task."""

    output_path = Path(runner.web_root_path) / output_file
    if output_path.exists():
        os.remove(output_path)

    web_server_addr = f"http://{runner.host_address}:{runner.web_server_port}"

    return [
        {"deploy": {}},
        {"start": {}},
        {
            "transfer": {
                "from": f"{web_server_addr}/scene.blend",
                "to": "container:/golem/resource/scene.blend",
            }
        },
        {
            "transfer": {
                "from": f"{web_server_addr}/params.json",
                "to": "container:/golem/work/params.json",
            }
        },
        {"run": {"entry_point": "/golem/entrypoints/run-blender.sh", "args": []}},
        {
            "transfer": {
                "from": f"container:/golem/output/{output_file}",
                "to": f"{web_server_addr}/upload/{output_file}",
            }
        },
    ]


def wasi_exe_script(runner: Runner, output_file: str = "upload_file"):
    """WASI exe script builder."""
    """Create a WASI exe script for running a WASI tutorial task."""

    output_path = Path(runner.web_root_path) / output_file
    if output_path.exists():
        os.remove(output_path)

    web_server_addr = f"http://{runner.host_address}:{runner.web_server_port}"

    return [
        {"deploy": {}},
        {"start": {"args": []}},
        {
            "transfer": {
                "from": f"{web_server_addr}/params.json",
                "to": "container:/input/file_in",
            }
        },
        {
            "run": {
                "entry_point": "rust-wasi-tutorial",
                "args": ["/input/file_in", "/output/file_cp"],
            }
        },
        {
            "transfer": {
                "from": "container:/output/file_cp",
                "to": f"{web_server_addr}/upload/{output_file}",
            }
        },
    ]
