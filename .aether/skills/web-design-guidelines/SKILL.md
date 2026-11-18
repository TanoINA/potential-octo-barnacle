---
name: web-design-guidelines
description: Review website and application UI for accessibility, keyboard behavior, responsive layout, forms, and usability. Use for frontend audits or interface quality checks before shipping.
---
# Web Design Guidelines

This checklist works locally without downloading remote rules.

Inspect affected components and shared styles. Exercise actual behavior when browser tooling is available. Prioritize concrete barriers over cosmetic preferences. Do not present a source review as WCAG certification.

## Semantics and keyboard
- Use links for navigation and buttons for actions. Name icon-only controls accessibly.
- Keep visible focus and logical tab order. Avoid positive tabindex and mouse-only interaction.
- Dialogs need names, focus handling, safe keyboard dismissal, and focus restoration.
- Hover content must also work through focus or activation. Do not convey state by color alone.

## Content, forms, and feedback
- Label inputs and associate validation messages with fields. Explain constrained input formats.
- Preserve user input on recoverable failures. Prevent duplicate destructive submissions without blocking ordinary editing.
- Announce important asynchronous feedback without repeatedly interrupting screen readers.
- Use meaningful headings, links, and image alternatives. Decorative images should not produce redundant announcements.
- Measure contrast when tooling is available and target WCAG AA. State when contrast has not been measured.

## Layout and resilience
- Check narrow widths, zoom, long labels, wrapping, and empty/error states. Avoid clipped content and accidental horizontal scrolling.
- Make touch targets large and spaced enough. Do not hide important actions behind subtle hover effects.
- Honor reduced motion and avoid disruptive autoplay or unnecessary animation.
- Reserve image space when practical and minimize layout shifts during loading.

Report severity, file location, user impact, and a specific correction. Separate confirmed behavior from source-based suspicions. Stay within the requested scope rather than turning a review into a redesign.
