import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { ApiKey } from '../types'

export default function ApiKeyListPage() {
  const [keys, setKeys] = useState<ApiKey[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', scopes: '' })
  const [saving, setSaving] = useState(false)
  const [newKey, setNewKey] = useState('')

  const load = () => {
    setLoading(true)
    api.get('/api/v1/api-keys')
      .then((r) => setKeys(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const create = async () => {
    setSaving(true)
    setError('')
    try {
      const scopes = form.scopes.split(',').map((s) => s.trim()).filter(Boolean)
      const res = await api.post('/api/v1/api-keys', { name: form.name, scopes })
      setNewKey(res.data.key ?? res.data.raw_key ?? '')
      setForm({ name: '', scopes: '' })
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const revoke = async (id: string) => {
    if (!confirm('Revoke this API key?')) return
    try {
      await api.delete(`/api/v1/api-keys/${id}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">API Keys</h1>
        <button onClick={() => { setShowForm(!showForm); setNewKey('') }}
          className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          {showForm ? 'Cancel' : 'Create'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      {newKey && (
        <div className="bg-amber-50 rounded-2xl ring-1 ring-amber-200 p-4 mb-4">
          <p className="font-medium text-amber-800 mb-1">Save this key now -- it will not be shown again:</p>
          <code className="text-sm bg-amber-100 px-2 py-1 rounded-lg break-all">{newKey}</code>
        </div>
      )}

      {showForm && !newKey && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4 mb-4 space-y-3">
          <input placeholder="Key name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          <input placeholder="Scopes (comma-separated)" value={form.scopes}
            onChange={(e) => setForm({ ...form, scopes: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          <button onClick={create} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
            {saving ? 'Creating...' : 'Create Key'}
          </button>
        </div>
      )}

      {keys.length === 0 ? (
        <p className="text-slate-500">No API keys yet.</p>
      ) : (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-left text-slate-600">
              <tr>
                <th className="px-4 py-3">Name</th><th className="px-4 py-3">Prefix</th>
                <th className="px-4 py-3">Scopes</th><th className="px-4 py-3">Created</th>
                <th className="px-4 py-3">Last Used</th><th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {keys.map((k) => (
                <tr key={k.id} className="hover:bg-slate-50">
                  <td className="px-4 py-3 font-medium text-slate-900">{k.name}</td>
                  <td className="px-4 py-3 text-slate-500 font-mono">{k.prefix}...</td>
                  <td className="px-4 py-3">
                    <div className="flex flex-wrap gap-1">
                      {k.scopes?.map((s) => (
                        <span key={s} className="rounded-full px-2.5 py-0.5 text-xs font-medium bg-slate-50 text-slate-600 ring-1 ring-slate-600/20">{s}</span>
                      ))}
                    </div>
                  </td>
                  <td className="px-4 py-3 text-slate-500">{new Date(k.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-3 text-slate-500">{k.last_used_at ? new Date(k.last_used_at).toLocaleDateString() : 'Never'}</td>
                  <td className="px-4 py-3 text-right">
                    <button onClick={() => revoke(k.id)} className="text-red-600 hover:text-red-500 text-sm font-medium transition-colors">Revoke</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
