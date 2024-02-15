import subprocess
import time
import json


def run_command(command):
    p = subprocess.Popen(command.split(" "), shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    out, err = p.communicate()
    rc = p.returncode
    return out, err, rc


def run_test():
    subprocess.run(["cargo", "build"], shell=True)
    subprocess.run(["cargo", "build", "-p", "erc20_processor"], shell=True)

    with open(".env", "w") as f:
        f.write("RUST_LOG=info,ya_relay_client=error,ya_net=error\n")
        f.write("YAGNA_DATADIR=data\n")

    yagna = "..\\..\\..\\target\\debug\\yagna.exe"
    processor = "..\\..\\..\\target\\debug\\erc20_processor.exe"

    pr = subprocess.Popen([yagna, "service", "run"], shell=True)

    keys, _, _ = run_command(f"{processor} generate-key -n 10")
    public_addrs = []
    for line in keys.decode("utf-8").split("\n"):
        print(line)
        if "ETH_ADDRESS_" in line:
            address = line.split(":")[1].strip().lower()
            public_addrs.append(address)
        if line.startswith("ETH_PRIVATE_KEYS="):
            keys = line.split("=")[1].split(",")
            print("private keys: ", keys)
            break

    print(keys)
    time.sleep(10)

    eth_private_key = keys[0]
    eth_public_key = public_addrs[0]
    output, _, _ = run_command(f"{yagna} id create --from-private-key {eth_private_key} --no-password")
    print(output)

    output, _, _ = run_command(f"{yagna} payment fund --account {eth_public_key} --json")

    output, _, _ = run_command(f"{yagna} id list --json")
    output_json = json.loads(output)
    print(output_json)

    faucet_address = "0x5b984629E2Cc7570cBa7dD745b83c3dD23Ba6d0f"

    res = run_command(f"{yagna} payment transfer --account {eth_public_key} --amount 1000 --to-address {faucet_address}")
    print(res)

    time.sleep(80)

    subprocess.run([yagna, "service", "shutdown"], shell=True)

    pr.wait(timeout=30)




if __name__ == "__main__":
    run_test()