import { useState, useEffect, useRef } from 'react'
import { useParams } from 'react-router-dom'
import api from '../lib/api'
import type { Agent } from '../types'
import PromptStackEditor, { type PromptBlock, type AvailableAgent } from './components/PromptStackEditor'
import ToolsEditor from './components/ToolsEditor'

interface TextBlock { type: 'text'; label: string; content: string }
interface KnowledgeBlock { type: 'knowledge'; label: string; budget_tokens: number }
interface DatasourceBlock { type: 'datasource'; label: string; source: Record<string, unknown>; budget_tokens: number; on_missing: string }

interface ChatMessage { role: 'user' | 'assistant'; content: string; tool_calls?: { name: string; input: Record<string, unknown>; output?: string }[] }

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
  const [saving, setSaving] = useState(false)

  // Available agents (for delegates block)
  const [availableAgents, setAvailableAgents] = useState<AvailableAgent[]>([])

  // Chat
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [input, setInput] = useState('')
  const [sending, setSending] = useState(false)
  const chatEnd = useRef<HTMLDivElement>(null)

  // YAML
  const [yaml, setYaml] = useState('')

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
      })
      .catch(e => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
    // Fetch all agents for delegate picker (exclude current agent)
    api.get('/api/v1/agents').then(r => {
      const list = (r.data.data || r.data || []) as AvailableAgent[]
      setAvailableAgents(list.filter(a => a.name !== name))
    }).catch(() => {})
  }, [name])

  useEffect(() => { chatEnd.current?.scrollIntoView({ behavior: 'smooth' }) }, [messages])
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
        tools: { builtin, mcp: agent?.tools?.mcp || [] },
      })
      // Reload
      const r = await api.get(`/api/v1/agents/${name}`)
      setAgent(r.data)
    } catch (e: any) { setError(e.response?.data?.message ?? e.message) }
    finally { setSaving(false) }
  }

  const sendMessage = async () => {
    if (!input.trim() || sending) return
    setMessages(m => [...m, { role: 'user', content: input }])
    setInput(''); setSending(true)
    try {
      const res = await api.post(`/api/v1/agents/${name}/run`, { message: input })
      const data = res.data
      setMessages(m => [...m, { role: 'assistant', content: data.message?.content || data.content || JSON.stringify(data), tool_calls: data.tool_calls }])
    } catch (e: any) {
      setMessages(m => [...m, { role: 'assistant', content: `Error: ${e.response?.data?.message ?? e.message}` }])
    } finally { setSending(false) }
  }

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
                  <input value={modelDeployment} onChange={e => setModelDeployment(e.target.value)}
                    className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none" placeholder="sonnet-fast" />
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
            <ToolsEditor builtinTools={builtinTools} setBuiltinTools={setBuiltinTools} />

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
                {blocks.length} blocks | {Object.keys(builtinTools).length} tools enabled
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Chat Tab */}
      {tab === 'Chat' && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm flex flex-col" style={{ height: 'calc(100vh - 280px)' }}>
          <div className="flex-1 overflow-y-auto p-4 space-y-3">
            {messages.length === 0 && <p className="text-slate-400 text-sm text-center mt-8">Send a message to start chatting with the agent.</p>}
            {messages.map((m, i) => (
              <div key={i} className={`flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                <div className={`max-w-[70%] rounded-2xl px-4 py-2.5 text-sm ${m.role === 'user' ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-900'}`}>
                  <p className="whitespace-pre-wrap">{m.content}</p>
                  {m.tool_calls?.map((tc, j) => (
                    <div key={j} className="mt-2 text-xs bg-white/20 rounded-lg p-2">
                      <p className="font-semibold">Tool: {tc.name}</p>
                      <pre className="mt-1 overflow-x-auto text-[11px]">{JSON.stringify(tc.input, null, 2)}</pre>
                    </div>
                  ))}
                </div>
              </div>
            ))}
            <div ref={chatEnd} />
          </div>
          <div className="border-t border-slate-200 p-3 flex gap-2">
            <input value={input} onChange={e => setInput(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage() } }}
              placeholder="Type a message..."
              className="flex-1 rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none" />
            <button onClick={sendMessage} disabled={sending}
              className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 transition-colors disabled:opacity-50">
              {sending ? '...' : 'Send'}
            </button>
          </div>
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
