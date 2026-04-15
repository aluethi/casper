import { useEffect, useState } from 'react'
import api from '../../lib/api'

interface Model {
  id: string; name: string; display_name: string; provider: string;
  cap_chat: boolean; cap_embedding: boolean; cap_thinking: boolean; cap_vision: boolean;
  cap_tool_use: boolean; cap_json_output: boolean;
  context_window: number | null; max_output_tokens: number | null;
  cost_per_1k_input: number | null; cost_per_1k_output: number | null;
  published: boolean; is_active: boolean; created_at: string;
}

export default function ModelsPage() {
  const [models, setModels] = useState<Model[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showCreate, setShowCreate] = useState(false)
  const [editId, setEditId] = useState<string | null>(null)
  const [form, setForm] = useState({
    name: '', display_name: '', provider: 'anthropic',
    cap_chat: true, cap_embedding: false, cap_thinking: false, cap_vision: false,
    cap_tool_use: false, cap_json_output: false,
    context_window: 200000, max_output_tokens: 8192,
    cost_per_1k_input: 0.003, cost_per_1k_output: 0.015, published: false,
  })
  const [editForm, setEditForm] = useState({ display_name: '', context_window: 0, max_output_tokens: 0, cost_per_1k_input: 0, cost_per_1k_output: 0 })

  const load = () => {
    api.get('/api/v1/models').then(r => {
      setModels(r.data.data || r.data)
      setLoading(false)
    }).catch(e => { setError(e.message); setLoading(false) })
  }
  useEffect(load, [])

  const create = () => {
    api.post('/api/v1/models', form).then(() => {
      setShowCreate(false)
      setForm({ ...form, name: '', display_name: '' })
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const togglePublish = (m: Model) => {
    api.patch(`/api/v1/models/${m.id}`, { published: !m.published }).then(load)
  }

  const toggleActive = (m: Model) => {
    api.patch(`/api/v1/models/${m.id}`, { is_active: !m.is_active }).then(load)
  }

  const startEdit = (m: Model) => {
    setEditId(m.id)
    setEditForm({
      display_name: m.display_name,
      context_window: m.context_window ?? 0,
      max_output_tokens: m.max_output_tokens ?? 0,
      cost_per_1k_input: m.cost_per_1k_input ?? 0,
      cost_per_1k_output: m.cost_per_1k_output ?? 0,
    })
  }

  const saveEdit = () => {
    if (!editId) return
    api.patch(`/api/v1/models/${editId}`, editForm).then(() => {
      setEditId(null)
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  if (loading) return <p className="text-slate-500">Loading models...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Model Catalog (Admin)</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          + Add Model
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-xl ring-1 ring-red-200 text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {showCreate && (
        <div className="mb-6 p-4 bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <input placeholder="name (e.g. claude-sonnet-4)" value={form.name} onChange={e => setForm({...form, name: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input placeholder="Display Name" value={form.display_name} onChange={e => setForm({...form, display_name: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <select value={form.provider} onChange={e => setForm({...form, provider: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
              <option value="anthropic">Anthropic</option>
              <option value="openai">OpenAI</option>
              <option value="azure_openai">Azure OpenAI</option>
              <option value="mistral">Mistral</option>
              <option value="self_hosted">Self-hosted</option>
            </select>
          </div>
          <div className="grid grid-cols-4 gap-3">
            <input type="number" placeholder="Context window" value={form.context_window} onChange={e => setForm({...form, context_window: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" placeholder="Max output" value={form.max_output_tokens} onChange={e => setForm({...form, max_output_tokens: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" step="0.001" placeholder="$/1k input" value={form.cost_per_1k_input} onChange={e => setForm({...form, cost_per_1k_input: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" step="0.001" placeholder="$/1k output" value={form.cost_per_1k_output} onChange={e => setForm({...form, cost_per_1k_output: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          </div>
          <div className="flex gap-4 text-sm">
            {(['cap_chat','cap_thinking','cap_vision','cap_tool_use','cap_json_output','cap_embedding'] as const).map(cap => (
              <label key={cap} className="flex items-center gap-1">
                <input type="checkbox" checked={(form as any)[cap]} onChange={e => setForm({...form, [cap]: e.target.checked})} />
                {cap.replace('cap_','')}
              </label>
            ))}
          </div>
          <div className="flex gap-2">
            <button onClick={create} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">Create Model</button>
            <button onClick={() => setShowCreate(false)} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">Cancel</button>
          </div>
        </div>
      )}

      {/* Inline edit panel */}
      {editId && (
        <div className="mb-6 p-4 bg-blue-50 rounded-2xl ring-1 ring-blue-200 shadow-sm space-y-3">
          <h3 className="text-sm font-semibold text-blue-800">Editing model</h3>
          <div className="grid grid-cols-5 gap-3">
            <input placeholder="Display Name" value={editForm.display_name} onChange={e => setEditForm({...editForm, display_name: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" placeholder="Context window" value={editForm.context_window} onChange={e => setEditForm({...editForm, context_window: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" placeholder="Max output" value={editForm.max_output_tokens} onChange={e => setEditForm({...editForm, max_output_tokens: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" step="0.001" placeholder="$/1k input" value={editForm.cost_per_1k_input} onChange={e => setEditForm({...editForm, cost_per_1k_input: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" step="0.001" placeholder="$/1k output" value={editForm.cost_per_1k_output} onChange={e => setEditForm({...editForm, cost_per_1k_output: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          </div>
          <div className="flex gap-2">
            <button onClick={saveEdit} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">Save</button>
            <button onClick={() => setEditId(null)} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">Cancel</button>
          </div>
        </div>
      )}

      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
        <table className="w-full">
          <thead className="bg-slate-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Name</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Provider</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Capabilities</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Context</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Pricing (in/out)</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Status</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {models.map(m => (
              <tr key={m.id} className={`hover:bg-slate-50 ${!m.is_active ? 'opacity-50' : ''}`}>
                <td className="px-4 py-3"><div className="font-medium text-sm">{m.display_name}</div><div className="text-xs text-slate-500">{m.name}</div></td>
                <td className="px-4 py-3 text-sm">{m.provider}</td>
                <td className="px-4 py-3">
                  <div className="flex flex-wrap gap-1">
                    {m.cap_chat && <span className="rounded-full px-2 py-0.5 bg-blue-50 text-blue-700 text-xs font-medium ring-1 ring-blue-600/20">Chat</span>}
                    {m.cap_thinking && <span className="rounded-full px-2 py-0.5 bg-purple-50 text-purple-700 text-xs font-medium ring-1 ring-purple-600/20">Think</span>}
                    {m.cap_vision && <span className="rounded-full px-2 py-0.5 bg-green-50 text-green-700 text-xs font-medium ring-1 ring-green-600/20">Vision</span>}
                    {m.cap_tool_use && <span className="rounded-full px-2 py-0.5 bg-orange-50 text-orange-700 text-xs font-medium ring-1 ring-orange-600/20">Tools</span>}
                    {m.cap_json_output && <span className="rounded-full px-2 py-0.5 bg-cyan-50 text-cyan-700 text-xs font-medium ring-1 ring-cyan-600/20">JSON</span>}
                    {m.cap_embedding && <span className="rounded-full px-2 py-0.5 bg-pink-50 text-pink-700 text-xs font-medium ring-1 ring-pink-600/20">Embed</span>}
                  </div>
                </td>
                <td className="px-4 py-3 text-sm text-slate-600">{m.context_window?.toLocaleString() ?? '-'}</td>
                <td className="px-4 py-3 text-xs text-slate-600">{m.cost_per_1k_input != null ? `$${m.cost_per_1k_input} / $${m.cost_per_1k_output}` : '-'}</td>
                <td className="px-4 py-3">
                  <div className="flex gap-1">
                    <button onClick={() => togglePublish(m)} className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${m.published ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-slate-50 text-slate-500 ring-1 ring-slate-600/20'}`}>
                      {m.published ? 'Published' : 'Draft'}
                    </button>
                  </div>
                </td>
                <td className="px-4 py-3">
                  <div className="flex gap-2">
                    <button onClick={() => startEdit(m)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
                    <button onClick={() => toggleActive(m)} className={`text-sm font-medium transition-colors ${m.is_active ? 'text-amber-600 hover:text-amber-500' : 'text-green-600 hover:text-green-500'}`}>
                      {m.is_active ? 'Deactivate' : 'Activate'}
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
