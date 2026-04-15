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

  if (loading) return <p className="text-gray-500">Loading models...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-bold text-gray-900">Model Catalog (Admin)</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="px-4 py-2 bg-blue-600 text-white rounded-lg text-sm hover:bg-blue-700">
          + Add Model
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-lg text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {showCreate && (
        <div className="mb-6 p-4 bg-white border border-gray-200 rounded-lg space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <input placeholder="name (e.g. claude-sonnet-4)" value={form.name} onChange={e => setForm({...form, name: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input placeholder="Display Name" value={form.display_name} onChange={e => setForm({...form, display_name: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <select value={form.provider} onChange={e => setForm({...form, provider: e.target.value})} className="border rounded px-3 py-2 text-sm">
              <option value="anthropic">Anthropic</option>
              <option value="openai">OpenAI</option>
              <option value="azure_openai">Azure OpenAI</option>
              <option value="mistral">Mistral</option>
              <option value="self_hosted">Self-hosted</option>
            </select>
          </div>
          <div className="grid grid-cols-4 gap-3">
            <input type="number" placeholder="Context window" value={form.context_window} onChange={e => setForm({...form, context_window: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" placeholder="Max output" value={form.max_output_tokens} onChange={e => setForm({...form, max_output_tokens: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" step="0.001" placeholder="$/1k input" value={form.cost_per_1k_input} onChange={e => setForm({...form, cost_per_1k_input: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" step="0.001" placeholder="$/1k output" value={form.cost_per_1k_output} onChange={e => setForm({...form, cost_per_1k_output: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
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
            <button onClick={create} className="px-4 py-2 bg-green-600 text-white rounded text-sm hover:bg-green-700">Create Model</button>
            <button onClick={() => setShowCreate(false)} className="px-4 py-2 bg-gray-200 text-gray-700 rounded text-sm hover:bg-gray-300">Cancel</button>
          </div>
        </div>
      )}

      {/* Inline edit panel */}
      {editId && (
        <div className="mb-6 p-4 bg-blue-50 border border-blue-200 rounded-lg space-y-3">
          <h3 className="text-sm font-semibold text-blue-800">Editing model</h3>
          <div className="grid grid-cols-5 gap-3">
            <input placeholder="Display Name" value={editForm.display_name} onChange={e => setEditForm({...editForm, display_name: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" placeholder="Context window" value={editForm.context_window} onChange={e => setEditForm({...editForm, context_window: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" placeholder="Max output" value={editForm.max_output_tokens} onChange={e => setEditForm({...editForm, max_output_tokens: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" step="0.001" placeholder="$/1k input" value={editForm.cost_per_1k_input} onChange={e => setEditForm({...editForm, cost_per_1k_input: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" step="0.001" placeholder="$/1k output" value={editForm.cost_per_1k_output} onChange={e => setEditForm({...editForm, cost_per_1k_output: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
          </div>
          <div className="flex gap-2">
            <button onClick={saveEdit} className="px-4 py-2 bg-green-600 text-white rounded text-sm hover:bg-green-700">Save</button>
            <button onClick={() => setEditId(null)} className="px-4 py-2 bg-gray-200 text-gray-700 rounded text-sm hover:bg-gray-300">Cancel</button>
          </div>
        </div>
      )}

      <table className="w-full bg-white border border-gray-200 rounded-lg overflow-hidden">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Name</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Provider</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Capabilities</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Context</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Pricing (in/out)</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Status</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Actions</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {models.map(m => (
            <tr key={m.id} className={`hover:bg-gray-50 ${!m.is_active ? 'opacity-50' : ''}`}>
              <td className="px-4 py-3"><div className="font-medium text-sm">{m.display_name}</div><div className="text-xs text-gray-500">{m.name}</div></td>
              <td className="px-4 py-3 text-sm">{m.provider}</td>
              <td className="px-4 py-3">
                <div className="flex flex-wrap gap-1">
                  {m.cap_chat && <span className="px-1.5 py-0.5 bg-blue-100 text-blue-700 rounded text-xs">Chat</span>}
                  {m.cap_thinking && <span className="px-1.5 py-0.5 bg-purple-100 text-purple-700 rounded text-xs">Think</span>}
                  {m.cap_vision && <span className="px-1.5 py-0.5 bg-green-100 text-green-700 rounded text-xs">Vision</span>}
                  {m.cap_tool_use && <span className="px-1.5 py-0.5 bg-orange-100 text-orange-700 rounded text-xs">Tools</span>}
                  {m.cap_json_output && <span className="px-1.5 py-0.5 bg-cyan-100 text-cyan-700 rounded text-xs">JSON</span>}
                  {m.cap_embedding && <span className="px-1.5 py-0.5 bg-pink-100 text-pink-700 rounded text-xs">Embed</span>}
                </div>
              </td>
              <td className="px-4 py-3 text-sm text-gray-600">{m.context_window?.toLocaleString() ?? '-'}</td>
              <td className="px-4 py-3 text-xs text-gray-600">{m.cost_per_1k_input != null ? `$${m.cost_per_1k_input} / $${m.cost_per_1k_output}` : '-'}</td>
              <td className="px-4 py-3">
                <div className="flex gap-1">
                  <button onClick={() => togglePublish(m)} className={`px-2 py-1 rounded text-xs font-medium ${m.published ? 'bg-green-100 text-green-700' : 'bg-gray-100 text-gray-500'}`}>
                    {m.published ? 'Published' : 'Draft'}
                  </button>
                </div>
              </td>
              <td className="px-4 py-3">
                <div className="flex gap-2">
                  <button onClick={() => startEdit(m)} className="text-blue-600 hover:text-blue-800 text-sm">Edit</button>
                  <button onClick={() => toggleActive(m)} className={`text-sm ${m.is_active ? 'text-yellow-600 hover:text-yellow-800' : 'text-green-600 hover:text-green-800'}`}>
                    {m.is_active ? 'Deactivate' : 'Activate'}
                  </button>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
