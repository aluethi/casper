import { useState, useEffect, useRef, useCallback } from 'react'
import { useParams } from 'react-router-dom'
import api from '../lib/api'
import type { Agent } from '../types'

// ── Prompt stack block types ─────────────────────────────────────
interface BlockBase { type: string; label: string }
interface TextBlock extends BlockBase { type: 'text'; content: string }
interface EnvironmentBlock extends BlockBase { type: 'environment'; include: string[] }
interface AgentMemoryBlock extends BlockBase { type: 'agent_memory' }
interface TenantMemoryBlock extends BlockBase { type: 'tenant_memory' }
interface KnowledgeBlock extends BlockBase { type: 'knowledge'; budget_tokens: number }
interface DelegatesBlock extends BlockBase { type: 'delegates'; agents: { name: string; description: string; when: string }[] }
interface DatasourceBlock extends BlockBase { type: 'datasource'; source: Record<string, unknown>; budget_tokens: number; on_missing: string }
interface SnippetBlock extends BlockBase { type: 'snippet'; snippet_id: string }
interface VariableBlock extends BlockBase { type: 'variable'; key: string; value: string }
type PromptBlock = TextBlock | EnvironmentBlock | AgentMemoryBlock | TenantMemoryBlock | KnowledgeBlock | DelegatesBlock | DatasourceBlock | SnippetBlock | VariableBlock

const BLOCK_TYPES = [
  { type: 'text', label: 'Text', icon: 'T', color: 'blue', desc: 'Static instructions (markdown)' },
  { type: 'environment', label: 'Environment', icon: 'E', color: 'green', desc: 'Runtime context (datetime, tenant)' },
  { type: 'agent_memory', label: 'Agent Memory', icon: 'M', color: 'purple', desc: "Agent's versioned memory document" },
  { type: 'tenant_memory', label: 'Tenant Memory', icon: 'S', color: 'indigo', desc: 'Shared tenant memory (read-only)' },
  { type: 'knowledge', label: 'Knowledge', icon: 'K', color: 'amber', desc: 'RAG retrieval from knowledge base' },
  { type: 'delegates', label: 'Delegates', icon: 'D', color: 'rose', desc: 'Available sub-agents' },
  { type: 'datasource', label: 'Datasource', icon: 'DS', color: 'cyan', desc: 'External data fetched at assembly' },
  { type: 'snippet', label: 'Snippet', icon: 'Sn', color: 'orange', desc: 'Reusable block from snippet library' },
  { type: 'variable', label: 'Variable', icon: 'V', color: 'slate', desc: 'Custom key-value pair' },
] as const

function createBlock(type: string): PromptBlock {
  const base = { label: BLOCK_TYPES.find(b => b.type === type)?.label || type }
  switch (type) {
    case 'text': return { ...base, type: 'text', content: '' }
    case 'environment': return { ...base, type: 'environment', include: ['datetime', 'tenant_name', 'agent_name'] }
    case 'agent_memory': return { ...base, type: 'agent_memory' }
    case 'tenant_memory': return { ...base, type: 'tenant_memory' }
    case 'knowledge': return { ...base, type: 'knowledge', budget_tokens: 2000 }
    case 'delegates': return { ...base, type: 'delegates', agents: [] }
    case 'datasource': return { ...base, type: 'datasource', source: { type: 'mcp' }, budget_tokens: 500, on_missing: 'skip' }
    case 'snippet': return { ...base, type: 'snippet', snippet_id: '' }
    case 'variable': return { ...base, type: 'variable', key: '', value: '' }
    default: return { ...base, type: 'text', content: '' } as TextBlock
  }
}

const ENV_FIELDS = ['datetime', 'tenant_name', 'tenant_slug', 'agent_name', 'agent_display_name', 'invocation_source']

// ── Per-type block editors ───────────────────────────────────────
function BlockEditor({ block, onChange }: { block: PromptBlock; onChange: (b: PromptBlock) => void }) {
  switch (block.type) {
    case 'text':
      return (
        <div>
          <label className="block text-xs font-medium text-slate-500 mb-1">Content (markdown)</label>
          <textarea value={block.content} onChange={e => onChange({ ...block, content: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none font-mono" rows={4} placeholder="You are the triage agent..." />
        </div>
      )
    case 'environment':
      return (
        <div>
          <label className="block text-xs font-medium text-slate-500 mb-1">Include fields</label>
          <div className="flex flex-wrap gap-2">
            {ENV_FIELDS.map(f => (
              <label key={f} className="flex items-center gap-1.5 text-sm text-slate-700">
                <input type="checkbox" checked={block.include.includes(f)}
                  onChange={e => {
                    const include = e.target.checked ? [...block.include, f] : block.include.filter(x => x !== f)
                    onChange({ ...block, include })
                  }}
                  className="rounded" />
                {f}
              </label>
            ))}
          </div>
        </div>
      )
    case 'agent_memory':
    case 'tenant_memory':
      return <p className="text-xs text-slate-500 italic">This block loads the {block.type === 'agent_memory' ? 'agent' : 'tenant'} memory document automatically. No configuration needed.</p>
    case 'knowledge':
      return (
        <div>
          <label className="block text-xs font-medium text-slate-500 mb-1">Token budget: {block.budget_tokens.toLocaleString()}</label>
          <input type="range" min={100} max={10000} step={100} value={block.budget_tokens}
            onChange={e => onChange({ ...block, budget_tokens: +e.target.value })}
            className="w-full accent-blue-600" />
          <div className="flex justify-between text-xs text-slate-400"><span>100</span><span>10,000</span></div>
        </div>
      )
    case 'delegates':
      return (
        <div className="space-y-2">
          <label className="block text-xs font-medium text-slate-500 mb-1">Sub-agents</label>
          {block.agents.map((a, i) => (
            <div key={i} className="flex gap-2 items-start">
              <input placeholder="Agent name" value={a.name} onChange={e => {
                const agents = [...block.agents]; agents[i] = { ...a, name: e.target.value }; onChange({ ...block, agents })
              }} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm flex-1" />
              <input placeholder="Description" value={a.description} onChange={e => {
                const agents = [...block.agents]; agents[i] = { ...a, description: e.target.value }; onChange({ ...block, agents })
              }} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm flex-1" />
              <input placeholder="When to use" value={a.when} onChange={e => {
                const agents = [...block.agents]; agents[i] = { ...a, when: e.target.value }; onChange({ ...block, agents })
              }} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm flex-1" />
              <button onClick={() => { const agents = block.agents.filter((_, j) => j !== i); onChange({ ...block, agents }) }}
                className="text-red-500 hover:text-red-400 text-sm px-1">x</button>
            </div>
          ))}
          <button onClick={() => onChange({ ...block, agents: [...block.agents, { name: '', description: '', when: '' }] })}
            className="text-blue-600 hover:text-blue-500 text-xs font-medium">+ Add agent</button>
        </div>
      )
    case 'datasource':
      return (
        <div className="space-y-2">
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className="block text-xs font-medium text-slate-500 mb-1">Source (JSON)</label>
              <textarea value={JSON.stringify(block.source, null, 2)} onChange={e => {
                try { onChange({ ...block, source: JSON.parse(e.target.value) }) } catch { /* ignore parse errors while typing */ }
              }} className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-xs font-mono" rows={3} />
            </div>
            <div>
              <label className="block text-xs font-medium text-slate-500 mb-1">Budget: {block.budget_tokens}</label>
              <input type="range" min={100} max={5000} step={100} value={block.budget_tokens}
                onChange={e => onChange({ ...block, budget_tokens: +e.target.value })} className="w-full accent-blue-600" />
              <label className="block text-xs font-medium text-slate-500 mt-2">On missing</label>
              <select value={block.on_missing} onChange={e => onChange({ ...block, on_missing: e.target.value })}
                className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm w-full">
                <option value="skip">Skip</option>
                <option value="fail">Fail</option>
              </select>
            </div>
          </div>
        </div>
      )
    case 'snippet':
      return (
        <div>
          <label className="block text-xs font-medium text-slate-500 mb-1">Snippet ID</label>
          <input placeholder="UUID of the snippet" value={block.snippet_id} onChange={e => onChange({ ...block, snippet_id: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm" />
        </div>
      )
    case 'variable':
      return (
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block text-xs font-medium text-slate-500 mb-1">Key</label>
            <input value={block.key} onChange={e => onChange({ ...block, key: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm" placeholder="escalation_email" />
          </div>
          <div>
            <label className="block text-xs font-medium text-slate-500 mb-1">Value</label>
            <input value={block.value} onChange={e => onChange({ ...block, value: e.target.value })}
              className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm" placeholder="oncall@acme.com" />
          </div>
        </div>
      )
    default:
      return <p className="text-xs text-slate-400">Unknown block type</p>
  }
}

// ── Built-in tool config types ───────────────────────────────────
const BUILTIN_TOOLS = [
  { name: 'delegate', label: 'Delegate', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 300 }, { key: 'max_depth', label: 'Max depth', type: 'number', default: 3 }] },
  { name: 'ask_user', label: 'Ask User', fields: [] },
  { name: 'knowledge_search', label: 'Knowledge Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 5 }, { key: 'relevance_threshold', label: 'Threshold', type: 'number', default: 0.7 }] },
  { name: 'update_memory', label: 'Update Memory', fields: [{ key: 'max_document_tokens', label: 'Max tokens', type: 'number', default: 4000 }] },
  { name: 'web_search', label: 'Web Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 10 }] },
  { name: 'web_fetch', label: 'Web Fetch', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 30 }, { key: 'max_response_bytes', label: 'Max bytes', type: 'number', default: 1048576 }] },
]

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
  const [showAddBlock, setShowAddBlock] = useState(false)
  const [expandedBlock, setExpandedBlock] = useState<number | null>(null)
  const [dragIdx, setDragIdx] = useState<number | null>(null)

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

  // Drag & drop reorder
  const moveBlock = useCallback((from: number, to: number) => {
    setBlocks(prev => {
      const next = [...prev]
      const [moved] = next.splice(from, 1)
      next.splice(to, 0, moved)
      return next
    })
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

      {/* ══════════ CONFIG TAB ══════════ */}
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
            <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
              <div className="flex items-center justify-between mb-4">
                <h2 className="font-display text-lg tracking-tight text-slate-900">Prompt Stack</h2>
                <div className="flex items-center gap-3">
                  <span className="text-xs text-slate-400">~{totalTokens.toLocaleString()} tokens</span>
                  <div className="relative">
                    <button onClick={() => setShowAddBlock(!showAddBlock)}
                      className="bg-blue-600 text-white px-3 py-1.5 rounded-full text-xs font-semibold hover:bg-blue-500 transition-colors">
                      + Add Block
                    </button>
                    {showAddBlock && (
                      <div className="absolute right-0 top-full mt-1 z-20 bg-white rounded-xl ring-1 ring-slate-900/10 shadow-xl p-2 w-64">
                        {BLOCK_TYPES.map(bt => (
                          <button key={bt.type} onClick={() => { setBlocks([...blocks, createBlock(bt.type)]); setShowAddBlock(false); setExpandedBlock(blocks.length) }}
                            className="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left hover:bg-slate-50 transition-colors">
                            <span className={`flex h-7 w-7 items-center justify-center rounded-lg text-xs font-bold bg-${bt.color}-100 text-${bt.color}-700`}>{bt.icon}</span>
                            <div>
                              <p className="text-sm font-medium text-slate-900">{bt.label}</p>
                              <p className="text-xs text-slate-500">{bt.desc}</p>
                            </div>
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              </div>

              {blocks.length === 0 && (
                <div className="text-center py-8 text-slate-400 text-sm">
                  No blocks yet. Click "+ Add Block" to build your prompt stack.
                </div>
              )}

              <div className="space-y-2">
                {blocks.map((block, i) => {
                  const meta = BLOCK_TYPES.find(b => b.type === block.type)
                  const isExpanded = expandedBlock === i
                  return (
                    <div key={i}
                      draggable
                      onDragStart={() => setDragIdx(i)}
                      onDragOver={e => { e.preventDefault() }}
                      onDrop={() => { if (dragIdx !== null && dragIdx !== i) moveBlock(dragIdx, i); setDragIdx(null) }}
                      onDragEnd={() => setDragIdx(null)}
                      className={`rounded-xl ring-1 ring-slate-200 transition-all ${dragIdx === i ? 'opacity-40' : ''} ${isExpanded ? 'ring-blue-300 shadow-sm' : 'hover:ring-slate-300'}`}>
                      {/* Block header */}
                      <div className="flex items-center gap-3 px-4 py-3 cursor-grab active:cursor-grabbing"
                        onClick={() => setExpandedBlock(isExpanded ? null : i)}>
                        <span className="text-slate-300 select-none">⠿</span>
                        <span className={`flex h-7 w-7 items-center justify-center rounded-lg text-xs font-bold bg-${meta?.color || 'slate'}-100 text-${meta?.color || 'slate'}-700`}>
                          {meta?.icon || '?'}
                        </span>
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-medium text-slate-900">{block.label || meta?.label}</span>
                            <span className={`rounded-full px-2 py-0.5 text-[10px] font-medium bg-${meta?.color || 'slate'}-50 text-${meta?.color || 'slate'}-600`}>{block.type}</span>
                          </div>
                        </div>
                        <button onClick={e => { e.stopPropagation(); setBlocks(blocks.filter((_, j) => j !== i)); if (expandedBlock === i) setExpandedBlock(null) }}
                          className="text-slate-300 hover:text-red-500 transition-colors text-sm p-1">x</button>
                      </div>
                      {/* Block editor (expanded) */}
                      {isExpanded && (
                        <div className="px-4 pb-4 border-t border-slate-100 pt-3">
                          <div className="mb-3">
                            <label className="block text-xs font-medium text-slate-500 mb-1">Label</label>
                            <input value={block.label} onChange={e => { const next = [...blocks]; next[i] = { ...block, label: e.target.value }; setBlocks(next) }}
                              className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm w-full" />
                          </div>
                          <BlockEditor block={block} onChange={b => { const next = [...blocks]; next[i] = b; setBlocks(next) }} />
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>
            </div>

            {/* Tools */}
            <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
              <h2 className="font-display text-lg tracking-tight text-slate-900 mb-4">Built-in Tools</h2>
              <div className="space-y-3">
                {BUILTIN_TOOLS.map(tool => {
                  const enabled = tool.name in builtinTools
                  const config = builtinTools[tool.name] || {}
                  return (
                    <div key={tool.name} className={`rounded-xl ring-1 px-4 py-3 transition-all ${enabled ? 'ring-blue-200 bg-blue-50/30' : 'ring-slate-200'}`}>
                      <div className="flex items-center justify-between">
                        <label className="flex items-center gap-3 cursor-pointer">
                          <input type="checkbox" checked={enabled} onChange={e => {
                            const next = { ...builtinTools }
                            if (e.target.checked) {
                              const defaults: Record<string, unknown> = {}
                              tool.fields.forEach(f => { defaults[f.key] = f.default })
                              next[tool.name] = defaults
                            } else { delete next[tool.name] }
                            setBuiltinTools(next)
                          }} className="rounded border-slate-300 text-blue-600 focus:ring-blue-500" />
                          <span className="text-sm font-medium text-slate-900">{tool.label}</span>
                        </label>
                      </div>
                      {enabled && tool.fields.length > 0 && (
                        <div className="mt-3 flex gap-4 pl-8">
                          {tool.fields.map(f => (
                            <div key={f.key}>
                              <label className="block text-xs text-slate-500 mb-0.5">{f.label}</label>
                              <input type="number" value={(config[f.key] as number) ?? f.default}
                                onChange={e => setBuiltinTools({ ...builtinTools, [tool.name]: { ...config, [f.key]: +e.target.value } })}
                                className="w-28 rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>
            </div>

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

      {/* ══════════ CHAT TAB ══════════ */}
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

      {/* ══════════ YAML TAB ══════════ */}
      {tab === 'YAML' && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
          <pre className="text-sm font-mono text-slate-800 whitespace-pre-wrap overflow-x-auto">{yaml || 'Loading...'}</pre>
        </div>
      )}
    </div>
  )
}
