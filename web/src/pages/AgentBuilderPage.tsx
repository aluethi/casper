import { useState, useEffect, useCallback, useRef } from 'react'
import { useParams } from 'react-router-dom'
import api from '../lib/api'
import { fetchSSE } from '../lib/sse'
import type { Agent, Deployment, AvailableModel } from '../types'
import PromptStackEditor, { type PromptBlock, type AvailableAgent } from './components/PromptStackEditor'
import ToolsEditor from './components/ToolsEditor'
import { ChatPanel } from '../components/chat'
import type { ChatMessage, ToolCallBlock } from '../components/chat'

// ── Ask-user popup ──────────────────────────────────────────────

function AskUserPopup({ question, options, onSubmit }: {
  question: string
  options: string[]
  onSubmit: (answer: string) => void
}) {
  const [freeText, setFreeText] = useState('')

  return (
    <div className="fixed inset-0 bg-black/40 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-white rounded-2xl shadow-2xl ring-1 ring-slate-900/10 max-w-lg w-full overflow-hidden">
        <div className="bg-blue-50 border-b border-blue-100 px-6 py-3 flex items-center gap-2">
          <svg className="w-5 h-5 text-blue-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <span className="text-sm font-semibold text-blue-900">Agent needs your input</span>
        </div>

        <div className="p-6">
          <p className="text-sm text-slate-800 leading-relaxed whitespace-pre-wrap mb-4">{question}</p>

          {options.length > 0 ? (
            <div className="space-y-2">
              {options.map((opt, i) => (
                <button key={i} onClick={() => onSubmit(opt)}
                  className="w-full text-left rounded-xl ring-1 ring-slate-200 hover:ring-blue-300 hover:bg-blue-50 px-4 py-2.5 text-sm font-medium text-slate-800 transition-all">
                  {opt}
                </button>
              ))}
              <div className="pt-2 border-t border-slate-100 mt-3">
                <p className="text-xs text-slate-400 mb-1.5">Or type a custom answer:</p>
                <div className="flex gap-2">
                  <input type="text" value={freeText} onChange={e => setFreeText(e.target.value)}
                    onKeyDown={e => { if (e.key === 'Enter' && freeText.trim()) onSubmit(freeText.trim()) }}
                    placeholder="Type your answer..."
                    className="flex-1 rounded-lg ring-1 ring-slate-300 px-3 py-1.5 text-sm focus:ring-2 focus:ring-blue-600 focus:outline-none"
                    autoFocus />
                  <button onClick={() => { if (freeText.trim()) onSubmit(freeText.trim()) }}
                    disabled={!freeText.trim()}
                    className="rounded-lg bg-blue-600 text-white px-4 py-1.5 text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                    Send
                  </button>
                </div>
              </div>
            </div>
          ) : (
            <div className="flex gap-2">
              <input type="text" value={freeText} onChange={e => setFreeText(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && freeText.trim()) onSubmit(freeText.trim()) }}
                placeholder="Type your answer..."
                className="flex-1 rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm focus:ring-2 focus:ring-blue-600 focus:outline-none"
                autoFocus />
              <button onClick={() => { if (freeText.trim()) onSubmit(freeText.trim()) }}
                disabled={!freeText.trim()}
                className="rounded-lg bg-blue-600 text-white px-5 py-2 text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                Send
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

// ── Connect-required popup ──────────────────────────────────────

function ConnectProviderPopup({ provider, displayName, onComplete }: {
  provider: string
  displayName: string
  onComplete: (result: string) => void
}) {
  const [status, setStatus] = useState<'idle' | 'connecting' | 'done'>('idle')

  const startConnect = async () => {
    setStatus('connecting')
    try {
      const res = await api.post(`/api/v1/connections/${provider}/start`)
      const authUrl = res.data.redirect_url

      // Open OAuth flow in a popup window
      const popup = window.open(authUrl, `connect_${provider}`, 'width=600,height=700,scrollbars=yes')

      // Poll until the popup closes (user completed or cancelled the flow)
      const poll = setInterval(() => {
        if (!popup || popup.closed) {
          clearInterval(poll)
          setStatus('done')
          onComplete('connected')
        }
      }, 500)
    } catch (e: any) {
      setStatus('idle')
      onComplete(`error: ${e.response?.data?.message ?? e.message}`)
    }
  }

  return (
    <div className="fixed inset-0 bg-black/40 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-white rounded-2xl shadow-2xl ring-1 ring-slate-900/10 max-w-md w-full overflow-hidden">
        <div className="bg-amber-50 border-b border-amber-100 px-6 py-3 flex items-center gap-2">
          <svg className="w-5 h-5 text-amber-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
          </svg>
          <span className="text-sm font-semibold text-amber-900">Connection required</span>
        </div>

        <div className="p-6 text-center">
          {status === 'idle' && (
            <>
              <p className="text-sm text-slate-700 mb-4">
                This action requires access to your <strong>{displayName}</strong> account.
                Click below to sign in and grant access.
              </p>
              <button onClick={startConnect}
                className="bg-blue-600 text-white px-6 py-2.5 rounded-full text-sm font-semibold hover:bg-blue-500 transition-colors">
                Connect {displayName}
              </button>
              <button onClick={() => onComplete('cancelled')}
                className="block mx-auto mt-3 text-xs text-slate-400 hover:text-slate-600 transition-colors">
                Skip
              </button>
            </>
          )}
          {status === 'connecting' && (
            <div className="py-4">
              <div className="animate-spin w-8 h-8 border-2 border-blue-600 border-t-transparent rounded-full mx-auto mb-3" />
              <p className="text-sm text-slate-500">Complete sign-in in the popup window...</p>
              <p className="text-xs text-slate-400 mt-1">This dialog will close automatically when done.</p>
            </div>
          )}
          {status === 'done' && (
            <p className="text-sm text-green-700 py-4">Connected. Resuming...</p>
          )}
        </div>
      </div>
    </div>
  )
}

// ── Main component ──────────────────────────────────────────────

interface TextBlock { type: 'text'; label: string; content: string }
interface KnowledgeBlock { type: 'knowledge'; label: string; budget_tokens: number }
interface DatasourceBlock { type: 'datasource'; label: string; source: Record<string, unknown>; budget_tokens: number; on_missing: string }

const tabs = ['Config', 'Chat', 'YAML'] as const
type Tab = (typeof tabs)[number]

// ── Main component ───────────────────────────────────────────────
export default function AgentBuilderPage() {
  const { name } = useParams<{ name: string }>()
  const [tab, setTab] = useState<Tab>('Config')
  const [agent, setAgent] = useState<Agent | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  // Config
  const [displayName, setDisplayName] = useState('')
  const [description, setDescription] = useState('')
  const [modelDeployment, setModelDeployment] = useState('')
  const [blocks, setBlocks] = useState<PromptBlock[]>([])
  const [builtinTools, setBuiltinTools] = useState<Record<string, Record<string, unknown>>>({})
  const [mcpConnectionNames, setMcpConnectionNames] = useState<string[]>([])
  const [saving, setSaving] = useState(false)

  // Available agents (for delegates block)
  const [availableAgents, setAvailableAgents] = useState<AvailableAgent[]>([])

  // Deployments (for model deployment dropdown)
  const [deployments, setDeployments] = useState<Deployment[]>([])
  const [models, setModels] = useState<AvailableModel[]>([])

  // Chat
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [sending, setSending] = useState(false)
  const [conversationId, setConversationId] = useState<string | null>(null)

  // Ask-user popup
  const [askPopup, setAskPopup] = useState<{ question: string; options: string[] } | null>(null)
  const askResolveRef = useRef<((answer: string) => void) | null>(null)

  // Connect-required popup
  const [connectPopup, setConnectPopup] = useState<{ provider: string; displayName: string } | null>(null)
  const connectResolveRef = useRef<((result: string) => void) | null>(null)

  // YAML
  const [yaml, setYaml] = useState('')

  // System prompt preview
  const [systemPrompt, setSystemPrompt] = useState('')
  const [showPrompt, setShowPrompt] = useState(false)
  const [promptLoading, setPromptLoading] = useState(false)

  // Token budget estimate
  const totalTokens = blocks.reduce((sum, b) => {
    if (b.type === 'text') return sum + Math.ceil((b.content?.length || 0) / 4)
    if (b.type === 'knowledge') return sum + b.budget_tokens
    if (b.type === 'datasource') return sum + b.budget_tokens
    return sum + 50 // rough estimate for other block types
  }, 0)

  useEffect(() => {
    api.get(`/api/v1/agents/${name}`)
      .then(r => {
        const a = r.data
        setAgent(a)
        setDisplayName(a.display_name || '')
        setDescription(a.description || '')
        setModelDeployment(a.model_deployment || '')
        setBlocks(a.prompt_stack || [])
        // Parse builtin tools
        const bt: Record<string, Record<string, unknown>> = {}
        const toolsData = a.tools?.builtin || []
        for (const t of toolsData) { bt[t.name] = { ...t }; delete bt[t.name].name }
        setBuiltinTools(bt)
        // Parse MCP servers
        const mcpRaw = a.tools?.mcp || []
        setMcpConnectionNames(mcpRaw.map((m: string | { name: string }) => typeof m === 'string' ? m : m.name))
      })
      .catch(e => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
    // Fetch all agents for delegate picker (exclude current agent)
    api.get('/api/v1/agents').then(r => {
      const list = (r.data.data || r.data || []) as AvailableAgent[]
      setAvailableAgents(list.filter(a => a.name !== name))
    }).catch(() => {})
    // Fetch deployments + models for dropdown
    Promise.all([
      api.get('/api/v1/deployments?per_page=100'),
      api.get('/api/v1/deployments/available-models'),
    ]).then(([depRes, modRes]) => {
      setDeployments((depRes.data.data ?? depRes.data).filter((d: Deployment) => d.is_active))
      setModels(modRes.data)
    }).catch(() => {})
  }, [name])

  useEffect(() => {
    if (tab === 'YAML') {
      api.get(`/api/v1/agents/${name}/export`).then(r => setYaml(typeof r.data === 'string' ? r.data : JSON.stringify(r.data, null, 2)))
        .catch(() => setYaml('# Export not available'))
    }
  }, [tab, name])

  const saveConfig = async () => {
    setSaving(true); setError('')
    try {
      const builtin = Object.entries(builtinTools).map(([name, config]) => ({ name, ...config }))
      await api.patch(`/api/v1/agents/${name}`, {
        display_name: displayName, description, model_deployment: modelDeployment,
        prompt_stack: blocks,
        tools: { builtin, mcp: mcpConnectionNames },
      })
      // Reload
      const r = await api.get(`/api/v1/agents/${name}`)
      setAgent(r.data)
    } catch (e: any) { setError(e.response?.data?.message ?? e.message) }
    finally { setSaving(false) }
  }

  const loadSystemPrompt = async () => {
    if (showPrompt) { setShowPrompt(false); return }
    setPromptLoading(true)
    try {
      const r = await api.get(`/api/v1/agents/${name}/prompt`)
      setSystemPrompt(r.data)
      setShowPrompt(true)
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setPromptLoading(false)
    }
  }

  // Ref to accumulate streaming assistant message state without causing re-render per token
  const streamRef = useRef({ thinking: '', content: '', toolCalls: [] as ToolCallBlock[] })
  const abortRef = useRef<AbortController | null>(null)

  const sendMessage = useCallback(async (text: string) => {
    if (sending) return
    setMessages(m => [...m, { role: 'user', content: text }])
    setSending(true)

    // Reset streaming accumulator and add placeholder assistant message
    streamRef.current = { thinking: '', content: '', toolCalls: [] }
    setMessages(m => [...m, { role: 'assistant', content: '' }])

    // Helper to update the last (assistant) message from the ref
    const flush = () => {
      const s = streamRef.current
      setMessages(m => {
        const updated = [...m]
        updated[updated.length - 1] = {
          role: 'assistant',
          content: s.content,
          thinking: s.thinking || undefined,
          toolCalls: s.toolCalls.length > 0 ? [...s.toolCalls] : undefined,
        }
        return updated
      })
    }

    // Batch UI updates — flush at most every 50ms during fast streaming
    let flushTimer: ReturnType<typeof setTimeout> | null = null
    const scheduleFlush = () => {
      if (!flushTimer) {
        flushTimer = setTimeout(() => { flushTimer = null; flush() }, 50)
      }
    }

    const controller = new AbortController()
    abortRef.current = controller

    try {
      const body: Record<string, unknown> = { message: text }
      if (conversationId) body.conversation_id = conversationId

      await fetchSSE(`/api/v1/agents/${name}/run/stream`, body, {
        onThinking(delta) {
          streamRef.current.thinking += delta
          scheduleFlush()
        },
        onContentDelta(delta) {
          streamRef.current.content += delta
          scheduleFlush()
        },
        onToolCallStart(id, tcName, input) {
          streamRef.current.toolCalls.push({ id, name: tcName, input })
          flush()
        },
        onToolResult(id, _tcName, content, isError) {
          const tc = streamRef.current.toolCalls.find(t => t.id === id)
          if (tc) { tc.result = content; tc.is_error = isError }
          flush()
        },
        async onMcpOAuthRequired(_mcpServerUrl, authorizationUrl) {
          // Open the OAuth authorization URL directly in a popup
          const result = await new Promise<string>((resolve) => {
            const popup = window.open(authorizationUrl, 'mcp_oauth', 'width=600,height=700,scrollbars=yes')
            const poll = setInterval(() => {
              if (!popup || popup.closed) {
                clearInterval(poll)
                resolve('connected')
              }
            }, 500)
          })
          if (conversationId) {
            await api.post('/api/v1/agents/respond', {
              conversation_id: conversationId,
              answer: result,
            }).catch(() => {})
          }
        },
        async onConnectRequired(provider, displayName) {
          // Show the connect popup and wait for completion
          const result = await new Promise<string>((resolve) => {
            connectResolveRef.current = resolve
            setConnectPopup({ provider, displayName })
          })
          setConnectPopup(null)
          connectResolveRef.current = null
          // Notify the engine that the user connected (or cancelled)
          if (conversationId) {
            await api.post('/api/v1/agents/respond', {
              conversation_id: conversationId,
              answer: result,
            }).catch(() => {})
          }
        },
        async onAskUser(_questionId, question, options) {
          // Show the popup and wait for the user's answer
          const answer = await new Promise<string>((resolve) => {
            askResolveRef.current = resolve
            setAskPopup({ question, options })
          })
          setAskPopup(null)
          askResolveRef.current = null
          // POST the answer back to the engine
          if (conversationId) {
            await api.post('/api/v1/agents/respond', {
              conversation_id: conversationId,
              answer,
            }).catch(() => {})
          }
        },
        onDone(data) {
          if (data.conversation_id) setConversationId(data.conversation_id)
          if (flushTimer) { clearTimeout(flushTimer); flushTimer = null }
          flush()
        },
        onError(message) {
          streamRef.current.content += `\n\nError: ${message}`
          flush()
        },
      }, controller.signal)
    } catch (e: unknown) {
      if ((e as Error).name === 'AbortError') {
        streamRef.current.content += '\n\n*Stopped.*'
      } else {
        const err = e as { message?: string }
        streamRef.current.content = `Error: ${err.message ?? 'Unknown error'}`
      }
      flush()
    } finally {
      abortRef.current = null
      if (flushTimer) { clearTimeout(flushTimer); flushTimer = null }
      flush()
      setSending(false)
    }
  }, [sending, conversationId, name])

  const stopStream = useCallback(() => {
    abortRef.current?.abort()
  }, [])

  if (loading) return <p className="text-slate-500">Loading...</p>
  if (!agent) return <p className="text-red-600">{error || 'Agent not found'}</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">{agent.display_name || agent.name}</h1>
        <span className="rounded-full bg-slate-100 px-3 py-1 text-xs font-medium text-slate-600">v{agent.version}</span>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {/* Tabs */}
      <div className="border-b border-slate-200 mb-6">
        <div className="flex gap-6">
          {tabs.map(t => (
            <button key={t} onClick={() => setTab(t)}
              className={`pb-3 text-sm font-medium border-b-2 transition-colors ${tab === t ? 'border-blue-600 text-blue-600' : 'border-transparent text-slate-500 hover:text-slate-700'}`}>
              {t}
            </button>
          ))}
        </div>
      </div>

      {/* Config Tab */}
      {tab === 'Config' && (
        <div className="grid grid-cols-3 gap-6">
          {/* Left: Identity + Prompt Stack */}
          <div className="col-span-2 space-y-6">
            {/* Identity */}
            <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6 space-y-4">
              <h2 className="font-display text-lg tracking-tight text-slate-900">Identity</h2>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-xs font-medium text-slate-500 mb-1">Display Name</label>
                  <input value={displayName} onChange={e => setDisplayName(e.target.value)}
                    className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none" />
                </div>
                <div>
                  <label className="block text-xs font-medium text-slate-500 mb-1">Model Deployment</label>
                  <select value={modelDeployment} onChange={e => setModelDeployment(e.target.value)}
                    className="w-full rounded-lg ring-1 ring-slate-300 bg-white px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none">
                    <option value="">Select a deployment...</option>
                    {deployments.map(d => {
                      const m = models.find(x => x.id === d.model_id)
                      return <option key={d.slug} value={d.slug}>{d.name} ({d.slug}){m ? ` \u2014 ${m.display_name}` : ''}</option>
                    })}
                  </select>
                </div>
              </div>
              <div>
                <label className="block text-xs font-medium text-slate-500 mb-1">Description</label>
                <textarea value={description} onChange={e => setDescription(e.target.value)} rows={2}
                  className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none" />
              </div>
            </div>

            {/* Prompt Stack */}
            <PromptStackEditor blocks={blocks} setBlocks={setBlocks} totalTokens={totalTokens} availableAgents={availableAgents} />

            {/* Tools */}
            <ToolsEditor builtinTools={builtinTools} setBuiltinTools={setBuiltinTools} mcpConnectionNames={mcpConnectionNames} setMcpConnectionNames={setMcpConnectionNames} />

            <button onClick={saveConfig} disabled={saving}
              className="bg-blue-600 text-white px-6 py-2.5 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
              {saving ? 'Saving...' : 'Save Configuration'}
            </button>
          </div>

          {/* Right: Token budget summary */}
          <div className="space-y-4">
            <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6 sticky top-8">
              <h3 className="font-display text-base tracking-tight text-slate-900 mb-4">Token Budget</h3>
              <div className="space-y-2">
                {blocks.map((b, i) => {
                  const tokens = b.type === 'text' ? Math.ceil(((b as TextBlock).content?.length || 0) / 4)
                    : b.type === 'knowledge' ? (b as KnowledgeBlock).budget_tokens
                    : b.type === 'datasource' ? (b as DatasourceBlock).budget_tokens
                    : 50
                  return (
                    <div key={i} className="flex justify-between text-sm">
                      <span className="text-slate-600 truncate">{b.label || b.type}</span>
                      <span className="text-slate-400 tabular-nums">{tokens.toLocaleString()}</span>
                    </div>
                  )
                })}
                {blocks.length > 0 && (
                  <div className="flex justify-between text-sm font-semibold border-t border-slate-100 pt-2 mt-2">
                    <span className="text-slate-900">Total</span>
                    <span className="text-blue-600 tabular-nums">~{totalTokens.toLocaleString()}</span>
                  </div>
                )}
              </div>
              <div className="mt-4 text-xs text-slate-400">
                {blocks.length} blocks | {Object.keys(builtinTools).length} tools | {mcpConnectionNames.length} MCP {mcpConnectionNames.length === 1 ? 'server' : 'servers'}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Chat Tab */}
      {tab === 'Chat' && (
        <div className="flex flex-col" style={{ height: 'calc(100vh - 280px)' }}>
          {/* System prompt toggle */}
          <div className="mb-3 flex items-center gap-3">
            <button onClick={loadSystemPrompt} disabled={promptLoading}
              className="rounded-lg ring-1 ring-slate-300 px-3 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-50 transition-colors disabled:opacity-50">
              {promptLoading ? 'Loading...' : showPrompt ? 'Hide System Prompt' : 'Show System Prompt'}
            </button>
            <span className="text-xs text-slate-400">Debug: view the fully assembled prompt as sent to the model</span>
          </div>

          {/* System prompt panel */}
          {showPrompt && systemPrompt && (
            <div className="mb-3 bg-amber-50 rounded-2xl ring-1 ring-amber-200 overflow-hidden" style={{ maxHeight: '40vh' }}>
              <div className="flex items-center justify-between px-4 py-2 bg-amber-100/50 border-b border-amber-200">
                <span className="text-xs font-semibold text-amber-800">System Prompt ({Math.ceil(systemPrompt.length / 4).toLocaleString()} est. tokens)</span>
                <button onClick={() => setShowPrompt(false)} className="text-amber-600 hover:text-amber-800 text-xs">&times; Close</button>
              </div>
              <pre className="p-4 text-xs font-mono text-amber-900 whitespace-pre-wrap overflow-y-auto" style={{ maxHeight: 'calc(40vh - 36px)' }}>{systemPrompt}</pre>
            </div>
          )}

          <ChatPanel
            messages={messages}
            loading={sending}
            onSend={sendMessage}
            onStop={stopStream}
            emptyStateText="Send a message to start chatting with the agent"
          />

          {/* Ask-user popup */}
          {askPopup && <AskUserPopup
            question={askPopup.question}
            options={askPopup.options}
            onSubmit={(answer) => askResolveRef.current?.(answer)}
          />}

          {/* Connect-required popup */}
          {connectPopup && <ConnectProviderPopup
            provider={connectPopup.provider}
            displayName={connectPopup.displayName}
            onComplete={(result) => connectResolveRef.current?.(result)}
          />}
        </div>
      )}

      {/* YAML Tab */}
      {tab === 'YAML' && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
          <pre className="text-sm font-mono text-slate-800 whitespace-pre-wrap overflow-x-auto">{yaml || 'Loading...'}</pre>
        </div>
      )}
    </div>
  )
}
