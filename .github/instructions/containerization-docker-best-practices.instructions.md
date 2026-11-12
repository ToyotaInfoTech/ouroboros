---
applyTo: "**/Dockerfile,**/Dockerfile.*,**/*.dockerfile,**/docker-compose*.yml,**/docker-compose*.yaml,**/compose*.yml,**/compose*.yaml"
description: "Guidelines for Dockerfiles and Compose files in Ouroboros (local TimescaleDB and Grafana containers)."
---

# Docker and Compose

The containers under `container/` run local development services (TimescaleDB and Grafana). Follow these rules when changing them or adding new ones.

## Images

- Prefer pinned image tags (for example a specific TimescaleDB release) over `latest`. `latest` is tolerable for disposable local services, but pin as soon as reproducibility matters.
- Use official or vendor-maintained images, and slim variants where they exist.

## Configuration and secrets

- Take credentials from environment variables such as `${POSTGRES_PASSWORD}`, supplied by an untracked `.env` file. Never write them into Compose files, images or committed config.
- Document variables only with placeholder values in `examples/dot.env.example`.
- Mount configuration read-only (`:ro`).
- Keep persistent data under `container/data/`, which is ignored by git.

## Runtime hardening

- Bind published ports to the loopback interface (`127.0.0.1:5432:5432`) unless remote access is required.
- Run as a non-root user where the image allows it, and drop capabilities that are not needed.
- Add a `healthcheck` to each service and use `depends_on` with `condition: service_healthy`.

## Dockerfiles

- Use multi-stage builds to keep runtime images small.
- Copy dependency manifests before sources so the dependency layer stays cached.
- Combine related `RUN` steps and remove temporary files in the same layer.
- Set a non-root `USER` and a `HEALTHCHECK`.
- Keep `.git`, `target/`, `node_modules/` and `.env` out of the build context with `.dockerignore`.
- Pass build-time secrets with BuildKit secrets, never with `ARG` or `COPY`.

## Checklist

- [ ] Image tags are pinned, or the use of `latest` is justified
- [ ] No credentials in the file; values come from the environment
- [ ] Ports are bound to loopback unless remote access is intended
- [ ] Configuration is mounted read-only
- [ ] Health checks and start-up ordering are defined
- [ ] Dockerfiles use a non-root user and a minimal build context
