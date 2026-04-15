import { useState, useEffect } from 'react'
import { Link } from 'react-router-dom'
import api from '../lib/api'
import type { Agent } from '../types'

export default function AgentListPage() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', display_name: '', description: '', model_deployment: '' })
  const [saving, setSaving] = useState(false)

  const load = () => {
    setLoading(true)
    api.get('/agents')
      .then((r) => setAgents(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const create = async () => {
    setSaving(true)
    setError('')
    try {
      await api.post('/agents', form)
      setShowForm(false)
      setForm({ name: '', display_name: '', description: '', model_deployment: '' })
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-2xl font-bold text-gray-900">Agents</h1>
        <button onClick={() => setShowForm(!showForm)}
          className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700">
          {showForm ? 'Cancel' : 'Create Agent'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      {showForm && (
        <div className="bg-white rounded-lg border border-gray-200 p-4 mb-4 space-y-3">
          <input placeholder="Name (slug)" value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <input placeholder="Display Name" value={form.display_name}
            onChange={(e) => setForm({ ...form, display_name: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <textarea placeholder="Description" value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" rows={2} />
          <input placeholder="Model Deployment" value={form.model_deployment}
            onChange={(e) => setForm({ ...form, model_deployment: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <button onClick={create} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700 disabled:opacity-50">
            {saving ? 'Creating...' : 'Create'}
          </button>
        </div>
      )}

      {agents.length === 0 ? (
        <p className="text-gray-500">No agents yet.</p>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {agents.map((a) => (
            <Link key={a.id} to={`/agents/${a.name}`}
              className="bg-white rounded-lg border border-gray-200 p-4 hover:border-blue-300 transition-colors">
              <div className="flex items-start justify-between mb-2">
                <h3 className="font-semibold text-gray-900">{a.display_name || a.name}</h3>
                <span className={`text-xs px-2 py-0.5 rounded-full ${a.status === 'active' ? 'bg-green-100 text-green-700' : 'bg-gray-100 text-gray-600'}`}>
                  {a.status}
                </span>
              </div>
              {a.description && <p className="text-sm text-gray-500 mb-2">{a.description}</p>}
              <div className="text-xs text-gray-400 space-y-0.5">
                <p>Model: {a.model_deployment}</p>
                <p>Version: {a.version}</p>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
