import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "@/lib/utils";

/** AI 解释 / 建议等场景的 Markdown 只读预览（v2.1 规格：explain_logs 返回 markdown）。 */
export function MarkdownPreview({
  content,
  className,
  streaming = false,
}: {
  content: string;
  className?: string;
  /** 流式生成中：末尾显示输入光标并允许空内容占位。 */
  streaming?: boolean;
}) {
  if (!content && !streaming) return null;

  return (
    <div className={cn("text-[0.8rem] leading-relaxed text-[var(--t1,#222326)]", className)}>
      {content ? (
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={{
            h1: ({ children }) => (
              <h1 className="mb-3 mt-4 text-[1rem] font-semibold first:mt-0">{children}</h1>
            ),
            h2: ({ children }) => (
              <h2 className="mb-2 mt-3 text-[0.92rem] font-semibold first:mt-0">{children}</h2>
            ),
            h3: ({ children }) => (
              <h3 className="mb-2 mt-3 text-[0.85rem] font-semibold first:mt-0">{children}</h3>
            ),
            p: ({ children }) => <p className="mb-2 last:mb-0">{children}</p>,
            ul: ({ children }) => (
              <ul className="mb-2 flex list-disc flex-col gap-1 pl-5 last:mb-0">{children}</ul>
            ),
            ol: ({ children }) => (
              <ol className="mb-2 flex list-decimal flex-col gap-1 pl-5 last:mb-0">{children}</ol>
            ),
            li: ({ children }) => <li className="leading-relaxed">{children}</li>,
            blockquote: ({ children }) => (
              <blockquote className="mb-2 border-l-2 border-[var(--st-accent,#5e6ad2)] pl-3 text-[var(--t2,#62666d)] last:mb-0">
                {children}
              </blockquote>
            ),
            code: ({ className: codeClass, children, ...props }) => {
              const isBlock = codeClass?.includes("language-");
              if (isBlock) {
                return (
                  <code className={cn("font-mono text-[0.72rem]", codeClass)} {...props}>
                    {children}
                  </code>
                );
              }
              return (
                <code
                  className="rounded bg-[var(--surface-3,#efeff1)] px-1 py-0.5 font-mono text-[0.72rem]"
                  {...props}
                >
                  {children}
                </code>
              );
            },
            pre: ({ children }) => (
              <pre className="mb-2 overflow-x-auto rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2 font-mono text-[0.72rem] leading-relaxed last:mb-0">
                {children}
              </pre>
            ),
            a: ({ href, children }) => (
              <a
                href={href}
                className="text-[var(--st-accent,#5e6ad2)] underline hover:text-[var(--st-accent-hover,#4f5ac8)]"
                target="_blank"
                rel="noreferrer"
              >
                {children}
              </a>
            ),
            table: ({ children }) => (
              <div className="mb-2 overflow-x-auto last:mb-0">
                <table className="w-full border-collapse text-[0.75rem]">{children}</table>
              </div>
            ),
            th: ({ children }) => (
              <th className="border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1 text-left font-medium">
                {children}
              </th>
            ),
            td: ({ children }) => (
              <td className="border border-[var(--line,#e6e6e6)] px-2 py-1">{children}</td>
            ),
            hr: () => <hr className="my-3 border-[var(--line,#e6e6e6)]" />,
            strong: ({ children }) => <strong className="font-semibold">{children}</strong>,
          }}
        >
          {content}
        </ReactMarkdown>
      ) : null}
      {streaming ? (
        <span
          className="ml-0.5 inline-block h-[1em] w-0.5 animate-pulse bg-[var(--st-accent,#5e6ad2)] align-text-bottom"
          aria-hidden
        />
      ) : null}
    </div>
  );
}
