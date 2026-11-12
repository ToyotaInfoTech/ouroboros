# Security Considerations

Ouroboros is a research prototype. Run it only on networks you trust, and read this section before exposing any port.

| Endpoint | Default | Protection |
| --- | --- | --- |
| WebUI (REST and WebSocket, port 3001) | `127.0.0.1` | Optional bearer token, same-origin check, no CORS |
| Metrics HTTP server (port 8080) | `127.0.0.1` | None |
| Zenoh router (`tcp/7447`, `tcp/7448`) | `0.0.0.0` | None by default; 7448 is unencrypted |

### Exposing the WebUI and metrics

Both servers listen on loopback only. Change the listen address with environment variables, for example in `.env` (see `examples/dot.env.example`):

```bash
OURO_WEBUI_BIND=0.0.0.0:3001
OURO_METRICS_BIND=0.0.0.0:8080
OURO_WEBUI_TOKEN=$(openssl rand -hex 32)   # use only A-Za-z0-9_- characters
```

- The `ray` configuration (`etc/daemon_config_ray.toml`) needs these variables on the cloud host when other hosts, Prometheus or an SSH tunnel must reach the WebUI or the metrics server. Make sure the variables reach the `ourodaemon` process.
- Always set `OURO_WEBUI_TOKEN` when the WebUI is not loopback-only. Without a token, anyone who can reach the port can read data and send commands to edges. The daemon logs a warning when authentication is disabled.
- The browser asks for the token once per session through a plain `window.prompt` dialog. A dedicated login page is not implemented.
- The token travels in clear text over HTTP. Use a TLS-terminating reverse proxy or an SSH tunnel across untrusted networks.
- The same-origin check does not stop DNS rebinding. Use the token whenever the browser can reach untrusted sites.
- The metrics server has no authentication. Restrict it with a firewall or keep it on loopback.

### Database and Grafana credentials

No default passwords ship with the repository. Copy `examples/dot.env.example` to `.env` (git-ignored) and replace every `CHANGE_ME` before running `cargo make cup` or `ourodaemon`:

```bash
cp examples/dot.env.example .env
openssl rand -hex 24   # generate a value for each CHANGE_ME
```

- `DATABASE_URL` is required by the daemon configuration; the daemon refuses to start with an unexpanded placeholder. Its password must match `POSTGRES_PASSWORD`.
- `docker compose` fails fast when `POSTGRES_USER`, `POSTGRES_PASSWORD`, `GF_ADMIN_USER` or `GF_ADMIN_PASSWORD` is unset.
- `cargo make dshow`, `dclear` and `ddrop` require `PGPASSWORD`.
- PostgreSQL is published on `127.0.0.1` only. Set `POSTGRES_BIND` to expose it on a trusted network.
- Database passwords created by an earlier setup stay in `container/data/pg`; change them with `ALTER USER` or recreate the volume.

### Zenoh

The sample `etc/zenohd/zenohd.json5` listens on all interfaces. Port 7448 is unencrypted and unauthenticated and is meant for a private network such as a VPC; restrict it with a firewall or security group. For edges that connect over the Internet, enable TLS and access control in the Zenoh configuration before use.

See also [SECURITY.md](../SECURITY.md) and [docs/WebUI.md](WebUI.md#security).
