import { useState, useEffect } from 'react'
import api from '../../lib/api'

interface Secret {
  id: string
  key: string
  created_at: string
  updated_at: string
}

export default function SecretsPage() {
  const [secrets, setSecrets] = useState<Secret[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ key: '', value: '' })
  const [saving, setSaving] = useState(false)

  const load = () => {
    setLoading(true)
    api.get('/api/v1/secrets')
      .then((r) => setSecrets(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const save = async () => {
    setSaving(true)
    setError('')
    try {
      await api.put(`/api/v1/secrets/${form.key}`, { value: form.value })
      setShowForm(false)
      setForm({ key: '', value: '' })
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const remove = async (key: string) => {
    if (!confirm(`Delete secret "${key}"?`)) return
    try {
      await api.delete(`/api/v1/secrets/${key}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-2xl font-bold text-gray-900">Secrets</h1>
        <button onClick={() => setShowForm(!showForm)}
          className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700">
          {showForm ? 'Cancel' : 'Set Secret'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      {showForm && (
        <div className="bg-white rounded-lg border border-gray-200 p-4 mb-4 space-y-3">
          <input placeholder="Key name" value={form.key} onChange={(e) => setForm({ ...form, key: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <input placeholder="Value" type="password" value={form.value}
            onChange={(e) => setForm({ ...form, value: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <button onClick={save} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700 disabled:opacity-50">
            {saving ? 'Saving...' : 'Set Secret'}
          </button>
        </div>
      )}

      {secrets.length === 0 ? (
        <p className="text-gray-500">No secrets yet.</p>
      ) : (
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-left text-gray-600">
              <tr>
                <th className="px-4 py-3">Key</th><th className="px-4 py-3">Created</th>
                <th className="px-4 py-3">Updated</th><th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {secrets.map((s) => (
                <tr key={s.id || s.key} className="hover:bg-gray-50">
                  <td className="px-4 py-3 font-medium text-gray-900 font-mono">{s.key}</td>
                  <td className="px-4 py-3 text-gray-500">{new Date(s.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-3 text-gray-500">{new Date(s.updated_at).toLocaleDateString()}</td>
                  <td className="px-4 py-3 text-right space-x-2">
                    <button onClick={() => { setForm({ key: s.key, value: '' }); setShowForm(true) }}
                      className="text-blue-600 text-xs hover:text-blue-800">Update</button>
                    <button onClick={() => remove(s.key)} className="text-red-600 text-xs hover:text-red-800">Delete</button>
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
