FROM rust:slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY zerda.toml.full ./zerda.toml.full
COPY src ./src
RUN find src -name '*.rs' -exec touch {} + && cargo build --release && strip target/release/zerda

FROM archlinux:latest

RUN echo -e "\n[archlinuxcn]\nSigLevel = Optional TrustAll\nServer = https://repo.archlinuxcn.org/\$arch" >> /etc/pacman.conf && \
    pacman -Syu --noconfirm && \
    pacman -S --noconfirm --needed \
    base-devel git curl wget openssh \
    paru \
    ripgrep fd jq yq tree less which file \
    zip unzip tar gzip bzip2 xz zstd p7zip \
    neovim ffmpeg \
    htop \
    && pacman -Scc --noconfirm

RUN useradd -m builder && \
    echo 'builder ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers

USER root
RUN pacman -S --noconfirm --needed \
    nodejs npm uv \
    alsa-lib nss nspr gtk3 pango cairo at-spi2-core \
    libxcomposite libxdamage libxext libxfixes libxrandr \
    libxkbcommon libxkbcommon-x11 libxi libxtst libxcursor \
    libxshmfence libdrm mesa dbus cups \
    && pacman -Scc --noconfirm

ENV VIRTUAL_ENV=/opt/zerda-python
ENV PATH=/opt/zerda-python/bin:$PATH
ENV ZERDA_PTC_PYTHON=/opt/zerda-python/bin/python
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
ENV UV_MANAGED_PYTHON=1

RUN uv venv --python 3.13 "$VIRTUAL_ENV" && \
    uv pip install --python "$ZERDA_PTC_PYTHON" 'scrapling[fetchers]==0.4.3' playwright && \
    "$ZERDA_PTC_PYTHON" -m playwright install chromium && \
    pacman -Scc --noconfirm

COPY --from=builder /build/target/release/zerda /usr/local/bin/zerda
COPY docs/zerda/ /usr/local/share/zerda/docs/zerda/
COPY code_primitives/ /usr/local/share/zerda/code_primitives/
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

WORKDIR /root/.zerda
ENV ZERDA_PRIMITIVES_ROOT=/usr/local/share/zerda/code_primitives/python
ENV PYTHONPATH=/usr/local/share/zerda/code_primitives/python:/root/.zerda
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["serve"]
