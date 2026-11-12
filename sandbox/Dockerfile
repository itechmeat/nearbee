# The image Nearbee's sandboxed apb instance runs in (scripts/apb-instance.sh
# with NEARBEE_APB_SANDBOX=1). Build it with sandbox/build.sh, which copies
# the host's apb, nearbee and self-contained agent CLI binaries into bin/
# first. Ubuntu 26.04 is the host release, so the copied binaries find the
# same glibc. No OPENCODE_, proxy or NEARAI_CLOUD_URL variable is set here:
# the tier check treats any of them as a route change.
FROM ubuntu:26.04

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates git gh curl jq ripgrep python3 python3-venv tini \
      poppler-utils imagemagick util-linux nodejs npm \
 && rm -rf /var/lib/apt/lists/*

RUN npm install -g --no-fund --no-audit \
      @fission-ai/openspec@2.0.0 \
      @itechmeat/open-code-review@1.13.8-fork.1 \
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
