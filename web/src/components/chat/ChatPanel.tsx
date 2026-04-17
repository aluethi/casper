import { useState, useRef, useEffect } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { ChatPanelProps, ChatMessage, ToolCallBlock } from './types'

// ── Markdown renderer ───────────────────────────────────────────

function Markdown({ children }: { children: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        // Headings
        h1: ({ children }) => <h1 className="text-lg font-bold mt-3 mb-1.5 first:mt-0">{children}</h1>,
        h2: ({ children }) => <h2 className="text-base font-bold mt-3 mb-1 first:mt-0">{children}</h2>,
        h3: ({ children }) => <h3 className="text-sm font-bold mt-2 mb-1 first:mt-0">{children}</h3>,
        // Paragraphs
        p: ({ children }) => <p className="mb-2 last:mb-0 leading-relaxed">{children}</p>,
        // Lists
        ul: ({ children }) => <ul className="list-disc pl-5 mb-2 last:mb-0 space-y-0.5">{children}</ul>,
        ol: ({ children }) => <ol className="list-decimal pl-5 mb-2 last:mb-0 space-y-0.5">{children}</ol>,
        li: ({ children }) => <li className="leading-relaxed">{children}</li>,
        // Code
        code: ({ className, children, ...props }) => {
          const isBlock = className?.startsWith('language-')
          if (isBlock) {
            return (
              <code className={`block bg-slate-800 text-slate-100 rounded-lg px-3 py-2 text-xs font-mono overflow-x-auto my-2 ${className ?? ''}`} {...props}>
                {children}
              </code>
            )
          }
          return (
            <code className="bg-slate-200/70 text-slate-800 px-1 py-0.5 rounded text-[0.85em] font-mono" {...props}>
              {children}
            </code>
          )
        },
        pre: ({ children }) => <pre className="my-2">{children}</pre>,
        // Block quotes
        blockquote: ({ children }) => (
          <blockquote className="border-l-2 border-slate-300 pl-3 my-2 text-slate-600 italic">{children}</blockquote>
        ),
        // Tables
        table: ({ children }) => (
          <div className="overflow-x-auto my-2">
            <table className="min-w-full text-xs border-collapse">{children}</table>
          </div>
        ),
        thead: ({ children }) => <thead className="bg-slate-200/50">{children}</thead>,
        th: ({ children }) => <th className="px-2 py-1 text-left font-semibold border-b border-slate-300">{children}</th>,
        td: ({ children }) => <td className="px-2 py-1 border-b border-slate-200">{children}</td>,
        // Links
        a: ({ href, children }) => (
          <a href={href} target="_blank" rel="noopener noreferrer" className="text-blue-600 hover:text-blue-800 underline underline-offset-2">
            {children}
          </a>
        ),
        // Horizontal rules
        hr: () => <hr className="my-3 border-slate-200" />,
        // Strong / em
        strong: ({ children }) => <strong className="font-semibold">{children}</strong>,
      }}
    >
      {children}
    </ReactMarkdown>
  )
}

// ── Thinking block ──────────────────────────────────────────────

function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4)
}

function ThinkingBlock({ text }: { text: string }) {
  const [open, setOpen] = useState(false)
  const tokens = estimateTokens(text)

  return (
    <div className="mb-2">
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 text-[11px] font-medium text-violet-600 hover:text-violet-800 transition-colors"
      >
        <svg className={`w-3 h-3 transition-transform ${open ? 'rotate-90' : ''}`} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
        </svg>
        Thinking (~{tokens.toLocaleString()} tokens)
      </button>
      {open && (
        <div className="mt-1.5 rounded-lg bg-violet-50 ring-1 ring-violet-200/60 px-3 py-2 text-xs text-violet-900 leading-relaxed whitespace-pre-wrap max-h-64 overflow-y-auto font-mono">
          {text}
        </div>
      )}
    </div>
  )
}

// ── Tool call block ─────────────────────────────────────────────

function ToolCallItem({ tc }: { tc: ToolCallBlock }) {
  const [showResult, setShowResult] = useState(false)
  const displayName = tc.name.replace(/^mcp__/, '').replace(/__/g, ' / ')
  const hasInput = tc.input && Object.keys(tc.input).length > 0

  return (
    <div className="rounded-lg ring-1 ring-amber-200/80 bg-amber-50/50 overflow-hidden">
      <div className="px-3 py-1.5 flex items-center justify-between gap-2 bg-amber-100/40">
        <div className="flex items-center gap-1.5 min-w-0">
          <svg className="w-3.5 h-3.5 text-amber-600 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M11.42 15.17l-5.59-2.4a1 1 0 01-.57-1.14l1.44-6.5a1 1 0 01.9-.74h8.8a1 1 0 01.9.74l1.44 6.5a1 1 0 01-.57 1.14l-5.59 2.4a1 1 0 01-1.16 0z" />
          </svg>
          <span className="text-[11px] font-semibold text-amber-900 truncate">{displayName}</span>
        </div>
        {tc.result !== undefined && (
          <button
            onClick={() => setShowResult(!showResult)}
            className={`text-[10px] font-medium px-1.5 py-0.5 rounded transition-colors ${
              tc.is_error
                ? 'text-red-700 bg-red-100 hover:bg-red-200'
                : 'text-amber-700 bg-amber-100 hover:bg-amber-200'
            }`}
          >
            {showResult ? 'Hide' : tc.is_error ? 'Error' : 'Result'}
          </button>
        )}
      </div>
      {hasInput && (
        <pre className="px-3 py-1.5 text-[11px] text-amber-800 font-mono leading-relaxed overflow-x-auto border-t border-amber-200/50">
          {JSON.stringify(tc.input, null, 2)}
        </pre>
      )}
      {showResult && tc.result !== undefined && (
        <div className={`px-3 py-1.5 text-[11px] font-mono leading-relaxed max-h-48 overflow-y-auto border-t ${
          tc.is_error ? 'bg-red-50 text-red-800 border-red-200/50' : 'bg-white/50 text-slate-700 border-amber-200/50'
        }`}>
          <pre className="whitespace-pre-wrap">{formatResult(tc.result)}</pre>
        </div>
      )}
    </div>
  )
}

function formatResult(result: string): string {
  try {
    const parsed = JSON.parse(result)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return result
  }
}

// ── Message bubble ──────────────────────────────────────────────

function MessageBubble({ message }: { message: ChatMessage }) {
  if (message.role === 'user') {
    return (
      <div className="flex justify-end">
        <div className="max-w-[75%] rounded-2xl rounded-br-md bg-blue-600 text-white px-4 py-2.5 text-sm whitespace-pre-wrap shadow-sm">
          {message.content}
        </div>
      </div>
    )
  }

  // Assistant message
  return (
    <div className="flex justify-start">
      <div className="max-w-[85%] space-y-2">
        {/* Thinking */}
        {message.thinking && <ThinkingBlock text={message.thinking} />}

        {/* Tool calls */}
        {message.toolCalls && message.toolCalls.length > 0 && (
          <div className="space-y-1.5">
            {message.toolCalls.map((tc, i) => (
              <ToolCallItem key={i} tc={tc} />
            ))}
          </div>
        )}

        {/* Content (rendered as markdown) */}
        {message.content && (
          <div className="rounded-2xl rounded-bl-md bg-slate-100 text-slate-800 px-4 py-2.5 text-sm shadow-sm">
            <Markdown>{message.content}</Markdown>
          </div>
        )}
      </div>
    </div>
  )
}

// ── Loading indicator ───────────────────────────────────────────

function LoadingDots() {
  return (
    <div className="flex justify-start">
      <div className="rounded-2xl rounded-bl-md bg-slate-100 px-4 py-3 text-sm text-slate-400 shadow-sm">
        <span className="inline-flex gap-0.5">
          <span className="w-1.5 h-1.5 bg-slate-400 rounded-full animate-bounce" style={{ animationDelay: '0ms' }} />
          <span className="w-1.5 h-1.5 bg-slate-400 rounded-full animate-bounce" style={{ animationDelay: '150ms' }} />
          <span className="w-1.5 h-1.5 bg-slate-400 rounded-full animate-bounce" style={{ animationDelay: '300ms' }} />
        </span>
      </div>
    </div>
  )
}

// ── Main ChatPanel ──────────────────────────────────────────────

export default function ChatPanel({
  messages,
  loading,
  onSend,
  onStop,
  disabled,
  placeholder = 'Type a message... (Enter to send, Shift+Enter for newline)',
  emptyStateText = 'Send a message to start chatting',
  emptyStateSubtext,
}: ChatPanelProps) {
  const [input, setInput] = useState('')
  const chatEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, loading])

  useEffect(() => {
    if (!loading) {
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [loading])

  const send = () => {
    const text = input.trim()
    if (!text || loading || disabled) return
    setInput('')
    onSend(text)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Messages */}
      <div className="flex-1 overflow-y-auto bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4 space-y-4 mb-3">
        {messages.length === 0 && !loading && (
          <div className="flex items-center justify-center h-full">
            <div className="text-center">
              <p className="text-slate-400 text-sm">{emptyStateText}</p>
              {emptyStateSubtext && (
                <p className="text-slate-300 text-xs mt-1">{emptyStateSubtext}</p>
              )}
            </div>
          </div>
        )}

        {messages.map((msg, i) => (
          <MessageBubble key={i} message={msg} />
        ))}

        {loading && <LoadingDots />}

        <div ref={chatEndRef} />
      </div>

      {/* Input */}
      <div className="flex gap-3">
        <textarea
          ref={inputRef}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={disabled ? 'Chat disabled' : placeholder}
          disabled={disabled || loading}
          rows={2}
          className="flex-1 resize-none rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow disabled:bg-slate-50 disabled:text-slate-400"
        />
        {loading && onStop ? (
          <button
            onClick={onStop}
            className="self-end bg-red-600 text-white px-5 py-2 rounded-full text-sm font-semibold hover:bg-red-500 active:bg-red-800 transition-colors whitespace-nowrap"
          >
            Stop
          </button>
        ) : (
          <button
            onClick={send}
            disabled={!input.trim() || loading || disabled}
            className="self-end bg-blue-600 text-white px-5 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50 whitespace-nowrap"
          >
            Send
          </button>
        )}
      </div>
    </div>
  )
}
