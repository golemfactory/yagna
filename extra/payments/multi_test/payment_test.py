import os
import subprocess
import time
import json
import shutil
import logging

logging.basicConfig(level=logging.DEBUG)
logger = logging.getLogger(__name__)


def run_command(command, working_dir=None):
    logger.info(f"Running command: {command}")
    command_array = command.split(" ")
    logger.info(f"Command array: {command_array}")
    p = subprocess.Popen(command_array, cwd=working_dir, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    out, err = p.communicate()
    rc = p.returncode
    logger.info(f"Command output: {out}")
    logger.info(f"Command error: {err}")
    logger.info(f"Command return code: {rc}")
    return out, err, rc


def run_command_output_log(command, working_dir=None):
    logger.info(f"Running command: {command}")
    command_array = command.split(" ")
    logger.info(f"Command array: {command_array}")
    subprocess.run(command_array, cwd=working_dir)


yagna = None
processor = None


def prepare():
    logger.info("Preparing test environment...")

    with open(".env", "w") as f:
        f.write("RUST_LOG=info,ya_relay_client=error,ya_net=error\n")
        f.write("YAGNA_DATADIR=datadir\n")

    if os.path.exists("datadir"):
        shutil.rmtree("datadir")
        # operating system need time to propagate events that folder not exists (at least Windows)
        time.sleep(1)

    os.mkdir("datadir")

    if os.path.exists("processor"):
        shutil.rmtree("processor")
        # operating system need time to propagate events that folder not exists (at least Windows)
        time.sleep(1)

    os.mkdir("processor")

    global yagna
    global processor
    yagna = shutil.which("yagna")
    processor = shutil.which("erc20_processor")

    if yagna is None or processor is None:
        subprocess.run(["cargo", "build", "-p", "erc20_processor", "-p", "yagna"])

        if os.name == "nt":
            yagna = "..\\..\\..\\target\\debug\\yagna.exe"
            processor = "..\\..\\..\\..\\target\\debug\\erc20_processor.exe"
        else:
            yagna = "../../../target/debug/yagna"
            processor = "../../../../target/debug/erc20_processor"


def create_keys():
    logger.info("Creating keys...")

    env_file, _, _ = run_command(f"{processor} generate-key -n 10", working_dir="processor")
    public_addrs = []
    for line in env_file.decode("utf-8").split("\n"):
        print(line)
        if "ETH_ADDRESS_" in line:
            address = line.split(":")[1].strip().lower()
            public_addrs.append(address)
        if line.startswith("ETH_PRIVATE_KEYS="):
            keys = line.split("=")[1].split(",")
            print("private keys: ", keys)
            break

    return env_file, keys, public_addrs


def create_account_and_fund(eth_private_key, eth_public_key):
    logger.info("Creating account and funding...")

    output, _, _ = run_command(f"{yagna} id create --from-private-key {eth_private_key} --no-password")
    print(output)

    output, _, _ = run_command(f"{yagna} payment fund --account {eth_public_key} --json")

    output, _, _ = run_command(f"{yagna} id list --json")
    output_json = json.loads(output)
    print(output_json)


def block_account(eth_public_key):
    logger.info("Blocking account...")

    faucet_address = "0x5b984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f"

    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x00984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x01984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x02984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x03984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x04984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x05984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x06984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x07984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x08984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x09984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x10984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x11984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x12984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x12984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 11 --to-address 0x13984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f")
    res = run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 1100 --to-address {faucet_address}")
    print(res)


def transfer(eth_public_key, transfer_to):
    logger.info("Transferring funds...")

    res = run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 100 --to-address {transfer_to}")
    print(res)


def append_return_funds(eth_public_key):
    logger.info("Returning funds...")

    faucet_address = "0x5b984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f"

    res = run_command(
        f"{processor} transfer --address {eth_public_key} --all --recipient {faucet_address}",
        working_dir="processor")
    print(res)


def get_balance():
    logger.info("Getting balance...")

    res = run_command(
        f"{processor} balance",
        working_dir="processor")
    return json.loads(res)


def process_erc20():
    run_command_output_log(
        f"{processor} run",
        working_dir="processor")


if __name__ == "__main__":
    prepare()
    env_file, keys, public_addrs = create_keys()
    with open("processor/.env", "w") as f:
        f.write(env_file.decode("utf-8"))

    pr = subprocess.Popen([yagna, "service", "run"])
    time.sleep(10)

    create_account_and_fund(keys[0], public_addrs[0])

    time.sleep(100)

    block_account(public_addrs[0])

    time.sleep(100)

    create_account_and_fund(keys[1], public_addrs[1])

    time.sleep(100)

    transfer(public_addrs[1], public_addrs[0])

    time.sleep(200)

    subprocess.run([yagna, "service", "shutdown"])

    pr.wait(timeout=60)

    append_return_funds(public_addrs[0])
    append_return_funds(public_addrs[1])
    process_erc20()

    balance = get_balance()
    logger.info(f"Balance: {balance}")


