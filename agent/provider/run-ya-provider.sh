#!/bin/bash

usage() {
  cat 1>&2 <<EOF
Runs Yagna Service and ya-provider from given deb.
Installs two runtimes from github.

USAGE:
    $(basename $0) <yagna.deb>

Steps:
    1. Downloads and installs WASI and VM runtimes.
    2. Installs and (re)starts Yagna Service.
    3. Configures and runs ya-provider.
EOF
}

#############################################
## helpers, adapted from from sh.rustup.rs ##

get_install_dir() {
  basename $1 | sed -e 's/\(_amd64\)\?.deb$//' -e 's/^\(yagna\)\?/testnet/'
}

say() {
    printf 'yagna-up [%s]: %s\n' "$(date +'%Y-%m-%d %H:%M:%S')" "$@"
}

err() {
    say "$1" >&2
    exit 1
}

need_cmd() {
    if ! check_cmd "$1"; then
        err "need '$1' (command not found)"
    fi
}

check_cmd() {
    command -v "$1" > /dev/null 2>&1
}

# Run a command that should never fail.
ensure() {
    if ! "$@"; then err "command failed: $*"; fi
}

# To indicate that commands' results are being intentionally ignored.
ignore() {
    "$@"
}

#################
###  m a i n  ###
#################
main() {
    need_cmd mkdir
    need_cmd sed
    need_cmd wget
    need_cmd dpkg-deb

    if [[ "$#" -eq 0 ]]; then
      usage
      exit 1
    fi


    local testnet="testnet"
    local deb_file

    while [[ "$#" -gt 0 ]]; do
        case $1 in
            -h|--help) usage; exit 0 ;;
            -t) testnet="$2"; shift; shift ;;
            *) deb_file="$1"; shift; break ;;
        esac
    done

    if [[ "$#" -gt 0 ]]; then
      usage
      exit 2
    fi

    local install_dir="$(get_install_dir $deb_file)"

    if [[ ! -d "$install_dir" ]]; then
        say "Creating install dir $install_dir"
        ensure mkdir "$install_dir"
    else
        say "Using install dir $install_dir"
    fi

    cd "$install_dir"

    if [[ ! -f usr/lib/yagna/plugins/ya-runtime-wasi.json ]]; then
    	  say "Install WASI runtime"
        ensure wget https://github.com/golemfactory/ya-runtime-wasi/releases/download/v0.2.0/ya-runtime-wasi_0.2.0_amd64.deb
        dpkg-deb -R ya-runtime-wasi_0.2.0_amd64.deb .
        rm -rf DEBIAN
    fi


    if [[ ! -f usr/lib/yagna/plugins/ya-runtime-vm.json ]]; then
    	  say "Install VM runtime"
        ensure wget https://github.com/golemfactory/ya-runtime-vm/releases/download/pre-rel-1/ya-runtime-vm_0.1.0_amd64.deb
        dpkg-deb -R ya-runtime-vm_0.1.0_amd64.deb .
        rm -rf DEBIAN
    fi

    if [[ ! -f ./usr/bin/yagna ]]; then
        say "Install yagna"
        ensure dpkg-deb -R $deb_file .
        rm -rf DEBIAN
    fi

    local prov_dir="ya-prov"
    mkdir -p "$prov_dir"
    ensure cd "$prov_dir"

    if [[ ! -f .env ]]; then
        say "Configure ya-provider"

        ensure wget https://raw.githubusercontent.com/golemfactory/yagna/release/vm-poc/.env-template
	      ensure sed \
            -e "s|^#GSB_URL=tcp://127.0.0.1:7464|GSB_URL=tcp://127.0.0.1:17474|" \
            -e "s|^#YAGNA_API_URL=http://127.0.0.1:7465|YAGNA_API_URL=http://127.0.0.1:17475|" \
	          -e "s|__YOUR_NODE_NAME_GOES_HERE__|${USER}@${HOSTNAME}-${testnet}|" \
	          -e "s|EXE_UNIT_PATH=.*|EXE_UNIT_PATH=../usr/lib/yagna/plugins/ya-runtime-*.json|" \
	          -e "s|#SUBNET=1234567890|SUBNET=${testnet}|" \
            .env-template > .env
    fi

    local pid_file="yagna.pid"
    if [[ -f "$pid_file" ]]; then
	      say "Getting pid"
        local pid=$(cat "$pid_file")
	      say "Killing Yagna service ($pid)..."
        ignore kill "$pid"
        ignore rm -f "$pid_file"
        sleep 2s # wait a bit for service to finish cleanly
        say "Yagna service killed."
    fi

    say "Starting Yagna Service... (stdout & err in yagna.log)"
    RUST_LOG=debug ../usr/bin/yagna --accept-terms service run >> yagna.log 2>&1 &
    local pid="$!"
    echo "$pid" > "$pid_file"
    sleep 2s # wait a bit for service to start fully
    say "Yagna Service started ($pid)."

    if grep "__GENERATED_APP_KEY__" .env > /dev/null; then
        say "Generate app key and place it within .env"
	      ignore ../usr/bin/yagna app-key drop 'provider-agent'
	      APP_KEY=$(ensure ../usr/bin/yagna app-key create 'provider-agent')
	      if [[ -z "$APP_KEY" ]]; then
            err "App key not generated"
	      fi
        sed -e "s/__GENERATED_APP_KEY__/$APP_KEY/" -i.bckp .env
    fi

    say "Register provider's payment account"
    ensure ../usr/bin/yagna payment init -p

    # wait for other nodes (optional; only for decentralized market milestone 1)
    #sleep 10s
    if [[ ! -f presets.json ]]; then
        presets_json > presets.json
    fi

    say "Start the Provider Agent (stdout & err in ya-provider.log)"
    ignore ../usr/bin/ya-provider \
      --data-dir . \
      --exe-unit-path '../usr/lib/yagna/plugins/ya-runtime-*.json' \
      run 2>&1 | tee ya-provider.log
}

presets_json() {
    cat << EOF
{
  "active": [
    "vm"
    "wasmtime",
  ],
  "presets": [
    {
      "name": "vm",
      "exeunit-name": "vm",
      "pricing-model": "linear",
      "usage-coeffs": {
        "duration": 0.001,
        "cpu": 0.002,
        "initial": 0.0
      }
    },
    {
      "name": "wasmtime",
      "exeunit-name": "wasmtime",
      "pricing-model": "linear",
      "usage-coeffs": {
        "initial": 1.0,
        "cpu": 1.0,
        "duration": 0.1
      }
    }
  ]
}
EOF
}

main "$@"
