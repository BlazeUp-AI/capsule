FROM debian:bookworm-slim

# ── System packages ─────────────────────────────────────────────────────────────

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    curl \
    git \
    git-lfs \
    ca-certificates \
    build-essential \
    tmux \
    zip \
    unzip \
    tar \
    sudo \
    openssh-client \
    procps \
    less \
    locales \
    vim \
    python3 \
    python3-pip \
    python3-venv \
    inotify-tools \
    && rm -rf /var/lib/apt/lists/*

# ── Node.js 22 (required for Claude Code) ──────────────────────────────────────

RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# ── Claude Code ────────────────────────────────────────────────────────────────

RUN npm install -g @anthropic-ai/claude-code

# ── Observal CLI ────────────────────────────────────────────────────────────────

RUN pip3 install --break-system-packages observal-cli

# ── Locale configuration ────────────────────────────────────────────────────────

RUN sed -i '/en_US.UTF-8/s/^# //g' /etc/locale.gen && locale-gen

# ── Environment ─────────────────────────────────────────────────────────────────

ENV TERM=xterm-256color
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8
ENV CAPSULE_AGENT=claude

# ── User setup ──────────────────────────────────────────────────────────────────

RUN useradd -ms /bin/bash developer \
    && echo "developer ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/developer \
    && chmod 0440 /etc/sudoers.d/developer

# ── Workspace ───────────────────────────────────────────────────────────────────

RUN mkdir -p /workspace && chown developer:developer /workspace

USER developer
WORKDIR /workspace

# ── Shell configuration ─────────────────────────────────────────────────────────

USER root

RUN printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set +e' \
    'if [ -z "${OBSERVAL_TOKEN:-}" ] || [ -z "${OBSERVAL_SERVER_URL:-}" ]; then' \
    '    return 0 2>/dev/null || exit 0' \
    'fi' \
    'mkdir -p "$HOME/.observal"' \
    'printf "{\"server_url\":\"%s\",\"access_token\":\"%s\"}" "$OBSERVAL_SERVER_URL" "$OBSERVAL_TOKEN" > "$HOME/.observal/config.json"' \
    'observal doctor --yes patch --all --harness claude-code >/dev/null 2>&1 || true' \
    > /usr/local/bin/capsule-observal-setup \
    && chmod 0755 /usr/local/bin/capsule-observal-setup \
    && echo 'capsule-observal-setup >/dev/null 2>&1 || true' >> /home/developer/.bashrc \
    && chown developer:developer /home/developer/.bashrc

USER developer
CMD ["/bin/bash"]
