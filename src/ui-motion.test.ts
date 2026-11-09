import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import postcss from 'postcss';
import { describe, expect, it } from 'vitest';

const stylesheet = () => postcss.parse(readFileSync(resolve(process.cwd(), 'src/index.css'), 'utf8'));

describe('shared interface motion', () => {
  it('defines short finite entry transitions using only opacity and transform', () => {
    const css = stylesheet();
    for (const name of ['ui-enter', 'ui-panel-enter', 'ui-menu', 'ui-overlay-enter', 'ui-drawer-enter', 'ui-sidebar-enter']) {
      const rules: postcss.Rule[] = [];
      css.walkRules((rule) => { if (rule.selector.split(',').map((part) => part.trim()).includes(`.${name}`)) rules.push(rule); });
      const declaration = rules.flatMap((rule) => rule.nodes).find((node) => node.type === 'decl' && node.prop === 'animation') as postcss.Declaration | undefined;
      expect(declaration, name).toBeTruthy();
      expect(declaration!.value).not.toMatch(/infinite|forwards/);
      const duration = declaration!.value.match(/\b(\d+)ms\b/);
      expect(Number(duration?.[1]), name).toBeGreaterThanOrEqual(100);
      expect(Number(duration?.[1]), name).toBeLessThanOrEqual(180);
      const keyframeName = declaration!.value.split(/\s+/)[0];
      let found = false;
      css.walkAtRules('keyframes', (frame) => {
        if (frame.params !== keyframeName) return;
        found = true;
        frame.walkDecls((property) => expect(['opacity', 'transform']).toContain(property.prop));
      });
      expect(found, keyframeName).toBe(true);
    }
  });

  it('fully disables animation and transitions for reduced motion without hidden content', () => {
    let reduced: postcss.AtRule | undefined;
    stylesheet().walkAtRules('media', (rule) => { if (rule.params.includes('prefers-reduced-motion: reduce')) reduced = rule; });
    expect(reduced).toBeTruthy();
    const declarations: Record<string, string> = {};
    reduced!.walkDecls((declaration) => { declarations[declaration.prop] = declaration.value; });
    expect(declarations.animation).toBe('none');
    expect(declarations.transition).toBe('none');
    expect(declarations['scroll-behavior']).toBe('auto');
    expect(declarations.opacity).toBeUndefined();
    expect(declarations.transform).toBeUndefined();
  });

  it('does not reserve permanent compositor layers or animate virtual row placement', () => {
    const source = readFileSync(resolve(process.cwd(), 'src/index.css'), 'utf8');
    expect(source).not.toContain('will-change:');
    expect(source).not.toMatch(/\[data-index\][^{]*\{[^}]*animation/s);
  });
});
