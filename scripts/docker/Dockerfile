ARG BASE_IMAGE=ubuntu:24.04
FROM node:22-bookworm-slim AS node
FROM ${BASE_IMAGE}
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/opt/cargo RUSTUP_HOME=/opt/rustup \
    PATH=/opt/cargo/bin:/opt/format/bin:/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
COPY scripts/install_chromium.sh /usr/local/bin/install-procinsh-chromium
RUN set -eu; ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && if command -v apt-get >/dev/null; then \
         apt-get update -o APT::Update::Error-Mode=any; apt-get install -y --no-install-recommends \
           ca-certificates curl git tar gzip build-essential clang llvm pkg-config \
           libelf-dev zlib1g-dev libcap2-bin python3 python3-venv graphviz; \
         if [ "$(. /etc/os-release; echo "$VERSION_ID")" != 24.04 ]; then \
           apt-get install -y --no-install-recommends bpftool; \
         else \
           apt-get install -y --no-install-recommends linux-tools-generic; \
           ln -sf /usr/lib/linux-tools/*/bpftool /usr/local/bin/bpftool; \
         fi; \
       else \
         dnf install -y ca-certificates curl git tar gzip gcc gcc-c++ make clang llvm \
           pkgconf-pkg-config elfutils-libelf-devel zlib-devel libcap python3 python3-pip bpftool graphviz; \
       fi \
    && bash /usr/local/bin/install-procinsh-chromium \
    && if command -v apt-get >/dev/null; then rm -rf /var/lib/apt/lists/*; else dnf clean all; fi
COPY rust-toolchain.toml /opt/toolchain/rust-toolchain.toml
COPY requirements-format.txt /opt/toolchain/requirements-format.txt
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh \
    && sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain none \
    && cd /opt/toolchain && rustup show active-toolchain \
    && python3 -m venv /opt/format \
    && /opt/format/bin/pip install --no-cache-dir -r requirements-format.txt \
    && rm /tmp/rustup-init.sh
COPY scripts/docker/entrypoint.sh /usr/local/bin/procinsh-test
WORKDIR /work
ENTRYPOINT ["bash", "/usr/local/bin/procinsh-test"]
