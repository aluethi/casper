import { useState, useEffect, useCallback } from 'react'
import api from '../lib/api'
import type { Deployment, AvailableModel } from '../types'
import { ChatPanel } from '../components/chat'
import type { ChatMessage, ToolCallBlock } from '../components/chat'

// ── Types ───────────────────────────────────────────────────────

interface CompletionUsage {
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
}

// ── Styles ──────────────────────────────────────────────────────

const inputCls = 'w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow'
const selectCls = inputCls + ' bg-white'
const labelCls = 'block text-xs font-medium text-slate-500 mb-1'

// ── Component ───────────────────────────────────────────────────

export default function PlaygroundPage() {
  const [deployments, setDeployments] = useState<Deployment[]>([])
  const [models, setModels] = useState<AvailableModel[]>([])
  const [selectedSlug, setSelectedSlug] = useState('')
  const [systemPrompt, setSystemPrompt] = useState('')
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [loading, setLoading] = useState(false)
  const [initialLoading, setInitialLoading] = useState(true)
  const [error, setError] = useState('')
  const [lastUsage, setLastUsage] = useState<CompletionUsage | null>(null)
  const [totalTokens, setTotalTokens] = useState(0)

  // Parameters
  const [temperature, setTemperature] = useState('')
  const [maxTokens, setMaxTokens] = useState('')

  useEffect(() => {
    Promise.all([
      api.get('/api/v1/deployments?per_page=100'),
      api.get('/api/v1/deployments/available-models'),
    ])
      .then(([depRes, modRes]) => {
        const deps: Deployment[] = depRes.data.data ?? depRes.data
        setDeployments(deps.filter((d) => d.is_active))
        setModels(modRes.data)
        if (deps.length > 0) setSelectedSlug(deps[0].slug)
      })
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setInitialLoading(false))
  }, [])

  const selectedDeployment = deployments.find((d) => d.slug === selectedSlug)
  const selectedModel = selectedDeployment
    ? models.find((m) => m.id === selectedDeployment.model_id)
    : null

  const send = useCallback(async (text: string) => {
    if (!selectedSlug || loading) return

    setError('')
    const userMsg: ChatMessage = { role: 'user', content: text }
    const newMessages = [...messages, userMsg]
    setMessages(newMessages)
    setLoading(true)

    try {
      const apiMessages = [
        ...(systemPrompt ? [{ role: 'system', content: systemPrompt }] : []),
        ...newMessages.map((m) => ({ role: m.role, content: m.content })),
      ]

      const body: Record<string, unknown> = {
        model: selectedSlug,
        messages: apiMessages,
        stream: false,
      }
      if (temperature !== '') body.temperature = parseFloat(temperature)
      if (maxTokens !== '') body.max_tokens = parseInt(maxTokens, 10)

      const res = await api.post('/v1/chat/completions', body)
      const data = res.data
      const choice = data.choices?.[0]
      const msg = choice?.message
      const usage: CompletionUsage | undefined = data.usage

      // Build assistant message with thinking and tool calls
      const assistantMsg: ChatMessage = {
        role: 'assistant',
        content: msg?.content ?? '',
        thinking: msg?.thinking ?? undefined,
        toolCalls: msg?.tool_calls?.map((tc: Record<string, unknown>): ToolCallBlock => {
          const func = tc.function as Record<string, unknown> | undefined
          let args: Record<string, unknown> = {}
          try { args = JSON.parse((func?.arguments as string) || '{}') } catch { /* noop */ }
          return {
            name: (func?.name as string) || 'unknown',
            input: args,
          }
        }),
      }

      setMessages([...newMessages, assistantMsg])
      if (usage) {
        setLastUsage(usage)
        setTotalTokens((t) => t + usage.total_tokens)
      }
    } catch (e: unknown) {
      const err = e as { response?: { data?: { message?: string; error?: string } }; message?: string }
      const msg = err.response?.data?.message ?? err.response?.data?.error ?? err.message ?? 'Unknown error'
      setError(msg)
    } finally {
      setLoading(false)
    }
  }, [selectedSlug, loading, messages, systemPrompt, temperature, maxTokens])

  const clearChat = () => {
    setMessages([])
    setLastUsage(null)
    setTotalTokens(0)
    setError('')
  }

  if (initialLoading) return <p className="text-slate-500">Loading...</p>

  return (
    <div className="flex gap-6 h-[calc(100vh-7.5rem)]">
      {/* Left: Settings panel */}
      <div className="w-72 flex-shrink-0 space-y-4 overflow-y-auto">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Playground</h1>

        <div>
          <label className={labelCls}>Deployment</label>
          <select value={selectedSlug} onChange={(e) => { setSelectedSlug(e.target.value); clearChat() }}
            className={selectCls}>
            {deployments.length === 0 && <option value="">No deployments</option>}
            {deployments.map((d) => (
              <option key={d.slug} value={d.slug}>
                {d.name} ({d.slug})
              </option>
            ))}
          </select>
          {selectedModel && (
            <div className="flex gap-1.5 mt-2 flex-wrap">
              <span className="text-xs bg-slate-100 text-slate-600 px-2 py-0.5 rounded-full">
                {selectedModel.display_name}
              </span>
              <span className="text-xs bg-slate-100 text-slate-500 px-2 py-0.5 rounded-full">
                {selectedModel.provider}
              </span>
            </div>
          )}
        </div>

        <div>
          <label className={labelCls}>System Prompt</label>
          <textarea value={systemPrompt} onChange={(e) => setSystemPrompt(e.target.value)}
            placeholder="You are a helpful assistant."
            className={inputCls} rows={4} />
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className={labelCls}>Temperature</label>
            <input type="number" step="0.1" min="0" max="2" value={temperature}
              onChange={(e) => setTemperature(e.target.value)}
              placeholder="Default" className={inputCls} />
          </div>
          <div>
            <label className={labelCls}>Max Tokens</label>
            <input type="number" step="1" min="1" value={maxTokens}
              onChange={(e) => setMaxTokens(e.target.value)}
              placeholder="Default" className={inputCls} />
          </div>
        </div>

        {/* Token counter */}
        <div className="bg-slate-50 rounded-xl p-3 ring-1 ring-slate-200 space-y-1">
          <p className="text-xs font-medium text-slate-500">Token Usage</p>
          {lastUsage ? (
            <>
              <div className="flex justify-between text-xs text-slate-600">
                <span>Last: prompt</span><span>{lastUsage.prompt_tokens.toLocaleString()}</span>
              </div>
              <div className="flex justify-between text-xs text-slate-600">
                <span>Last: completion</span><span>{lastUsage.completion_tokens.toLocaleString()}</span>
              </div>
              <div className="flex justify-between text-xs font-medium text-slate-700 border-t border-slate-200 pt-1 mt-1">
                <span>Session total</span><span>{totalTokens.toLocaleString()}</span>
              </div>
            </>
          ) : (
            <p className="text-xs text-slate-400">No requests yet</p>
          )}
        </div>

        <button onClick={clearChat}
          className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm text-slate-600 hover:bg-slate-50 transition-colors">
          Clear conversation
        </button>
      </div>

      {/* Right: Chat area */}
      <div className="flex-1 flex flex-col min-w-0">
        {error && (
          <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-3">
            {error}
            <button onClick={() => setError('')} className="ml-2 text-red-400 hover:text-red-600">&times;</button>
          </div>
        )}

        <ChatPanel
          messages={messages}
          loading={loading}
          onSend={send}
          disabled={!selectedSlug}
          placeholder={selectedSlug ? 'Type a message... (Enter to send, Shift+Enter for newline)' : 'Select a deployment first'}
          emptyStateText="Send a message to start testing"
          emptyStateSubtext={selectedSlug ? `Using deployment: ${selectedSlug}` : undefined}
        />
      </div>
    </div>
  )
}
