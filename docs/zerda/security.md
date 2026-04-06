# Security & Deployment

## Security Model

Zerda has **full system permissions**. The agent can:

- Execute arbitrary shell commands
- Read and write any file
- Install packages
- Make network requests

**Strong recommendation**: Run Zerda in a Docker container or VM for isolation.

## Secret Management

- API keys should be stored in environment variables, not directly in config files
- Use `${VAR}` syntax in config to reference environment variables
- Secrets are not logged in regular trace output

## Access Control

### Telegram

- `allowed_users = ["*"]`: Accept all users (not recommended for public bots)
- `allowed_users = ["123456"]`: Whitelist specific Telegram user IDs

## Docker Deployment

Zerda can be run in Docker with `zerda serve` for configured channels such as Telegram:

```dockerfile
FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/zerda /usr/local/bin/
CMD ["zerda", "serve"]
```

Use `env_file` or environment variables in `docker-compose.yml` for secrets. Zerda does not auto-load `.env` files; they must be sourced externally.

## Input Validation

- Code primitives perform deep input validation (types, ranges, whitelists)
- URL format checking (http/https only)
- Parameter range clamping (e.g., max_results)
- Type checking at system boundaries
