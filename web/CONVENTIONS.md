# Frontend Development Conventions

## Routing
All pages must be located in `web/src/app/` following the Next.js-style directory structure. Legacy `web/src/pages/` directory has been removed.

## Component Modularity
- Page files (`page.tsx`) should ideally not exceed 400 lines of code.
- Extract complex UI logic, tabs, and repeating elements into modular sub-components in `web/src/components/`.

## Data Fetching and Mutations
- **Direct API imports are prohibited** in UI components (e.g., from `@/lib/api/*`).
- All data fetching and side effects must use **TanStack Query hooks** located in `web/src/hooks/`.
- If a hook doesn't exist for your needs, create one in the appropriate hook file or create a new hook file.
- Components should only depend on hooks and types.

## Type Safety
- Avoid using `any`. Ensure all hooks and components have proper TypeScript interfaces.
- Prefer specific event types and data models from `@/lib/api/types.ts`.
