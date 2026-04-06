# Getting Started

## Build

```bash
cargo build --release
```

Binary output:

```text
target/release/zerda
```

## Quick Start

1. Copy `.env.example` to `.env` and fill in API keys.
2. For the bundled Compose stack, use the repository `zerda.toml`.
3. For bare-metal runs, copy `zerda.toml.full` to `~/.zerda/zerda.toml`.
4. Copy `identity.md` to `~/.zerda/identity.md`.

Minimal config:

```toml
[providers.openai]
type = "openai_chat"
api_key = "${OPENAI_API_KEY}"

[agent.primary_model]
model = "openai@gpt-4o"
vision = true
```

Run:

```bash
zerda
zerda run -m "Hello"
zerda run --resume
zerda serve
```

Compose quick start:

```bash
cp .env.example .env
docker compose up -d --build
```

## Config Resolution

1. `--config`
2. `ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

## Runtime Home Layout

```text
~/.zerda/
├── zerda.toml
├── identity.md
├── sessions/
└── ptc_jobs/
```

## Full Template

```bash
zerda config generate
```

Repository config note:

- `zerda.toml`: minimal Compose-ready config with memory enabled and Chroma at `http://chroma:8000`
- `zerda.toml.full`: full template for bare-metal or custom deployments

## Validate Config

```bash
zerda config validate
```
