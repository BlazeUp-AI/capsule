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

# ── AI Coding CLIs ─────────────────────────────────────────────────────────────

RUN npm install -g @anthropic-ai/claude-code opencode-ai

RUN curl -fsSL https://cli.kiro.dev/install | bash \
    && cp -f /root/.local/bin/kiro* /usr/local/bin/ 2>/dev/null || true \
    && cp -f /root/.local/bin/q /usr/local/bin/ 2>/dev/null || true \
    && ln -sf /usr/local/bin/kiro-cli /usr/local/bin/kiro

RUN curl -fsSL https://pi.dev/install.sh | sh \
    && cp -f /root/.local/bin/pi /usr/local/bin/ 2>/dev/null || true

# ── Observal CLI ────────────────────────────────────────────────────────────────

RUN pip3 install --break-system-packages observal-cli

# ── Install fastfetch ───────────────────────────────────────────────────────────

RUN ARCH=$(uname -m) \
    && curl -sL "https://github.com/fastfetch-cli/fastfetch/releases/download/2.40.3/fastfetch-linux-${ARCH}.deb" -o /tmp/fastfetch.deb \
    && dpkg -i /tmp/fastfetch.deb || true \
    && rm -f /tmp/fastfetch.deb

# ── Locale configuration ────────────────────────────────────────────────────────

RUN sed -i '/en_US.UTF-8/s/^# //g' /etc/locale.gen && locale-gen

# ── Environment ─────────────────────────────────────────────────────────────────

ENV TERM=xterm-256color
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8

# ── User setup ──────────────────────────────────────────────────────────────────

RUN useradd -ms /bin/bash developer \
    && echo "developer ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/developer \
    && chmod 0440 /etc/sudoers.d/developer

# ── Workspace ───────────────────────────────────────────────────────────────────

RUN mkdir -p /workspace && chown developer:developer /workspace

USER developer
WORKDIR /workspace

# ── Shell configuration ─────────────────────────────────────────────────────────

RUN echo 'fastfetch' >> ~/.bashrc \
    && echo '[ -n "$OBSERVAL_TOKEN" ] && mkdir -p ~/.observal && printf "{\"server_url\":\"%s\",\"access_token\":\"%s\"}" "$OBSERVAL_SERVER_URL" "$OBSERVAL_TOKEN" > ~/.observal/config.json && observal doctor patch --all --all-ides >/dev/null 2>&1 || true' >> ~/.bashrc

CMD ["/bin/bash"]
