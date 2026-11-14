# The image Nearbee's sandboxed apb instance runs in (scripts/apb-instance.sh
# with NEARBEE_APB_SANDBOX=1). Build it with sandbox/build.sh, which copies
# the host's apb, nearbee and self-contained agent CLI binaries into bin/
# first. Ubuntu 26.04 is the host release, so the copied binaries find the
# same glibc. No OPENCODE_, proxy or NEARAI_CLOUD_URL variable is set here:
# the tier check treats any of them as a route change.
FROM ubuntu:26.04

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates git gh curl jq ripgrep python3 python3-venv python3-yaml tini \
      poppler-utils imagemagick util-linux xz-utils \
      build-essential bubblewrap pkg-config socat \
 && rm -rf /var/lib/apt/lists/*

# Node.js 24 from nodejs.org (the host's version, checksum pinned): DeepSeek
# Harness needs Node 24.9 or newer and Ubuntu ships 22. Its npm 11 skips
# package install scripts unless they are allowed, so the native modules of
# dsh are allowed by name below.
ARG NODE_VERSION=24.21.0
ARG NODE_SHA256=fd8e59d5a511510f6a298afb548f18c7d2b1be404d8b4a27d94fbe49f56cb2d6
RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" -o /tmp/node.tar.xz \
 && echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum -c - \
 && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 --exclude='node-v*/CHANGELOG.md' --exclude='node-v*/README.md' \
 && rm /tmp/node.tar.xz

# Claude Code and DeepSeek Harness are pinned here, not copied from the host
# (sandbox/build.sh leaves both out of its default CLI list), so a script
# step's `claude -p` or `dsh` runs a known version; their token or key comes
# in through NEARBEE_APB_ENV_VARS.
RUN npm install -g --no-fund --no-audit \
      --allow-scripts=@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs \
      @fission-ai/openspec@2.0.0 \
      @itechmeat/open-code-review@1.13.8-fork.1 \
      @anthropic-ai/claude-code@2.1.319 \
      @deepseek-ai/dsh@0.2.2-rc.3 \
 && npm cache clean --force \
 && rm -rf /root/.npm

RUN python3 -m venv /opt/code-ranker \
 && /opt/code-ranker/bin/pip install --no-cache-dir code-ranker==5.0.4 \
 && ln -s /opt/code-ranker/bin/code-ranker /usr/local/bin/code-ranker

# Build and audit toolchains for script steps that install, build, test and
# scan a buyer's repository. Each download is the official artifact at a
# pinned version, checked against its SHA-256 at build. The registries they
# reach at run time are in egress.allow; the build host fetches the
# toolchains themselves, the sandbox never does.
#
# Go from go.dev; GOTOOLCHAIN=local keeps a go.mod `toolchain` line from
# downloading another Go.
ARG GO_VERSION=1.27.4
ARG GO_SHA256=7226736c2339719cda28fbf3a54d71aa63bcefe376e98cef00034268dab24840
RUN curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz" -o /tmp/go.tar.gz \
 && echo "${GO_SHA256}  /tmp/go.tar.gz" | sha256sum -c - \
 && tar -xzf /tmp/go.tar.gz -C /usr/local \
 && rm /tmp/go.tar.gz

# Rust through rustup (minimal profile) with one pinned stable toolchain.
# CARGO_HOME and RUSTUP_HOME are part of the read-only image: a step that
# fetches crates sets CARGO_HOME to a writable directory of its own; the
# rustup proxies on PATH still find the toolchain through RUSTUP_HOME.
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
ARG RUST_VERSION=1.100.0
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo
RUN curl -fsSL "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" -o /tmp/rustup-init \
 && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" \
 && rm /tmp/rustup-init \
 && chmod -R a+rX,go-w "$RUSTUP_HOME" "$CARGO_HOME"

# uv, the static release binary.
ARG UV_VERSION=0.13.8
ARG UV_SHA256=61fb26dabc4f0cf1d050fc6c67d40e43e4fae2994af21ebc1e050b233bf2b5ed
RUN curl -fsSL "https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-x86_64-unknown-linux-musl.tar.gz" -o /tmp/uv.tar.gz \
 && echo "${UV_SHA256}  /tmp/uv.tar.gz" | sha256sum -c - \
 && tar -xzf /tmp/uv.tar.gz -C /usr/local/bin --strip-components=1 \
      uv-x86_64-unknown-linux-musl/uv uv-x86_64-unknown-linux-musl/uvx \
 && rm /tmp/uv.tar.gz

# osv-scanner, the release binary.
ARG OSV_SCANNER_VERSION=2.7.1
ARG OSV_SCANNER_SHA256=fb7dab19a8bd9bbe5c3189f09d4b7ee1820c9553a23bac202648a56efd2f6ee8
RUN curl -fsSL "https://github.com/google/osv-scanner/releases/download/v${OSV_SCANNER_VERSION}/osv-scanner_linux_amd64" -o /usr/local/bin/osv-scanner \
 && echo "${OSV_SCANNER_SHA256}  /usr/local/bin/osv-scanner" | sha256sum -c - \
 && chmod 0755 /usr/local/bin/osv-scanner

# pip-audit in its own venv, from a lock that pins it and every dependency
# with its SHA-256 (pip-audit.lock), so two builds install the same files.
COPY pip-audit.lock /tmp/pip-audit.lock
RUN python3 -m venv /opt/pip-audit \
 && /opt/pip-audit/bin/pip install --no-cache-dir --require-hashes --no-deps -r /tmp/pip-audit.lock \
 && rm /tmp/pip-audit.lock \
 && ln -s /opt/pip-audit/bin/pip-audit /usr/local/bin/pip-audit

# GOTOOLCHAIN=local and RUSTUP_TOOLCHAIN hold Go and Rust to the image's
# toolchains: a repository's go.mod `toolchain` line or its
# rust-toolchain(.toml) file would otherwise start a download the sandbox
# cannot make (no route, read-only RUSTUP_HOME). RUSTUP_TOOLCHAIN overrides
# the toolchain file, so cargo builds with the pinned toolchain.
ENV PATH=/usr/local/cargo/bin:/usr/local/go/bin:$PATH \
    GOTOOLCHAIN=local \
    RUSTUP_TOOLCHAIN=${RUST_VERSION}

# Headless Chromium for script steps that render and check web pages
# (screenshots, axe): Playwright's own build at one pinned revision, shared
# read-only under PLAYWRIGHT_BROWSERS_PATH. An agent pins the same
# Playwright version in its own lock (astro-site: 1.64.1, Chromium 1248).
ARG PLAYWRIGHT_VERSION=1.64.1
ENV PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright
RUN npx -y "playwright@${PLAYWRIGHT_VERSION}" install --with-deps chromium chromium-headless-shell \
 && chmod -R a+rX /opt/ms-playwright \
 && rm -rf /root/.npm /var/lib/apt/lists/*

COPY bin/ /usr/local/bin/

# Agent CLIs must not update themselves or phone home from the sandbox.
ENV DISABLE_AUTOUPDATER=1 \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1

ENTRYPOINT ["/usr/bin/tini", "--"]
