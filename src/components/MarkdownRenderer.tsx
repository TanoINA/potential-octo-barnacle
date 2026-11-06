import React, { useEffect, useMemo, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Copy, Check } from 'lucide-react';

interface MarkdownRendererProps {
  content: string;
}

const stripProseEmojis = (text: string) => {
  if (!text) return '';
  return text
    .replace(/(```[\s\S]*?```|`[^`]*`)|[\p{Extended_Pictographic}\uFE0F]/gu, (_, code) => code || '');
};

export const MarkdownRenderer: React.FC<MarkdownRendererProps> = ({ content }) => {
  const cleanedContent = useMemo(() => stripProseEmojis(content), [content]);

  return (
    <div className="markdown-content text-[13px] leading-6 select-text text-[#e6edf3]">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          p({ children }) {
            return <p className="mb-2.5 last:mb-0 leading-6">{children}</p>;
          },
          strong({ children }) {
            return <strong className="font-semibold text-white">{children}</strong>;
          },
          em({ children }) {
            return <em className="italic text-[#c9d1d9]">{children}</em>;
          },
          ul({ children }) {
            return <ul className="list-disc list-outside ml-5 mb-2.5 space-y-1 text-[#e6edf3]">{children}</ul>;
          },
          ol({ children }) {
            return <ol className="list-decimal list-outside ml-5 mb-2.5 space-y-1 text-[#e6edf3]">{children}</ol>;
          },
          li({ children }) {
            return <li className="leading-6 pl-1">{children}</li>;
          },
          blockquote({ children }) {
            return (
              <blockquote className="border-l-2 border-[#58a6ff] pl-3 py-1 italic text-[#8b949e] my-2.5 bg-[#0d1117]/50 rounded-r">
                {children}
              </blockquote>
            );
          },
          a({ href, children }) {
            return (
              <a
                href={href}
                target="_blank"
                rel="noopener noreferrer"
                className="text-[#58a6ff] hover:underline underline-offset-2 break-all"
              >
                {children}
              </a>
            );
          },
          h1({ children }) {
            return <h1 className="text-base font-bold text-white mt-4 mb-2 pb-1 border-b border-[#30363d]">{children}</h1>;
          },
          h2({ children }) {
            return <h2 className="text-sm font-semibold text-white mt-3.5 mb-1.5">{children}</h2>;
          },
          h3({ children }) {
            return <h3 className="text-xs font-semibold text-[#58a6ff] mt-3 mb-1">{children}</h3>;
          },
          hr() {
            return <hr className="border-t border-[#30363d] my-3" />;
          },
          table({ children }) {
            return (
              <div className="overflow-x-auto my-3 rounded-lg border border-[#30363d] bg-[#0d1117] shadow-sm">
                <table className="w-full text-left border-collapse text-xs select-text">{children}</table>
              </div>
            );
          },
          thead({ children }) {
            return <thead className="bg-[#161b22] border-b border-[#30363d] text-[#8b949e] font-semibold tracking-wider text-[11px] uppercase">{children}</thead>;
          },
          tbody({ children }) {
            return <tbody className="divide-y divide-[#30363d]/50 bg-[#0d1117]">{children}</tbody>;
          },
          tr({ children }) {
            return <tr className="even:bg-[#161b22]/30 hover:bg-[#1f242c]/50 transition-colors">{children}</tr>;
          },
          th({ children, style }) {
            return <th style={style} className="px-3.5 py-2.5 text-xs font-semibold text-[#c9d1d9] border-b border-[#30363d]">{children}</th>;
          },
          td({ children, style }) {
            return <td style={style} className="px-3.5 py-2 text-xs text-[#e6edf3] leading-5">{children}</td>;
          },
          code({ className, children, ...props }) {
            const isInline = !className && !String(children).includes('\n');
            if (isInline) {
              return (
                <code
                  className="bg-[#0d1117] text-[#79c0ff] px-1.5 py-0.5 rounded font-mono text-[12px] border border-[#30363d]/70 font-normal"
                  {...props}
                >
                  {children}
                </code>
              );
            }

            const codeText = String(children).replace(/\n$/, '');
            return <CodeBlock codeText={codeText} className={className} />;
          },
        }}
      >
        {cleanedContent}
      </ReactMarkdown>
    </div>
  );
};

function CodeBlock({ codeText, className }: { codeText: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | null>(null);
  const lang = className ? className.replace('language-', '') : '';

  useEffect(() => () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  }, []);

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
    <div className="relative my-3 rounded-xl bg-[#0d1117] border border-[#30363d] overflow-hidden group shadow-sm">
      <div className="flex items-center justify-between px-3.5 py-1.5 bg-[#161b22] border-b border-[#30363d] text-[11px] text-[#8b949e] font-mono">
        <span>{lang || 'code'}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 text-[#8b949e] hover:text-[#e6edf3] transition-colors"
          title="Copy code"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-[#3fb950]" /> : <Copy className="w-3.5 h-3.5" />}
          <span>{copied ? 'Copied' : 'Copy'}</span>
        </button>
      </div>
      <pre className="p-3.5 overflow-x-auto text-xs font-mono leading-5 text-[#e6edf3]">
        <code>{codeText}</code>
      </pre>
    </div>
  );
}
