import { useState, useEffect, useRef } from 'react'
import { useParams } from 'react-router-dom'
import api from '../lib/api'
import type { Agent } from '../types'

interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
  tool_calls?: { name: string; input: Record<string, unknown>; output?: string }[]
}

const tabs = ['Config', 'Chat', 'YAML'] as const
type Tab = (typeof tabs)[number]

export default function AgentBuilderPage() {
  const { name } = useParams<{ name: string }>()
  const [tab, setTab] = useState<Tab>('Config')
  const [agent, setAgent] = useState<Agent | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  // Config state
  const [config, setConfig] = useState({ display_name: '', description: '', model_deployment: '', prompts: '[]', tools: '[]' })
  const [saving, setSaving] = useState(false)

  // Chat state
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [input, setInput] = useState('')
  const [sending, setSending] = useState(false)
  const chatEnd = useRef<HTMLDivElement>(null)

  // YAML state
  const [yaml, setYaml] = useState('')

  useEffect(() => {
    api.get(`/api/v1/agents/${name}`)
      .then((r) => {
        const a = r.data
        setAgent(a)
        setConfig({
          display_name: a.display_name || '',
          description: a.description || '',
          model_deployment: a.model_deployment || '',
          prompts: JSON.stringify(a.prompts ?? [], null, 2),
          tools: JSON.stringify(a.tools ?? [], null, 2),
        })
      })
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }, [name])

  useEffect(() => { chatEnd.current?.scrollIntoView({ behavior: 'smooth' }) }, [messages])

  useEffect(() => {
    if (tab === 'YAML') {
      api.get(`/api/v1/agents/${name}/export`).then((r) => setYaml(typeof r.data === 'string' ? r.data : JSON.stringify(r.data, null, 2)))
        .catch(() => setYaml('# Export not available'))
    }
  }, [tab, name])

  const saveConfig = async () => {
    setSaving(true)
    setError('')
    try {
      let prompts, tools
      try { prompts = JSON.parse(config.prompts) } catch { prompts = [] }
      try { tools = JSON.parse(config.tools) } catch { tools = [] }
      await api.put(`/api/v1/agents/${name}`, {
        display_name: config.display_name,
        description: config.description,
        model_deployment: config.model_deployment,
        prompts,
        tools,
      })
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const sendMessage = async () => {
    if (!input.trim() || sending) return
    const userMsg: ChatMessage = { role: 'user', content: input }
    setMessages((m) => [...m, userMsg])
    setInput('')
    setSending(true)
    try {
      const res = await api.post(`/api/v1/agents/${name}/run`, { message: input, history: messages })
      const reply: ChatMessage = {
        role: 'assistant',
        content: res.data.content ?? res.data.message ?? JSON.stringify(res.data),
        tool_calls: res.data.tool_calls,
      }
      setMessages((m) => [...m, reply])
    } catch (e: any) {
      setMessages((m) => [...m, { role: 'assistant', content: `Error: ${e.response?.data?.message ?? e.message}` }])
    } finally {
      setSending(false)
    }
  }

  if (loading) return <p className="text-slate-500">Loading...</p>
  if (!agent) return <p className="text-red-600">{error || 'Agent not found'}</p>

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">{agent.display_name || agent.name}</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      <div className="border-b border-slate-200 mb-4">
        <div className="flex gap-4">
          {tabs.map((t) => (
            <button key={t} onClick={() => setTab(t)}
              className={`pb-2 text-sm font-medium border-b-2 ${tab === t ? 'border-blue-600 text-blue-600' : 'border-transparent text-slate-500 hover:text-slate-700'}`}>
              {t}
            </button>
          ))}
        </div>
      </div>

      {tab === 'Config' && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4 space-y-4">
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">Display Name</label>
            <input value={config.display_name} onChange={(e) => setConfig({ ...config, display_name: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">Description</label>
            <textarea value={config.description} onChange={(e) => setConfig({ ...config, description: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" rows={2} />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">Model Deployment</label>
            <input value={config.model_deployment} onChange={(e) => setConfig({ ...config, model_deployment: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">Prompt Stack (JSON)</label>
            <textarea value={config.prompts} onChange={(e) => setConfig({ ...config, prompts: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow font-mono" rows={6} />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">Tools Config (JSON)</label>
            <textarea value={config.tools} onChange={(e) => setConfig({ ...config, tools: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow font-mono" rows={6} />
          </div>
          <button onClick={saveConfig} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
            {saving ? 'Saving...' : 'Save Configuration'}
          </button>
        </div>
      )}

      {tab === 'Chat' && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm flex flex-col" style={{ height: 'calc(100vh - 280px)' }}>
          <div className="flex-1 overflow-y-auto p-4 space-y-3">
            {messages.length === 0 && <p className="text-slate-400 text-sm">Send a message to start chatting with the agent.</p>}
            {messages.map((m, i) => (
              <div key={i} className={`flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                <div className={`max-w-[70%] rounded-2xl px-3 py-2 text-sm ${m.role === 'user' ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-900'}`}>
                  <p className="whitespace-pre-wrap">{m.content}</p>
                  {m.tool_calls?.map((tc, j) => (
                    <div key={j} className="mt-2 text-xs bg-slate-200 rounded-lg p-2">
                      <p className="font-semibold">Tool: {tc.name}</p>
                      <pre className="mt-1 overflow-x-auto">{JSON.stringify(tc.input, null, 2)}</pre>
                      {tc.output && <pre className="mt-1 text-slate-600">{tc.output}</pre>}
                    </div>
                  ))}
                </div>
              </div>
            ))}
            <div ref={chatEnd} />
          </div>
          <div className="border-t border-slate-200 p-3 flex gap-2">
            <input value={input} onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage() } }}
              placeholder="Type a message..." className="flex-1 rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <button onClick={sendMessage} disabled={sending}
              className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
              {sending ? '...' : 'Send'}
            </button>
          </div>
        </div>
      )}

      {tab === 'YAML' && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4">
          <pre className="text-sm font-mono text-slate-800 whitespace-pre-wrap overflow-x-auto">{yaml || 'Loading...'}</pre>
        </div>
      )}
    </div>
  )
}
