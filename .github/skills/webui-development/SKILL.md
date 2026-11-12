---
name: webui-development
description: Guidance for adding WebUI features across backend API, frontend pages, routing, state management, and navigation.
---

# WebUI Development

## When to Use

Use this skill for feature work that touches the WebUI backend, React frontend, or both.

## Add a WebUI Feature

### Backend API steps
1. Add a route module or extend an existing route under `crates/ouro_webui/src/api/`.
2. Use axum extractors (`State`, `Path`, `Query`, `Json`) as needed.
3. Register routes in the API router.
4. Use shared application state consistently.

### Frontend steps
1. Create a feature page under `webui/src/pages/<feature>/`.
2. Add API types in `webui/src/api/types.ts`.
3. Add API calls in `webui/src/api/client.ts`.
4. Register route in `webui/src/router.ts`.
5. Add sidebar/navigation entry if needed.

### State management rules
- Server data: TanStack Query.
- Client-only UI state: zustand.
- Realtime event stream: `useWebSocket` hook.

### Framework conventions
- React 19 + TypeScript 5.9 + Tailwind CSS 4.
- TanStack Router (code-based routing).
- Form validation with react-hook-form + zod.
- Keep comments and documentation in English.

### References
- `crates/ouro_webui/src/api/`
- `crates/ouro_webui/src/event/`
- `webui/src/router.ts`
- `webui/src/api/client.ts`
- `webui/src/api/types.ts`
- `webui/src/hooks/useWebSocket.ts`
- `webui/src/stores/`
- `webui/src/pages/dashboard/`
- `webui/src/components/layout/`
- `webui/src/components/ui/`

## Validation Checklist
- API route and handler added if backend change required.
- Page and route wiring completed.
- Query/store boundary is respected.
- Navigation updated.
- Feature renders and fetches expected data.

## Quick Checklist
- API, client, route, and page wiring are all aligned.
- Server state uses TanStack Query and UI-only state uses zustand.
- Navigation entry is present for user-facing features.
- Feature loads and displays expected data end-to-end.
