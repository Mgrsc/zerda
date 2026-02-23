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
RUN pacman -S --noconfirm --needed nodejs npm python python-pipx uv && \
    pacman -Scc --noconfirm

COPY --from=builder /build/target/release/zerda /usr/local/bin/zerda
COPY skills/ /usr/local/share/zerda/skills/
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

WORKDIR /root/.zerda
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["serve"]
