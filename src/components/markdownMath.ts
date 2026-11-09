import { fromMarkdown } from 'mdast-util-from-markdown';

interface SourceNode {
  type: string;
  position?: { start: { offset?: number }; end: { offset?: number } };
  lang?: string | null;
  data?: { hProperties?: Record<string, unknown> };
  children?: SourceNode[];
}

// Rehype-KaTeX juga mengenali language-math. Tandai fence asli supaya tetap
// menjadi kode; node rumus dari remark-math tidak memiliki type `code`.
export function remarkPreserveMathCode() {
  return (tree: SourceNode) => {
    const visit = (node: SourceNode) => {
      if (node.type === 'code' && node.lang === 'math') {
        node.data = { ...node.data, hProperties: { ...node.data?.hProperties, className: ['language-literal-math'] } };
      }
      node.children?.forEach(visit);
    };
    visit(tree);
  };
}

// CommonMark menganggap \( dan \[ sebagai escape tanda baca. Normalisasi
// sebelum parsing, tetapi lindungi rentang kode/HTML/link dari perubahan isi.
export function normalizeMathDelimiters(source: string): string {
  if (!source.includes('\\(') && !source.includes('\\[')) return source;
  const protectedRanges: Array<[number, number]> = [];
  const collect = (node: SourceNode) => {
    if (['code', 'inlineCode', 'html', 'link', 'image', 'definition'].includes(node.type)) {
      const start = node.position?.start.offset;
      const end = node.position?.end.offset;
      if (start !== undefined && end !== undefined) protectedRanges.push([start, end]);
      return;
    }
    node.children?.forEach(collect);
  };
  collect(fromMarkdown(source));
  const normalize = (text: string) => text.replace(/\\\[([\s\S]*?)\\\]|\\\(([\s\S]*?)\\\)/g, (match, display: string | undefined, inline: string | undefined, offset: number) => {
    let slashes = 0;
    for (let index = offset - 1; index >= 0 && text[index] === '\\'; index--) slashes++;
    if (slashes % 2 !== 0) return match;
    return display !== undefined ? `\n\n$$\n${display.trim()}\n$$\n\n` : `$${inline}$`;
  });
  let result = '';
  let cursor = 0;
  for (const [start, end] of protectedRanges) {
    result += normalize(source.slice(cursor, start)) + source.slice(start, end);
    cursor = end;
  }
  return result + normalize(source.slice(cursor));
}
