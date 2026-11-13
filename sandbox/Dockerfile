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

COPY bin/ /usr/local/bin/

# Agent CLIs must not update themselves or phone home from the sandbox.
ENV DISABLE_AUTOUPDATER=1 \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1

ENTRYPOINT ["/usr/bin/tini", "--"]
