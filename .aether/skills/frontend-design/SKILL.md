---
name: frontend-design
description: Design and implement websites, landing pages, and application interfaces with intentional typography, responsive layout, and accessible interaction. Use for new UI or meaningful visual redesigns.
---
# Frontend Design

## Start with the project
- Inspect components, routes, design tokens, assets, dependencies, and build commands before editing.
- Preserve an established product's visual language. Do not replace its design system or add a framework for a small change.
- For new designs, choose a clear direction based on audience and content. Ask only about decisions that materially affect scope.

## Build a coherent interface
- Establish hierarchy with typography, spacing, alignment, and contrast before decoration.
- Use a small set of reusable color, spacing, radius, and type tokens. Prefer existing or system fonts; avoid unnecessary external requests.
- Make the primary action obvious. Provide realistic content and useful loading, empty, error, and success states.
- Reuse components that share behavior or styling, without abstractions that only rename a single element.
- Use semantic HTML, visible focus, labeled controls, meaningful links, and text alternatives for informative images.
- Make navigation, dialogs, menus, and forms usable with a keyboard. Respect reduced motion.

## Responsive and practical
- Check narrow phone and wide desktop layouts, including long text and wrapping controls.
- Avoid horizontal overflow and fixed heights that clip content. Give touch controls adequate size and spacing.
- Keep text legible, animation purposeful, and decoration restrained. Follow product requirements for icons.
- Do not add a dependency for an effect that existing CSS or components can handle.

Run focused project checks. When browser tooling is available, inspect the rendered layout and exercise controls; otherwise disclose missing visual verification. Deliver accessible, responsive behavior, not only an attractive screenshot.
