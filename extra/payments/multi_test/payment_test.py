import os
import subprocess
import time
import json
import shutil
import logging

logging.basicConfig(level=logging.DEBUG)
logger = logging.getLogger(__name__)


def run_command(command):
    logger.info(f"Running command: {command}")
    p = subprocess.Popen(command.split(" "), shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    out, err = p.communicate()
    rc = p.returncode
    return out, err, rc


yagna = None
processor = None


def prepare():
    logger.info("Preparing test environment...")

    with open(".env", "w") as f:
        f.write("RUST_LOG=info,ya_relay_client=error,ya_net=error\n")
        f.write("YAGNA_DATADIR=data_dir\n")

    if os.path.exists("datadir"):
        shutil.rmtree("datadir")
        # operating system need time to propagate events that folder not exists (at least Windows)
        time.sleep(1)

    os.mkdir("datadir")

    global yagna
    global processor
    yagna = shutil.which("yagna")
    processor = shutil.which("erc20_processor")

    if yagna is None or processor is None:
        subprocess.run(["cargo", "build", "-p", "erc20_processor", "-p", "yagna"], shell=True)

        if os.name == "nt":
            yagna = "..\\..\\..\\target\\debug\\yagna.exe"
            processor = "..\\..\\..\\target\\debug\\erc20_processor.exe"
        else:
            yagna = "../../../target/debug/yagna"
            processor = "../../../target/debug/erc20_processor"


def create_keys():
    logger.info("Creating keys...")

    env_file, _, _ = run_command(f"{processor} generate-key -n 10")
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

    res = run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 1100 --to-address {faucet_address}")
    print(res)

    time.sleep(80)


def transfer(eth_public_key, transfer_to):
    logger.info("Transferring funds...")

    res = run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 100 --to-address {transfer_to}")
    print(res)

    time.sleep(80)


if __name__ == "__main__":
    prepare()
    env_file, keys, public_addrs = create_keys()

    pr = subprocess.Popen([yagna, "service", "run"], shell=True)
    time.sleep(10)

    create_account_and_fund(keys[0], public_addrs[0])

    time.sleep(100)

    block_account(public_addrs[0])

    time.sleep(100)

    create_account_and_fund(keys[1], public_addrs[1])

    time.sleep(100)

    transfer(public_addrs[1], public_addrs[0])

    time.sleep(200)

    subprocess.run([yagna, "service", "shutdown"], shell=True)

    pr.wait(timeout=60)
