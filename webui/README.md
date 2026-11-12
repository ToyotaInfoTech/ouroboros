# Ouroboros WebUI

Real-time monitoring dashboard for Ouroboros, built with React, TypeScript, Vite and Tailwind CSS.
The production build is served by the daemon (`ourodaemon`).

For the architecture, REST API and WebSocket events, see [docs/WebUI.md](../docs/WebUI.md).
For contributor guidance, see [AGENTS.md](AGENTS.md).

## Commands

Run these from the `webui/` directory.

| Command | Description |
| --- | --- |
| `npm install` | Install dependencies |
| `npm run dev` | Start the Vite dev server (proxies `/api` to `http://localhost:3001`) |
| `npm run build` | Type-check and build into `dist/` |
| `npm run preview` | Preview the production build |
| `npm run lint` | Run ESLint |
| `npm test` | Run the unit tests once with Vitest |
| `npm run test:watch` | Run Vitest in watch mode |

`cargo make bweb` runs `npm install` and `npm run build` from the repository root.

## Map tiles

The Metadata pages show a plain map background unless a tile source is configured at build time
with `VITE_MAP_TILE_URL` (tile URL template with `{z}`, `{x}`, `{y}`) and `VITE_MAP_ATTRIBUTION`
(attribution text). See [Map background tiles](../docs/WebUI.md#map-background-tiles).
