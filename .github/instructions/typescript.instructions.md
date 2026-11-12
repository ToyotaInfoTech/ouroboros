---
applyTo: "webui/**/*.{ts,tsx}"
---

# TypeScript / WebUI Standards

## Framework
- React 19 + TypeScript 5.9 + Vite 8 + Tailwind CSS 4

## Routing
- TanStack Router (code-based, NOT file-based)
- Route definitions in `webui/src/router.ts`

## State Management
- Server state: TanStack Query (`useQuery`, `useMutation` for fetch, cache, invalidation)
- Client state: zustand (UI-local state only, stores in `webui/src/stores/`)

## Forms
- react-hook-form + zod for validation

## API Communication
- Client: `webui/src/api/client.ts` (fetch-based)
- Types: `webui/src/api/types.ts`
- WebSocket: `webui/src/hooks/useWebSocket.ts` for real-time events

## Project Structure
- Pages: `webui/src/pages/{dashboard,edges,metadata,tags}/`
- Reusable components: `webui/src/components/ui/`
- Layout components: `webui/src/components/layout/`

## Comments
- Write all comments and documentation in English.
