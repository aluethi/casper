import { useEffect, useState } from 'react'
import api from '../../lib/api'

interface Backend {
  id: string; name: string; provider: string; provider_label: string | null;
  base_url: string | null; region: string | null; priority: number;
  is_active: boolean; created_at: string;
}

interface AgentKey {
  id: string; name: string; key_prefix: string; is_active: boolean; created_at: string;
}

export default function BackendsPage() {
  const [backends, setBackends] = useState<Backend[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ name: '', provider: 'anthropic', base_url: '', api_key_enc: '', region: '', max_concurrent: 8 })
  const [editId, setEditId] = useState<string | null>(null)
  const [editProvider, setEditProvider] = useState('')
  const [editForm, setEditForm] = useState({ base_url: '', api_key_enc: '', region: '', priority: 100, max_concurrent: 8 })

  // Agent key management
  const [keysForBackend, setKeysForBackend] = useState<string | null>(null)
  const [agentKeys, setAgentKeys] = useState<AgentKey[]>([])
  const [newKeyName, setNewKeyName] = useState('')
  const [createdKey, setCreatedKey] = useState<string | null>(null)
  const [keysLoading, setKeysLoading] = useState(false)

  const load = () => {
    api.get('/api/v1/backends').then(r => {
      setBackends(r.data.data || r.data)
      setLoading(false)
    }).catch(e => { setError(e.message); setLoading(false) })
  }
  useEffect(load, [])

  const create = () => {
    const body: Record<string, unknown> = { name: form.name, provider: form.provider }
    if (form.provider !== 'agent') {
      if (form.base_url) body.base_url = form.base_url
      if (form.api_key_enc) body.api_key_enc = form.api_key_enc
    }
    if (form.region) body.region = form.region
    if (form.provider === 'agent') {
      body.extra_config = { max_concurrent: form.max_concurrent }
    }
    api.post('/api/v1/backends', body).then(() => {
      setShowCreate(false)
      setForm({ name: '', provider: 'anthropic', base_url: '', api_key_enc: '', region: '', max_concurrent: 8 })
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const startEdit = (b: Backend) => {
    setEditId(b.id)
    setEditProvider(b.provider)
    setEditForm({ base_url: b.base_url || '', api_key_enc: '', region: b.region || '', priority: b.priority, max_concurrent: 8 })
  }

  const saveEdit = () => {
    if (!editId) return
    const body: Record<string, unknown> = { priority: editForm.priority }
    if (editProvider !== 'agent') {
      if (editForm.base_url) body.base_url = editForm.base_url
      if (editForm.api_key_enc) body.api_key_enc = editForm.api_key_enc
    } else {
      body.extra_config = { max_concurrent: editForm.max_concurrent }
    }
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

  // Agent key management
  const loadKeys = (backendId: string) => {
    if (keysForBackend === backendId) { setKeysForBackend(null); return }
    setKeysForBackend(backendId)
    setKeysLoading(true)
    setCreatedKey(null)
    api.get(`/api/v1/backends/${backendId}/keys`).then(r => {
      setAgentKeys(r.data.data || r.data || [])
    }).catch(() => setAgentKeys([]))
    .finally(() => setKeysLoading(false))
  }

  const createKey = (backendId: string) => {
    if (!newKeyName.trim()) return
    api.post(`/api/v1/backends/${backendId}/keys`, { name: newKeyName }).then(r => {
      setCreatedKey(r.data.key)
      setNewKeyName('')
      // Reload keys
      api.get(`/api/v1/backends/${backendId}/keys`).then(r2 => {
        setAgentKeys(r2.data.data || r2.data || [])
      })
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const revokeKey = (backendId: string, keyId: string) => {
    if (!confirm('Revoke this agent key? Connected sidecars using it will be disconnected.')) return
    api.delete(`/api/v1/backends/${backendId}/keys/${keyId}`).then(() => {
      setAgentKeys(agentKeys.filter(k => k.id !== keyId))
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  if (loading) return <p className="text-slate-500">Loading backends...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Platform Backends</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          + Add Backend
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-xl ring-1 ring-red-200 text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {showCreate && (
        <div className="mb-6 p-4 bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <input placeholder="Backend name (e.g. anthropic-eu)" value={form.name} onChange={e => setForm({...form, name: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <select value={form.provider} onChange={e => setForm({...form, provider: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
              <option value="anthropic">Anthropic</option>
              <option value="openai_compatible">OpenAI-compatible</option>
              <option value="azure_openai">Azure OpenAI</option>
              <option value="agent">Agent (self-hosted GPU)</option>
            </select>
          </div>
          {form.provider !== 'agent' && (
            <div className="space-y-3">
              {form.provider === 'azure_openai' && (
                <p className="text-xs text-slate-500">
                  Azure OpenAI requires the full deployment endpoint URL including api-version, e.g.:<br/>
                  <code className="bg-slate-100 px-1 rounded text-[11px]">https://myresource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21</code>
                </p>
              )}
              <div className="grid grid-cols-3 gap-3">
                <input placeholder={form.provider === 'azure_openai' ? 'Full endpoint URL (see above)' : 'Base URL (e.g. https://api.anthropic.com)'} value={form.base_url} onChange={e => setForm({...form, base_url: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
                <input type="password" placeholder={form.provider === 'azure_openai' ? 'Azure API Key' : 'API Key'} value={form.api_key_enc} onChange={e => setForm({...form, api_key_enc: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
                <input placeholder="Region (optional)" value={form.region} onChange={e => setForm({...form, region: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              </div>
            </div>
          )}
          {form.provider === 'agent' && (
            <div className="space-y-3">
              <p className="text-xs text-slate-500">Agent backends connect via WebSocket. Inference URLs are configured on the sidecar, not here. Create the backend, then generate agent keys for the sidecar.</p>
              <div className="grid grid-cols-2 gap-3">
                <input type="number" placeholder="Max concurrent requests" value={form.max_concurrent} onChange={e => setForm({...form, max_concurrent: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
                <input placeholder="Region (optional)" value={form.region} onChange={e => setForm({...form, region: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              </div>
            </div>
          )}
          <div className="flex gap-2">
            <button onClick={create} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">Create Backend</button>
            <button onClick={() => setShowCreate(false)} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">Cancel</button>
          </div>
        </div>
      )}

      {editId && (
        <div className="mb-6 p-4 bg-blue-50 rounded-2xl ring-1 ring-blue-200 shadow-sm space-y-3">
          <h3 className="text-sm font-semibold text-blue-800">Editing backend {editProvider === 'agent' ? '(agent)' : ''}</h3>
          {editProvider !== 'agent' ? (
            <div className="grid grid-cols-4 gap-3">
              <input placeholder="Base URL" value={editForm.base_url} onChange={e => setEditForm({...editForm, base_url: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              <input type="password" placeholder="New API Key (leave empty to keep)" value={editForm.api_key_enc} onChange={e => setEditForm({...editForm, api_key_enc: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              <input placeholder="Region" value={editForm.region} onChange={e => setEditForm({...editForm, region: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              <input type="number" placeholder="Priority" value={editForm.priority} onChange={e => setEditForm({...editForm, priority: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            </div>
          ) : (
            <div className="grid grid-cols-3 gap-3">
              <input type="number" placeholder="Max concurrent" value={editForm.max_concurrent} onChange={e => setEditForm({...editForm, max_concurrent: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              <input placeholder="Region" value={editForm.region} onChange={e => setEditForm({...editForm, region: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
              <input type="number" placeholder="Priority" value={editForm.priority} onChange={e => setEditForm({...editForm, priority: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            </div>
          )}
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
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Base URL</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Region</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Priority</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Status</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {backends.length === 0 && (
              <tr><td colSpan={7} className="px-4 py-8 text-center text-slate-500">No backends configured yet.</td></tr>
            )}
            {backends.map(b => (
              <>
                <tr key={b.id} className={`hover:bg-slate-50 ${!b.is_active ? 'opacity-50' : ''}`}>
                  <td className="px-4 py-3">
                    <div className="font-medium text-sm">{b.name}</div>
                    <div className="text-xs text-slate-400">{b.id.slice(0,8)}...</div>
                  </td>
                  <td className="px-4 py-3">
                    <span className={`text-sm ${b.provider === 'agent' ? 'font-medium text-purple-700' : ''}`}>{b.provider_label || b.provider}</span>
                    {b.provider === 'agent' && <span className="ml-1.5 rounded-full px-1.5 py-0.5 bg-purple-50 text-purple-600 text-[10px] font-medium ring-1 ring-purple-600/20">GPU</span>}
                  </td>
                  <td className="px-4 py-3 text-sm text-slate-600 font-mono">{b.provider === 'agent' ? <span className="text-slate-400 italic">WebSocket</span> : (b.base_url || '-')}</td>
                  <td className="px-4 py-3 text-sm text-slate-600">{b.region || '-'}</td>
                  <td className="px-4 py-3 text-sm">{b.priority}</td>
                  <td className="px-4 py-3">
                    <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${b.is_active ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-red-50 text-red-700 ring-1 ring-red-600/20'}`}>
                      {b.is_active ? 'Active' : 'Inactive'}
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex gap-2">
                      <button onClick={() => startEdit(b)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
                      {b.provider === 'agent' && (
                        <button onClick={() => loadKeys(b.id)} className={`text-sm font-medium transition-colors ${keysForBackend === b.id ? 'text-purple-600' : 'text-purple-500 hover:text-purple-400'}`}>
                          Keys
                        </button>
                      )}
                      <button onClick={() => toggleActive(b)} className={`text-sm font-medium transition-colors ${b.is_active ? 'text-amber-600 hover:text-amber-500' : 'text-green-600 hover:text-green-500'}`}>
                        {b.is_active ? 'Deactivate' : 'Activate'}
                      </button>
                    </div>
                  </td>
                </tr>
                {/* Agent Keys panel */}
                {keysForBackend === b.id && (
                  <tr key={`${b.id}-keys`}>
                    <td colSpan={7} className="px-4 py-4 bg-purple-50/30">
                      <div className="rounded-xl ring-1 ring-purple-200 bg-white p-4">
                        <div className="flex items-center justify-between mb-3">
                          <h4 className="text-sm font-semibold text-purple-900">Agent Keys for {b.name}</h4>
                          <div className="flex gap-2 items-center">
                            <input placeholder="Key name (e.g. gpu-server-01)" value={newKeyName} onChange={e => setNewKeyName(e.target.value)}
                              onKeyDown={e => { if (e.key === 'Enter') createKey(b.id) }}
                              className="rounded-lg ring-1 ring-slate-300 px-3 py-1.5 text-sm w-56 focus:ring-2 focus:ring-purple-500 focus:outline-none" />
                            <button onClick={() => createKey(b.id)} className="bg-purple-600 text-white px-3 py-1.5 rounded-full text-xs font-semibold hover:bg-purple-500 transition-colors">
                              + Create Key
                            </button>
                          </div>
                        </div>

                        {createdKey && (
                          <div className="mb-3 p-3 bg-amber-50 rounded-lg ring-1 ring-amber-200">
                            <p className="text-xs font-semibold text-amber-800 mb-1">New key created — copy it now, it won't be shown again:</p>
                            <code className="text-sm font-mono text-amber-900 select-all break-all">{createdKey}</code>
                          </div>
                        )}

                        {keysLoading ? (
                          <p className="text-sm text-slate-400">Loading keys...</p>
                        ) : agentKeys.length === 0 ? (
                          <p className="text-sm text-slate-400">No keys yet. Create one to connect a GPU server sidecar.</p>
                        ) : (
                          <table className="w-full text-sm">
                            <thead>
                              <tr className="text-xs text-slate-500 uppercase tracking-wide">
                                <th className="text-left py-1.5">Name</th>
                                <th className="text-left py-1.5">Prefix</th>
                                <th className="text-left py-1.5">Status</th>
                                <th className="text-left py-1.5">Created</th>
                                <th className="text-left py-1.5"></th>
                              </tr>
                            </thead>
                            <tbody className="divide-y divide-slate-100">
                              {agentKeys.map(k => (
                                <tr key={k.id} className={!k.is_active ? 'opacity-50' : ''}>
                                  <td className="py-1.5 font-medium text-slate-900">{k.name}</td>
                                  <td className="py-1.5 font-mono text-slate-500">{k.key_prefix}...</td>
                                  <td className="py-1.5">
                                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${k.is_active ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-red-50 text-red-600 ring-1 ring-red-600/20'}`}>
                                      {k.is_active ? 'Active' : 'Revoked'}
                                    </span>
                                  </td>
                                  <td className="py-1.5 text-slate-500">{new Date(k.created_at).toLocaleDateString()}</td>
                                  <td className="py-1.5 text-right">
                                    {k.is_active && (
                                      <button onClick={() => revokeKey(b.id, k.id)} className="text-red-600 hover:text-red-500 text-xs font-medium transition-colors">Revoke</button>
                                    )}
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        )}

                        <div className="mt-3 pt-3 border-t border-slate-100">
                          <p className="text-xs text-slate-400">Use this key in the sidecar config: <code className="bg-slate-100 px-1 rounded">agent_key: csa-...</code></p>
                        </div>
                      </div>
                    </td>
                  </tr>
                )}
              </>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
