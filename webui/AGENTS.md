# WebUI — Agent Instructions

## Purpose

Real-time monitoring dashboard for Ouroboros. Displays edge device status, telemetry data, and system metrics.

## Tech Stack

- React 19 + TypeScript 5.9
- Vite 8 (build tool)
- Tailwind CSS 4 (styling)
- TanStack Router (code-based routing, NOT file-based)
- TanStack Query (server state management)
- zustand (client-only UI state)
- react-hook-form + zod (form validation)
- recharts (charts), leaflet + react-leaflet (maps), lucide-react (icons)

## Project Structure

```
webui/src/
├── api/
│   ├── client.ts       # fetch-based API client
│   └── types.ts        # API response types
├── components/
│   ├── layout/         # App layout, sidebar, header
│   └── ui/             # Reusable UI components
├── hooks/
│   └── useWebSocket.ts # WebSocket real-time events
├── pages/
│   ├── dashboard/      # Main dashboard
│   ├── edges/          # Edge device management
│   ├── metadata/       # Telemetry metadata viewer
│   └── tags/           # Tag management
├── stores/
│   └── metadataFilterStore.ts  # zustand store
├── router.ts           # TanStack Router definition
├── main.tsx            # App entry point
└── index.css           # Global styles (Tailwind)
```

## Routes

| Path | Page | Loading |
|---|---|---|
| `/` | DashboardPage | eager |
| `/tags` | TagsPage | eager |
| `/tags/$tagId` | TagDetailPage | eager |
| `/edges` | EdgesPage | eager |
| `/edges/$edgeId` | EdgeDetailPage | eager |
| `/metadata` | MetadataPage | lazy |
| `/metadata/$vin/$startTime` | MetadataDetailPage | lazy |

## Patterns

### API Data Fetching
```typescript
// Use TanStack Query for all API calls
const { data, isLoading } = useQuery({
  queryKey: ['edges'],
  queryFn: () => apiClient.getEdges(),
});
```

### Real-time Updates
```typescript
// Use the WebSocket hook for live data
const { lastEvent } = useWebSocket();
```

### Client State
```typescript
// Use zustand for UI-only state
const useFilterStore = create<FilterState>((set) => ({
  filter: '',
  setFilter: (f: string) => set({ filter: f }),
}));
```

## Build

```bash
npm install         # Install dependencies
npm run dev         # Dev server with HMR
npm run build       # Production build (tsc + vite)
npm run lint        # ESLint
```

## Backend Integration

- REST API served by `ouro_webui` (axum)
- WebSocket for `DashboardEvent` streaming
- Static assets embedded in Rust binary via `rust-embed`
- Build frontend BEFORE Rust build: `cargo make bweb`
