import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import ts from 'typescript';

const dialogs = ['SettingsModal', 'ModelPicker', 'MemoryModal', 'SessionHistoryModal', 'ToolsInspectorModal', 'ArtifactsLibraryModal', 'SpotlightBar', 'SelfImprovementBadge', 'OptionalPanel'];

function surfaceClasses(name: string) {
  const source = readFileSync(`src/components/${name}.tsx`, 'utf8');
  const file = ts.createSourceFile(`${name}.tsx`, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const surfaces: { classes: string[]; dialog: boolean }[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const attributes = node.attributes.properties.filter(ts.isJsxAttribute);
      const value = (key: string) => {
        const initializer = attributes.find((attribute) => attribute.name.getText(file) === key)?.initializer;
        return initializer && ts.isStringLiteral(initializer) ? initializer.text : '';
      };
      if (value('className')) surfaces.push({ classes: value('className').split(/\s+/), dialog: value('role') === 'dialog' });
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return { source, surfaces };
}

describe('shared modal surface hooks', () => {
  it.each(dialogs)('%s separates opacity-only dimmer from entering card', (name) => {
    const { surfaces } = surfaceClasses(name);
    const dimmer = surfaces.find(({ classes }) => classes.includes('fixed') && classes.includes('inset-0'))!;
    const card = surfaces.find(({ dialog }) => dialog)!;
    expect(dimmer.classes).toContain('ui-overlay-enter');
    expect(dimmer.classes).not.toContain('ui-panel-enter');
    expect(card.classes).toContain('ui-panel-enter');
    expect(card.classes).toContain('shadow-lg');
    expect(card.classes.some((item) => ['rounded-lg', 'rounded-xl'].includes(item))).toBe(true);
    expect([...dimmer.classes, ...card.classes].some((item) => item.startsWith('backdrop-blur') || item === 'shadow-2xl')).toBe(false);
  });

  it('animates only the ArtifactPane drawer roots, not virtualized rows', () => {
    const { source } = surfaceClasses('ArtifactPane');
    expect(source).toMatch(/\? 'ui-drawer-enter fixed/);
    expect(source).toMatch(/: 'ui-drawer-enter relative/);
    expect(source).not.toContain('shadow-2xl');
    expect(source).toContain('style={{ transform: `translateY(${virtualLine.start}px)` }}');
    expect(source.match(/ui-drawer-enter/g)).toHaveLength(2);
    expect(source).not.toContain('ui-panel-enter');
  });
});
