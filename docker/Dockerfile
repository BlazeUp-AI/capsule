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
    && rm -rf /var/lib/apt/lists/*

# ── Install fastfetch ───────────────────────────────────────────────────────────

RUN curl -sL https://github.com/fastfetch-cli/fastfetch/releases/latest/download/fastfetch-linux-amd64.deb -o /tmp/fastfetch.deb \
    && dpkg -i /tmp/fastfetch.deb \
    && rm /tmp/fastfetch.deb

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

# Run fastfetch on login
RUN echo 'fastfetch' >> ~/.bashrc

CMD ["/bin/bash"]
