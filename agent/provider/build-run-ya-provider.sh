#!/bin/bash

INSTALL_DIR=~/.yagna

usage() {
  cat 1>&2 <<EOF
Runs Yagna Service and ya-provider from sources.

USAGE:
    $(basename $0) <install_dir>

Files are being placed in '$INSTALL_DIR' by default.
You can pass another location as a first argument.


Steps:
    1. Installs or updates rust
    2. Clones and builds ya-runtime-wasi
    3. Clones and builds yagna
    4. Runs Yagna Service
    5. Configures and runs ya-provider.
EOF
}

#############################################
## helpers, adapted from from sh.rustup.rs ##

say() {
    printf 'yagna-up[%s]: %s\n' "$(date +'%Y-%m-%dT%H:%M:%S%z')" "$@"
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
    need_cmd grep
    need_cmd curl
    need_cmd git

    if [[ "$1" == "-h" ]]; then
        usage
        exit 0
    fi

    install_dir=${1:-$INSTALL_DIR}

    if ! check_cmd rustup; then
        say "install rust"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	      export PATH="$HOME/.cargo/bin:$PATH"
    fi
    rustup update

    if [[ ! -d "$install_dir" ]]; then
        say "creating install dir $install_dir"
        mkdir "$install_dir"
    fi

    cd "$install_dir"

    if [[ ! -d ya-runtime-wasi ]]; then
    	  say "clone WASI runtime"
        ensure git clone https://github.com/golemfactory/ya-runtime-wasi.git
    fi

    cd ya-runtime-wasi
    git pull
    say "build WASI runtime"
    ensure cargo build
    cd ..

    if [[ ! -d yagna ]]; then
        say "clone main yagna repo"
        ensure git clone https://github.com/golemfactory/yagna.git
    fi

    cd yagna
    git pull
    #git checkout exe-unit/init-logs

    say "build ExeUnit supervisor, Provider and Yagna (with decentralized marketplace)"
    ensure cargo build -p ya-exe-unit -p ya-provider -p yagna \
        --features market-decentralized --no-default-features

    local prov_dir="ya-prov"

    mkdir -p "$prov_dir"
    if [[ ! -f "$prov_dir/.env" ]]; then
        say "configure ya-provider"

        cp .env-template "$prov_dir/.env"
	      sed \
            -e "s|#GSB_URL=tcp://127.0.0.1:7464|GSB_URL=tcp://127.0.0.1:17474|" \
            -e "s|#YAGNA_API_URL=http://127.0.0.1:7465|YAGNA_API_URL=http://127.0.0.1:17475|" \
	          -e "s|__YOUR_NODE_NAME_GOES_HERE__|${USER}@${HOSTNAME}-ya-mkt-dece|" \
            -i.bckp "$prov_dir/.env"
    fi

    ensure cd "$prov_dir"

    local pid_file="yagna.pid"
    if [[ -f "$pid_file" ]]; then
	      say "getting pid"
        local pid=$(cat "$pid_file")
	      say "killing Yagna service ($pid)..."
        ignore kill "$pid"
        sleep 2s # wait a bit for service to finish cleanly
        say "Yagna service killed."
    fi

    say "starting Yagna Service... (stdout & err in yagna.log)"
    ../target/debug/yagna service run >> yagna.log 2>&1 &
    echo "$!" > "$pid_file"
    local pid=$(cat "$pid_file")
    sleep 2s # wait a bit for service to start fully
    say "Yagna Service started ($pid)."

    if grep "__GENERATED_APP_KEY__" .env > /dev/null; then
        say "generate app key and place it within .env"
	      ignore ../target/debug/yagna app-key drop 'provider-agent'
	      APP_KEY=$(ensure ../target/debug/yagna app-key create 'provider-agent')
	      if [[ -z "$APP_KEY" ]]; then
            err "app key not generated"
	      fi
        sed -e "s/__GENERATED_APP_KEY__/$APP_KEY/" -i.bckp .env
    fi

    say "register provider's payment account"
    ensure ../target/debug/yagna payment init gnt -p

    sleep 10s # wait for other nodes (optional)

    say "start the Provider Agent"
    ignore ../target/debug/ya-provider --data-dir . run
}

main "$@"
