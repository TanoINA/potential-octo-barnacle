---
name: react-best-practices
description: Implement or review React components for correct state, effects, rendering, data fetching, and measured performance. Use only for React projects; apply Next.js guidance only when that framework is present.
---
# React Best Practices

Inspect React version, framework, component boundaries, state ownership, and tests. Follow project conventions. Do not introduce Next.js assumptions into Vite or desktop apps.

## State and effects
- Keep state near its owner. Derive values during rendering instead of synchronizing duplicate state through effects.
- Use effects for external synchronization, with complete dependencies and cleanup. Prefer event handlers for direct user actions.
- Use functional updates when next state depends on previous state.
- Guard stale async responses when route, identity, or session changes. Cancel work when supported and verify ownership before applying results.
- Use stable data identities as list keys, not positions for reorderable or stateful rows.
- Keep subscriptions and external-store snapshots consistent; clean up listeners and timers.

## Data and performance
- Start independent requests together when safe, preserving error handling.
- Reuse the project's data/cache layer. Preserve authorization, invalidation, and per-user isolation.
- Measure slow paths before adding memoization. React.memo, useMemo, and useCallback are tools for demonstrated work or identity requirements, not blanket rules.
- For memoized children, check that props actually stay stable. Avoid reparsing static rich content on unrelated composer or status updates.
- Consider lazy loading for large optional routes. Virtualize genuinely large lists while preserving focus, accessibility, and scroll anchors.
- Avoid premature caches and broad state-management migrations.

## Framework-specific behavior
If Next.js is present, inspect version and router before using server components, caching, or server actions. Keep secrets server-side and validate authorization inside mutations. These rules do not automatically apply to other React runtimes.

Test changed interactions and relevant async/lifecycle cases. Use component tests first and browser tests when warranted. Do not claim a performance improvement without measurement.
