import { useEffect, useState } from 'react'
import api from '../../lib/api'

interface Backend {
  id: string; name: string; provider: string; provider_label: string | null;
  base_url: string | null; region: string | null; priority: number;
  is_active: boolean; created_at: string;
}

export default function BackendsPage() {
  const [backends, setBackends] = useState<Backend[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ name: '', provider: 'anthropic', base_url: '', api_key_enc: '', region: '' })
  const [editId, setEditId] = useState<string | null>(null)
  const [editForm, setEditForm] = useState({ base_url: '', api_key_enc: '', region: '', priority: 100 })

  const load = () => {
    api.get('/api/v1/backends').then(r => {
      setBackends(r.data.data || r.data)
      setLoading(false)
    }).catch(e => { setError(e.message); setLoading(false) })
  }
  useEffect(load, [])

  const create = () => {
    const body: Record<string, unknown> = { name: form.name, provider: form.provider }
    if (form.base_url) body.base_url = form.base_url
    if (form.api_key_enc) body.api_key_enc = form.api_key_enc
    if (form.region) body.region = form.region
    api.post('/api/v1/backends', body).then(() => {
      setShowCreate(false)
      setForm({ name: '', provider: 'anthropic', base_url: '', api_key_enc: '', region: '' })
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const startEdit = (b: Backend) => {
    setEditId(b.id)
    setEditForm({ base_url: b.base_url || '', api_key_enc: '', region: b.region || '', priority: b.priority })
  }

  const saveEdit = () => {
    if (!editId) return
    const body: Record<string, unknown> = { priority: editForm.priority }
    if (editForm.base_url) body.base_url = editForm.base_url
    if (editForm.api_key_enc) body.api_key_enc = editForm.api_key_enc
    if (editForm.region) body.region = editForm.region
    api.patch(`/api/v1/backends/${editId}`, body).then(() => {
      setEditId(null)
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const toggleActive = (b: Backend) => {
    api.patch(`/api/v1/backends/${b.id}`, { is_active: !b.is_active }).then(load)
      .catch(e => setError(e.response?.data?.message || e.message))
  }

  if (loading) return <p className="text-gray-500">Loading backends...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-bold text-gray-900">Platform Backends</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="px-4 py-2 bg-blue-600 text-white rounded-lg text-sm hover:bg-blue-700">
          + Add Backend
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-lg text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {showCreate && (
        <div className="mb-6 p-4 bg-white border border-gray-200 rounded-lg space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <input placeholder="Backend name (e.g. anthropic-eu)" value={form.name} onChange={e => setForm({...form, name: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <select value={form.provider} onChange={e => setForm({...form, provider: e.target.value})} className="border rounded px-3 py-2 text-sm">
              <option value="anthropic">Anthropic</option>
              <option value="openai_compatible">OpenAI-compatible</option>
            </select>
          </div>
          <div className="grid grid-cols-3 gap-3">
            <input placeholder="Base URL (e.g. https://api.anthropic.com)" value={form.base_url} onChange={e => setForm({...form, base_url: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="password" placeholder="API Key" value={form.api_key_enc} onChange={e => setForm({...form, api_key_enc: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input placeholder="Region (optional)" value={form.region} onChange={e => setForm({...form, region: e.target.value})} className="border rounded px-3 py-2 text-sm" />
          </div>
          <div className="flex gap-2">
            <button onClick={create} className="px-4 py-2 bg-green-600 text-white rounded text-sm hover:bg-green-700">Create Backend</button>
            <button onClick={() => setShowCreate(false)} className="px-4 py-2 bg-gray-200 text-gray-700 rounded text-sm hover:bg-gray-300">Cancel</button>
          </div>
        </div>
      )}

      {editId && (
        <div className="mb-6 p-4 bg-blue-50 border border-blue-200 rounded-lg space-y-3">
          <h3 className="text-sm font-semibold text-blue-800">Editing backend</h3>
          <div className="grid grid-cols-4 gap-3">
            <input placeholder="Base URL" value={editForm.base_url} onChange={e => setEditForm({...editForm, base_url: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="password" placeholder="New API Key (leave empty to keep)" value={editForm.api_key_enc} onChange={e => setEditForm({...editForm, api_key_enc: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input placeholder="Region" value={editForm.region} onChange={e => setEditForm({...editForm, region: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input type="number" placeholder="Priority" value={editForm.priority} onChange={e => setEditForm({...editForm, priority: +e.target.value})} className="border rounded px-3 py-2 text-sm" />
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
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Base URL</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Region</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Priority</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Status</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Actions</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {backends.length === 0 && (
            <tr><td colSpan={7} className="px-4 py-8 text-center text-gray-500">No backends configured yet.</td></tr>
          )}
          {backends.map(b => (
            <tr key={b.id} className={`hover:bg-gray-50 ${!b.is_active ? 'opacity-50' : ''}`}>
              <td className="px-4 py-3"><div className="font-medium text-sm">{b.name}</div><div className="text-xs text-gray-400">{b.id.slice(0,8)}...</div></td>
              <td className="px-4 py-3 text-sm">{b.provider_label || b.provider}</td>
              <td className="px-4 py-3 text-sm text-gray-600 font-mono">{b.base_url || '-'}</td>
              <td className="px-4 py-3 text-sm text-gray-600">{b.region || '-'}</td>
              <td className="px-4 py-3 text-sm">{b.priority}</td>
              <td className="px-4 py-3">
                <span className={`px-2 py-1 rounded text-xs font-medium ${b.is_active ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}`}>
                  {b.is_active ? 'Active' : 'Inactive'}
                </span>
              </td>
              <td className="px-4 py-3">
                <div className="flex gap-2">
                  <button onClick={() => startEdit(b)} className="text-blue-600 hover:text-blue-800 text-sm">Edit</button>
                  <button onClick={() => toggleActive(b)} className={`text-sm ${b.is_active ? 'text-yellow-600 hover:text-yellow-800' : 'text-green-600 hover:text-green-800'}`}>
                    {b.is_active ? 'Deactivate' : 'Activate'}
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
