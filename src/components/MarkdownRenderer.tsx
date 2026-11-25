import React, { useEffect, useMemo, useRef, useState } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import rehypeKatex from 'rehype-katex';
import 'katex/dist/katex.min.css';
import { normalizeMathDelimiters, remarkPreserveMathCode } from './markdownMath';
import { Copy, Check, ExternalLink, FileText } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { InteractivePlanCard } from './InteractivePlanCard';
import { safeEncodeURIComponent } from '../utils/encoding';

interface MarkdownRendererProps {
  content: string;
}

const stripProseEmojis = (text: string) => {
  if (!text) return '';
  return text
    .replace(/(```[\s\S]*?```|`[^`]*`)|[\p{Extended_Pictographic}\uFE0F]/gu, (_, code) => code || '');
};

const safeUrlTransform = (url: string) => {
  let candidate = url.trim().replace(/[\u0000-\u001f\u007f\s]+/g, '');
  try {
    candidate = decodeURIComponent(candidate);
  } catch {
    // Keep collapsed candidate for scheme inspection.
  }
  candidate = candidate.replace(/[\u0000-\u001f\u007f\s]+/g, '');
  if (/^(javascript|vbscript|data):/i.test(candidate)) {
    return '';
  }
  return url;
};

const remarkPlugins = [remarkGfm, remarkMath, remarkPreserveMathCode];
const rehypePlugins: NonNullable<React.ComponentProps<typeof ReactMarkdown>['rehypePlugins']> = [
  [rehypeKatex, { trust: false, strict: 'ignore', maxExpand: 1000, maxSize: 20, errorColor: '#f85149' }],
];
const markdownComponents: Components = {
  p({ children }) {
    return <p className="mb-2.5 last:mb-0 leading-6">{children}</p>;
  },
  strong({ children }) {
    return <strong className="font-semibold text-[var(--ui-text-primary)]">{children}</strong>;
  },
  em({ children }) {
    return <em className="italic text-[var(--ui-text-secondary)]">{children}</em>;
  },
  ul({ children }) {
    return <ul className="mb-2.5 ml-5 list-disc list-outside space-y-1 text-[var(--ui-text-primary)]">{children}</ul>;
  },
  ol({ children }) {
    return <ol className="mb-2.5 ml-5 list-decimal list-outside space-y-1 text-[var(--ui-text-primary)]">{children}</ol>;
  },
  li({ children }) {
    return <li className="leading-6 pl-1">{children}</li>;
  },
  blockquote({ children }) {
    return (
      <blockquote className="my-2.5 rounded-r border-l-2 border-[var(--ui-accent)] bg-[var(--ui-widget-bg)] py-1.5 pl-3 pr-2 italic text-[var(--ui-text-secondary)]">
        {children}
      </blockquote>
    );
  },
  a: function MarkdownLink({ href, children }) {
    const [error, setError] = useState<string | null>(null);
    const isWeb = Boolean(href && /^(https?:\/\/|mailto:)/i.test(href));
    const handleClick = (e: React.MouseEvent) => {
      e.preventDefault();
      if (!href) return;
      setError(null);
      let target = href;
      try {
        target = decodeURIComponent(href);
      } catch {
        try {
          target = decodeURI(href);
        } catch {
          // keep original
        }
      }
      if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
        invoke('open_path_or_url', { pathOrUrl: target }).catch(async (err) => {
          if (String(err).includes('Opening executable, shortcut, or unknown file types is not allowed')) {
            await invoke('reveal_in_explorer', { path: target });
          } else {
            throw err;
          }
        }).catch((err) => {
          setError(String(err));
        });
      } else if (isWeb) {
        window.open(target, '_blank', 'noopener,noreferrer');
      }
    };

    return (
      <>
      <a
        href={href}
        onClick={handleClick}
        data-file-link="true"
        data-file-path={href}
        className="inline-flex items-center gap-1 font-medium text-[var(--ui-accent)] hover:underline underline-offset-2 break-all cursor-pointer transition-colors hover:text-[var(--ui-accent-hover)]"
        title={isWeb ? `Open web link: ${href ?? ''}` : `Open file: ${href ?? ''}`}
      >
        {children}
        {isWeb ? (
          <ExternalLink size={11} className="shrink-0 opacity-70" />
        ) : (
          <FileText size={11} className="shrink-0 opacity-70 text-[var(--ui-purple)]" />
        )}
      </a>
      {error && <span role="alert" className="block text-xs text-[var(--ui-red)]">Could not open link: {error}</span>}
      </>
    );
  },
  h1({ children }) {
    return <h1 className="mt-4 mb-2 border-b border-[var(--ui-stroke-tertiary)] pb-1 text-base font-bold text-[var(--ui-text-primary)]">{children}</h1>;
  },
  h2({ children }) {
    return <h2 className="mt-3.5 mb-1.5 text-sm font-semibold text-[var(--ui-text-primary)]">{children}</h2>;
  },
  h3({ children }) {
    return <h3 className="mt-3 mb-1 text-xs font-semibold text-[var(--ui-accent)]">{children}</h3>;
  },
  hr() {
    return <hr className="my-3 border-t border-[var(--ui-stroke-tertiary)]" />;
  },
  table({ children }) {
    return (
      <div className="my-3 overflow-x-auto rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-code-bg)]">
        <table className="w-full text-left border-collapse text-xs select-text">{children}</table>
      </div>
    );
  },
  thead({ children }) {
    return <thead className="border-b border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] text-[11px] font-semibold uppercase tracking-wider text-[var(--ui-text-tertiary)]">{children}</thead>;
  },
  tbody({ children }) {
    return <tbody className="divide-y divide-[var(--ui-stroke-quaternary)] bg-[var(--ui-code-bg)]">{children}</tbody>;
  },
  tr({ children }) {
    return <tr className="transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]">{children}</tr>;
  },
  th({ children, style }) {
    return <th style={style} className="border-b border-[var(--ui-stroke-tertiary)] px-3.5 py-2.5 text-xs font-semibold text-[var(--ui-text-secondary)]">{children}</th>;
  },
  td({ children, style }) {
    return <td style={style} className="px-3.5 py-2 text-xs leading-5 text-[var(--ui-text-primary)]">{children}</td>;
  },
  code({ className, children, ...props }) {
    const isInline = !className && !String(children).includes('\n');
    if (isInline) {
      return (
        <code
          className="rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-1.5 py-0.5 font-mono text-[12px] font-normal text-[var(--ui-accent)]"
          {...props}
        >
          {children}
        </code>
      );
    }

    const codeText = String(children).replace(/\n$/, '');
    return <CodeBlock codeText={codeText} className={className === 'language-literal-math' ? 'language-math' : className} />;
  },
};

export const MarkdownRenderer = React.memo(function MarkdownRenderer({ content }: MarkdownRendererProps) {
  const cleanedContent = useMemo(() => normalizeMathDelimiters(stripProseEmojis(content)), [content]);

  return (
    <div className="markdown-content text-[13.5px] leading-[1.65] select-text text-[var(--ui-text-primary)]">
      <ReactMarkdown
        remarkPlugins={remarkPlugins}
        rehypePlugins={rehypePlugins}
        urlTransform={safeUrlTransform}
        components={markdownComponents}
      >
        {cleanedContent}
      </ReactMarkdown>
    </div>
  );
});

function CodeBlock({ codeText, className }: { codeText: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | null>(null);
  const lang = className ? className.replace('language-', '') : '';

  useEffect(() => () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  }, []);

  if (lang === 'plan') {
    return <InteractivePlanCard rawPlan={codeText} />;
  }

  const handleCopy = async () => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(codeText);
      } else {
        const textarea = document.createElement('textarea');
        textarea.value = codeText;
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
      }
      setCopied(true);
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // fallback ignore
    }
  };

  return (
    <div
      data-code-block="true"
      data-code-lang={lang}
      data-code-content={safeEncodeURIComponent(codeText)}
      className="group relative my-3 overflow-hidden rounded-xl border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-code-bg)]"
    >
      <div className="flex items-center justify-between border-b border-[var(--ui-stroke-quaternary)] bg-[var(--ui-widget-bg)] px-3.5 py-1.5 font-mono text-[11px] text-[var(--ui-text-tertiary)]">
        <span>{lang || 'code'}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[var(--ui-text-tertiary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
          title="Copy code"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-[var(--ui-green)]" /> : <Copy className="w-3.5 h-3.5" />}
          <span>{copied ? 'Copied' : 'Copy'}</span>
        </button>
      </div>
      <pre className="p-3.5 overflow-x-auto text-xs font-mono leading-5 text-[var(--ui-text-primary)]">
        <code>{codeText}</code>
      </pre>
    </div>
  );
}
