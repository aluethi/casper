import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { Deployment } from '../types'

export default function DeploymentListPage() {
  const [deployments, setDeployments] = useState<Deployment[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', slug: '', model_id: '', default_params: '{}' })
  const [saving, setSaving] = useState(false)

  const load = () => {
    setLoading(true)
    api.get('/api/v1/deployments')
      .then((r) => setDeployments(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const create = async () => {
    setSaving(true)
    setError('')
    try {
      let params = {}
      try { params = JSON.parse(form.default_params) } catch { /* ignore */ }
      await api.post('/api/v1/deployments', { name: form.name, slug: form.slug, model_id: form.model_id, default_params: params })
      setShowForm(false)
      setForm({ name: '', slug: '', model_id: '', default_params: '{}' })
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this deployment?')) return
    try {
      await api.delete(`/api/v1/deployments/${id}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-2xl font-bold text-gray-900">Deployments</h1>
        <button onClick={() => setShowForm(!showForm)} className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700">
          {showForm ? 'Cancel' : 'Create'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      {showForm && (
        <div className="bg-white rounded-lg border border-gray-200 p-4 mb-4 space-y-3">
          <input placeholder="Name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <input placeholder="Slug" value={form.slug} onChange={(e) => setForm({ ...form, slug: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <input placeholder="Model ID" value={form.model_id} onChange={(e) => setForm({ ...form, model_id: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <textarea placeholder='Default Params (JSON)' value={form.default_params}
            onChange={(e) => setForm({ ...form, default_params: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm font-mono" rows={3} />
          <button onClick={create} disabled={saving} className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700 disabled:opacity-50">
            {saving ? 'Creating...' : 'Create Deployment'}
          </button>
        </div>
      )}

      {deployments.length === 0 ? (
        <p className="text-gray-500">No deployments yet.</p>
      ) : (
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-left text-gray-600">
              <tr>
                <th className="px-4 py-3">Name</th><th className="px-4 py-3">Slug</th><th className="px-4 py-3">Model</th>
                <th className="px-4 py-3">Status</th><th className="px-4 py-3">Created</th><th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {deployments.map((d) => (
                <tr key={d.id} className="hover:bg-gray-50">
                  <td className="px-4 py-3 font-medium text-gray-900">{d.name}</td>
                  <td className="px-4 py-3 text-gray-500 font-mono">{d.slug}</td>
                  <td className="px-4 py-3 text-gray-500">{d.model_id}</td>
                  <td className="px-4 py-3">
                    <span className={`text-xs px-2 py-0.5 rounded-full ${d.is_active ? 'bg-green-100 text-green-700' : 'bg-gray-100 text-gray-600'}`}>
                      {d.is_active ? 'Active' : 'Inactive'}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-gray-500">{new Date(d.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-3 text-right">
                    <button onClick={() => remove(d.id)} className="text-red-600 hover:text-red-800 text-xs">Delete</button>
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
