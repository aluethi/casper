import { useState, useCallback } from 'react'

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
export type PromptBlock = TextBlock | EnvironmentBlock | AgentMemoryBlock | TenantMemoryBlock | KnowledgeBlock | DelegatesBlock | DatasourceBlock | SnippetBlock | VariableBlock

export const BLOCK_TYPES = [
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

export function createBlock(type: string): PromptBlock {
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

export interface AvailableAgent { name: string; display_name: string; description: string | null }

// ── Per-type block editors ───────────────────────────────────────
function BlockEditor({ block, onChange, availableAgents }: { block: PromptBlock; onChange: (b: PromptBlock) => void; availableAgents: AvailableAgent[] }) {
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
    case 'delegates': {
      const addedNames = new Set(block.agents.map(a => a.name))
      const remaining = availableAgents.filter(a => !addedNames.has(a.name))
      return (
        <div className="space-y-2">
          <label className="block text-xs font-medium text-slate-500 mb-1">Sub-agents</label>
          {block.agents.map((a, i) => (
            <div key={i} className="rounded-lg ring-1 ring-slate-200 bg-slate-50/50 px-3 py-2.5">
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-sm font-medium text-slate-900">{a.name}</span>
                <button onClick={() => { const agents = block.agents.filter((_, j) => j !== i); onChange({ ...block, agents }) }}
                  className="text-slate-300 hover:text-red-500 transition-colors text-sm">x</button>
              </div>
              <p className="text-xs text-slate-500 mb-1.5">{a.description || 'No description'}</p>
              <div>
                <label className="block text-[10px] font-medium text-slate-400 uppercase tracking-wide mb-0.5">When to delegate</label>
                <input placeholder="e.g. Build failures or pipeline issues" value={a.when} onChange={e => {
                  const agents = [...block.agents]; agents[i] = { ...a, when: e.target.value }; onChange({ ...block, agents })
                }} className="w-full rounded ring-1 ring-slate-300 px-2 py-1 text-sm focus:ring-2 focus:ring-blue-600 focus:outline-none" />
              </div>
            </div>
          ))}
          {remaining.length > 0 ? (
            <div className="relative">
              <select
                value=""
                onChange={e => {
                  const selected = availableAgents.find(a => a.name === e.target.value)
                  if (selected) {
                    onChange({ ...block, agents: [...block.agents, { name: selected.name, description: selected.description || selected.display_name, when: '' }] })
                  }
                }}
                className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1.5 text-sm text-blue-600 font-medium bg-white hover:bg-slate-50 cursor-pointer focus:ring-2 focus:ring-blue-600 focus:outline-none"
              >
                <option value="">+ Add agent...</option>
                {remaining.map(a => (
                  <option key={a.name} value={a.name}>{a.display_name || a.name} — {a.description || 'No description'}</option>
                ))}
              </select>
            </div>
          ) : (
            <p className="text-xs text-slate-400 italic">All available agents have been added.</p>
          )}
        </div>
      )
    }
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

// ── Prompt Stack Editor component ───────────────────────────────

interface PromptStackEditorProps {
  blocks: PromptBlock[]
  setBlocks: React.Dispatch<React.SetStateAction<PromptBlock[]>>
  totalTokens: number
  availableAgents: AvailableAgent[]
}

export default function PromptStackEditor({ blocks, setBlocks, totalTokens, availableAgents }: PromptStackEditorProps) {
  const [showAddBlock, setShowAddBlock] = useState(false)
  const [expandedBlock, setExpandedBlock] = useState<number | null>(null)
  const [dragIdx, setDragIdx] = useState<number | null>(null)
  const [editingLabel, setEditingLabel] = useState<number | null>(null)

  // Drag & drop reorder
  const moveBlock = useCallback((from: number, to: number) => {
    setBlocks(prev => {
      const next = [...prev]
      const [moved] = next.splice(from, 1)
      next.splice(to, 0, moved)
      return next
    })
  }, [setBlocks])

  return (
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
                <span className="text-slate-300 select-none">&#x2807;</span>
                <span className={`flex h-7 w-7 items-center justify-center rounded-lg text-xs font-bold bg-${meta?.color || 'slate'}-100 text-${meta?.color || 'slate'}-700`}>
                  {meta?.icon || '?'}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    {editingLabel === i ? (
                      <input
                        autoFocus
                        value={block.label}
                        onClick={e => e.stopPropagation()}
                        onChange={e => { const next = [...blocks]; next[i] = { ...block, label: e.target.value }; setBlocks(next) }}
                        onBlur={() => setEditingLabel(null)}
                        onKeyDown={e => { if (e.key === 'Enter') setEditingLabel(null) }}
                        className="rounded ring-1 ring-blue-300 px-1.5 py-0.5 text-sm font-medium text-slate-900 focus:outline-none focus:ring-2 focus:ring-blue-500 w-48" />
                    ) : (
                      <>
                        <span className="text-sm font-medium text-slate-900">{block.label || meta?.label}</span>
                        <button onClick={e => { e.stopPropagation(); setEditingLabel(i) }}
                          className="text-slate-300 hover:text-slate-500 transition-colors" title="Rename">
                          <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L10.582 16.07a4.5 4.5 0 01-1.897 1.13L6 18l.8-2.685a4.5 4.5 0 011.13-1.897l8.932-8.931z" />
                          </svg>
                        </button>
                      </>
                    )}
                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-medium bg-${meta?.color || 'slate'}-50 text-${meta?.color || 'slate'}-600`}>{block.type}</span>
                  </div>
                </div>
                <button onClick={e => { e.stopPropagation(); setBlocks(blocks.filter((_, j) => j !== i)); if (expandedBlock === i) setExpandedBlock(null) }}
                  className="text-slate-300 hover:text-red-500 transition-colors text-sm p-1">x</button>
              </div>
              {/* Block editor (expanded) */}
              {isExpanded && (
                <div className="px-4 pb-4 border-t border-slate-100 pt-3">
                  <BlockEditor block={block} onChange={b => { const next = [...blocks]; next[i] = b; setBlocks(next) }} availableAgents={availableAgents} />
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
