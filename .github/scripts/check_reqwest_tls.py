"""Keep production reqwest clients on Rustls with Mozilla roots on every target."""

import subprocess
from pathlib import Path


def main():
    tree = subprocess.check_output(
        [
            "cargo", "tree", "--locked",
            "-p", "yagna", "-p", "ya-exe-unit", "-p", "ya-erc20-driver",
            "--features", "require-consent,static-openssl",
            "--edges", "normal,build", "--target", "all",
            "--invert", "reqwest", "--prefix", "none", "--format", "{p}|{f}",
        ],
        cwd=Path(__file__).resolve().parents[2],
        text=True,
    )
    clients = [line for line in tree.splitlines() if line.startswith("reqwest v")]
    if not clients:
        raise SystemExit("No production reqwest dependency found")
    for client in clients:
        package, enabled = client.split("|", 1)
        features = set(enabled.split(","))
        forbidden = features & {
            "default-tls", "native-tls", "native-tls-alpn", "native-tls-vendored",
            "rustls-tls-native-roots", "rustls-tls-native-roots-no-provider",
        }
        if forbidden or "rustls-tls-webpki-roots" not in features:
            raise SystemExit(f"Unexpected TLS configuration for {package}: {enabled}")
        print(f"{package}: Rustls with Mozilla roots; no native TLS or native roots")


if __name__ == "__main__":
    main()
