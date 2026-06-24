#!/usr/bin/env bash
set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE_NAME="${CAPSULE_IMAGE_NAME:-capsule-runtime:latest}"
RUNTIME_URL="${CAPSULE_RUNTIME_URL:-http://localhost:3001}"
FRONTEND_URL="${CAPSULE_FRONTEND_URL:-http://localhost:3002}"
OBSERVAL_COMPOSE_FILE="${OBSERVAL_COMPOSE_FILE:-docker-compose.observal.yml}"

backend_pid=""
frontend_pid=""
initial_containers=""
started_observal=0

log() {
    printf '[capsule] %s\n' "$*"
}

require_command() {
    if command -v "$1" >/dev/null 2>&1; then
        return
    fi

    log "missing required command: $1"
    exit 1
}

snapshot_containers() {
    docker ps -a --filter name='capsule-' --format '{{.Names}}' | sort || true
}

load_env_file() {
    if [[ ! -f "$ROOT_DIR/.env" ]]; then
        return
    fi

    local key value
    while IFS='=' read -r key value; do
        case "$key" in
            OBSERVAL_API_URL|OBSERVAL_WEB_URL|OBSERVAL_CONTAINER_URL|OBSERVAL_COMPOSE_FILE|CAPSULE_CONTAINER_NETWORK|CAPSULE_OBSERVAL|CAPSULE_OBSERVAL_STOP)
                value="${value%$'\r'}"
                value="${value%\"}"
                value="${value#\"}"
                export "$key=$value"
                ;;
        esac
    done < "$ROOT_DIR/.env"
}

is_local_url() {
    case "$1" in
        http://127.0.0.1:*|http://localhost:*|https://127.0.0.1:*|https://localhost:*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

url_port() {
    local url="$1"
    local without_scheme="${url#*://}"
    local host_port="${without_scheme%%/*}"
    local port="${host_port##*:}"

    if [[ "$port" =~ ^[0-9]+$ ]]; then
        printf '%s\n' "$port"
        return
    fi

    return 1
}

port_is_open() {
    local port="$1"
    timeout 1 bash -c ":</dev/tcp/127.0.0.1/$port" >/dev/null 2>&1
}

require_local_port_available() {
    local url="$1"
    local name="$2"

    is_local_url "$url" || return

    local port
    port="$(url_port "$url")" || return

    if port_is_open "$port"; then
        log "$name port is already in use: $url"
        log "stop the existing process or set a different ${name^^} URL before running dev.sh"
        exit 1
    fi
}

runtime_bind_addr() {
    if [[ -n "${CAPSULE_BIND:-}" ]]; then
        printf '%s\n' "$CAPSULE_BIND"
        return
    fi

    local port
    port="$(url_port "$RUNTIME_URL")" || {
        printf '%s\n' "0.0.0.0:3001"
        return
    }
    printf '0.0.0.0:%s\n' "$port"
}

observal_should_start() {
    case "${CAPSULE_OBSERVAL:-auto}" in
        0|false|False|FALSE|off|Off|OFF|no|No|NO)
            return 1
            ;;
        1|true|True|TRUE|on|On|ON|yes|Yes|YES)
            return 0
            ;;
    esac

    [[ -f "$ROOT_DIR/$OBSERVAL_COMPOSE_FILE" ]] || return 1

    if [[ -n "${OBSERVAL_WEB_URL:-}" ]] && is_local_url "$OBSERVAL_WEB_URL"; then
        return 0
    fi

    if [[ -n "${OBSERVAL_API_URL:-}" ]] && is_local_url "$OBSERVAL_API_URL"; then
        return 0
    fi

    return 1
}

start_observal() {
    if ! observal_should_start; then
        return
    fi

    log "starting Observal stack"
    docker compose -f "$OBSERVAL_COMPOSE_FILE" up -d
    started_observal=1

    export OBSERVAL_CONTAINER_URL="${OBSERVAL_CONTAINER_URL:-http://observal-api:8000}"
    export CAPSULE_CONTAINER_NETWORK="${CAPSULE_CONTAINER_NETWORK:-capsule_observal-net}"

    wait_for_http "${OBSERVAL_API_URL:-http://127.0.0.1:8080}/readyz" "Observal API" 240
    wait_for_http "${OBSERVAL_WEB_URL:-http://127.0.0.1:3000}/" "Observal web" 240
}

cleanup() {
    local status=$?

    trap - EXIT INT TERM
    log "powering down"

    if [[ -n "$frontend_pid" ]] && kill -0 "$frontend_pid" >/dev/null 2>&1; then
        kill "$frontend_pid" >/dev/null 2>&1 || true
        wait "$frontend_pid" >/dev/null 2>&1 || true
    fi

    if [[ -n "$backend_pid" ]] && kill -0 "$backend_pid" >/dev/null 2>&1; then
        kill "$backend_pid" >/dev/null 2>&1 || true
        wait "$backend_pid" >/dev/null 2>&1 || true
    fi

    local current_containers
    current_containers="$(snapshot_containers)"

    local containers_to_remove
    containers_to_remove="$(
        comm -13 \
            <(printf '%s\n' "$initial_containers" | sed '/^$/d') \
            <(printf '%s\n' "$current_containers" | sed '/^$/d') || true
    )"

    if [[ -n "$containers_to_remove" ]]; then
        log "removing session containers"
        docker rm -f $containers_to_remove >/dev/null 2>&1 || true
    fi

    if [[ "$started_observal" == "1" && "${CAPSULE_OBSERVAL_STOP:-0}" == "1" ]]; then
        log "stopping Observal stack"
        docker compose -f "$OBSERVAL_COMPOSE_FILE" down >/dev/null 2>&1 || true
    fi

    log "stopped"
    exit "$status"
}

wait_for_http() {
    local url="$1"
    local name="$2"
    local attempts="${3:-60}"

    for ((i = 1; i <= attempts; i++)); do
        if curl -fsS "$url" >/dev/null 2>&1; then
            log "$name ready"
            return
        fi

        sleep 0.5
    done

    log "$name did not become ready: $url"
    exit 1
}

install_frontend_deps() {
    if [[ -d "$ROOT_DIR/frontend/node_modules" ]]; then
        return
    fi

    log "installing frontend dependencies"
    (
        cd "$ROOT_DIR/frontend"
        npm ci
    )
}

image_has_coding_harness() {
    docker run --rm "$IMAGE_NAME" bash -lc 'command -v node >/dev/null && command -v npm >/dev/null && command -v claude >/dev/null && command -v python3 >/dev/null && command -v observal >/dev/null' >/dev/null 2>&1
}

ensure_runtime_image() {
    if [[ "${CAPSULE_REBUILD_IMAGE:-0}" == "1" ]]; then
        log "rebuilding Docker image: $IMAGE_NAME"
        docker build -t "$IMAGE_NAME" docker/
        return
    fi

    if ! docker image inspect "$IMAGE_NAME" >/dev/null 2>&1; then
        log "building Docker image: $IMAGE_NAME"
        docker build -t "$IMAGE_NAME" docker/
        return
    fi

    if image_has_coding_harness; then
        log "Docker image exists: $IMAGE_NAME"
        return
    fi

    log "Docker image exists but is missing Claude Code or Observal CLI; rebuilding: $IMAGE_NAME"
    docker build -t "$IMAGE_NAME" docker/
}

require_command cargo
require_command curl
require_command docker
require_command npm

trap cleanup EXIT INT TERM

cd "$ROOT_DIR"

load_env_file

require_local_port_available "$RUNTIME_URL" "runtime"
require_local_port_available "$FRONTEND_URL" "frontend"

initial_containers="$(snapshot_containers)"

start_observal

ensure_runtime_image

log "building runtime"
cargo build -p capsule-runtime --release

install_frontend_deps

log "starting runtime on $RUNTIME_URL"
CAPSULE_BIND="$(runtime_bind_addr)" "$ROOT_DIR/target/release/capsule-runtime" &
backend_pid=$!

wait_for_http "$RUNTIME_URL/health" "runtime"

log "starting frontend on $FRONTEND_URL"
(
    cd "$ROOT_DIR/frontend"
    frontend_port="$(url_port "$FRONTEND_URL" || printf '3002')"
    CAPSULE_RUNTIME_TARGET="$RUNTIME_URL" npm run dev -- --host 0.0.0.0 --port "$frontend_port" --strictPort
) &
frontend_pid=$!

wait_for_http "$FRONTEND_URL/" "frontend"

log "ready: $FRONTEND_URL"
log "press Ctrl-C to power down"

wait -n "$backend_pid" "$frontend_pid"
