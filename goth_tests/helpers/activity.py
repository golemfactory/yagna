"""Activity helpers."""

import json
import os
from pathlib import Path

from goth.runner import Runner
from goth.runner.probe import ProviderProbe, RequestorProbe


wasi_task_package: str = (
    "hash://sha3:d5e31b2eed628572a5898bf8c34447644bfc4b5130cfc1e4f10aeaa1:"
    "http://yacn2.dev.golem.network:8000/rust-wasi-tutorial.zip"
)

wasi_sleeper_task_package: str = (
    "hash://sha3:eef139fcbc7691b446e43dd0575dfed052eb751c7862eea69c764544:"
    "http://yacn2.dev.golem.network:8000/rust-wasi-sleeper.zip"
)

vm_task_package: str = (
    "hash:sha3:9a3b5d67b0b27746283cb5f287c13eab1beaa12d92a9f536b747c7ae:"
    "http://yacn2.dev.golem.network:8000/local-image-c76719083b.gvmi"
)


async def run_activity(
    requestor: RequestorProbe,
    provider: ProviderProbe,
    agreement_id: str,
    exe_script: dict,
):
    """Run single Activity from start to end."""

    activity_id = await requestor.create_activity(agreement_id)
    await provider.wait_for_exeunit_started()

    batch_id = await requestor.call_exec(activity_id, json.dumps(exe_script))
    await requestor.collect_results(activity_id, batch_id, len(exe_script), timeout=30)

    await requestor.destroy_activity(activity_id)
    await provider.wait_for_exeunit_finished()


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
def vm_exe_script_outbound(runner: Runner, output_file: str = "output.txt"):
    """VM exe script builder."""
    """Create a VM exe script for running a outbound task."""

    output_path = Path(runner.web_root_path) / output_file
    if output_path.exists():
        os.remove(output_path)

    web_server_addr = f"http://{runner.host_address}:{runner.web_server_port}"

    return [
        {"deploy": {}},
        {"start": {}},
        {"run": {"entry_point": "/golem/entrypoints/request.sh", "args": []}},
        {
            "transfer": {
                "from": f"container:/golem/output/output.txt",
                "to": f"{web_server_addr}/upload/{output_file}",
            }
        },
    ]


def wasi_sleeper_exe_script(duration: float = 10.0):
    """WASI exe script builder."""
    """Create a WASI exe script for running a WASI sleeper task."""

    return [
        {"deploy": {}},
        {"start": {"args": []}},
        {
            "run": {
                "entry_point": "rust-wasi-sleeper",
                "args": [f"{duration}"],
            }
        },
    ]
