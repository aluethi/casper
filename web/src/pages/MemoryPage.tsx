import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { Agent } from '../types'

interface MemoryData {
  content: string
  versions?: { version: number; content: string; updated_at: string }[]
}

export default function MemoryPage() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [agentMemory, setAgentMemory] = useState<MemoryData | null>(null)
  const [tenantMemory, setTenantMemory] = useState<MemoryData | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [editingAgent, setEditingAgent] = useState(false)
  const [editingTenant, setEditingTenant] = useState(false)
  const [agentDraft, setAgentDraft] = useState('')
  const [tenantDraft, setTenantDraft] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    Promise.all([
      api.get('/api/v1/agents').then((r) => setAgents(r.data.data ?? r.data)),
      api.get('/api/v1/tenant-memory')
        .then((r) => { setTenantMemory(r.data); setTenantDraft(r.data.content ?? '') })
        .catch(() => { setTenantMemory({ content: '' }); setTenantDraft('') }),
    ])
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    if (!selectedAgent) { setAgentMemory(null); return }
    api.get(`/api/v1/agents/${selectedAgent}/memory`)
      .then((r) => { setAgentMemory(r.data); setAgentDraft(r.data.content ?? '') })
      .catch(() => { setAgentMemory({ content: '' }); setAgentDraft('') })
  }, [selectedAgent])

  const saveAgentMemory = async () => {
    setSaving(true)
    setError('')
    try {
      await api.put(`/api/v1/agents/${selectedAgent}/memory`, { content: agentDraft })
      setAgentMemory((m) => m ? { ...m, content: agentDraft } : { content: agentDraft })
      setEditingAgent(false)
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const saveTenantMemory = async () => {
    setSaving(true)
    setError('')
    try {
      await api.put('/api/v1/tenant-memory', { content: tenantDraft })
      setTenantMemory((m) => m ? { ...m, content: tenantDraft } : { content: tenantDraft })
      setEditingTenant(false)
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 mb-4">Memory</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Agent Memory */}
        <div className="bg-white rounded-lg border border-gray-200 p-4">
          <div className="flex items-center justify-between mb-3">
            <h2 className="font-semibold text-gray-900">Agent Memory</h2>
            {selectedAgent && agentMemory && !editingAgent && (
              <button onClick={() => setEditingAgent(true)} className="text-blue-600 text-sm hover:text-blue-800">Edit</button>
            )}
          </div>
          <select value={selectedAgent} onChange={(e) => { setSelectedAgent(e.target.value); setEditingAgent(false) }}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm mb-3">
            <option value="">Select an agent...</option>
            {agents.map((a) => <option key={a.id} value={a.name}>{a.display_name || a.name}</option>)}
          </select>
          {selectedAgent && agentMemory && (
            editingAgent ? (
              <div className="space-y-2">
                <textarea value={agentDraft} onChange={(e) => setAgentDraft(e.target.value)}
                  className="w-full border border-gray-300 rounded px-3 py-2 text-sm font-mono" rows={10} />
                <div className="flex gap-2">
                  <button onClick={saveAgentMemory} disabled={saving}
                    className="bg-blue-600 text-white px-3 py-1.5 rounded text-sm hover:bg-blue-700 disabled:opacity-50">
                    {saving ? 'Saving...' : 'Save'}
                  </button>
                  <button onClick={() => { setEditingAgent(false); setAgentDraft(agentMemory.content) }}
                    className="text-gray-600 text-sm hover:text-gray-800">Cancel</button>
                </div>
              </div>
            ) : (
              <div>
                <div className="bg-gray-50 rounded p-3 text-sm whitespace-pre-wrap min-h-[100px]">
                  {agentMemory.content || <span className="text-gray-400">No memory content.</span>}
                </div>
                {agentMemory.versions && agentMemory.versions.length > 0 && (
                  <div className="mt-3">
                    <h4 className="text-xs font-medium text-gray-500 mb-1">Version History</h4>
                    <div className="space-y-1 max-h-40 overflow-y-auto">
                      {agentMemory.versions.map((v) => (
                        <div key={v.version} className="text-xs text-gray-500 bg-gray-50 p-2 rounded">
                          v{v.version} -- {new Date(v.updated_at).toLocaleString()}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )
          )}
          {selectedAgent && !agentMemory && <p className="text-gray-400 text-sm">Loading memory...</p>}
        </div>

        {/* Tenant Memory */}
        <div className="bg-white rounded-lg border border-gray-200 p-4">
          <div className="flex items-center justify-between mb-3">
            <h2 className="font-semibold text-gray-900">Tenant Memory</h2>
            {tenantMemory && !editingTenant && (
              <button onClick={() => setEditingTenant(true)} className="text-blue-600 text-sm hover:text-blue-800">Edit</button>
            )}
          </div>
          {tenantMemory && (
            editingTenant ? (
              <div className="space-y-2">
                <textarea value={tenantDraft} onChange={(e) => setTenantDraft(e.target.value)}
                  className="w-full border border-gray-300 rounded px-3 py-2 text-sm font-mono" rows={10} />
                <div className="flex gap-2">
                  <button onClick={saveTenantMemory} disabled={saving}
                    className="bg-blue-600 text-white px-3 py-1.5 rounded text-sm hover:bg-blue-700 disabled:opacity-50">
                    {saving ? 'Saving...' : 'Save'}
                  </button>
                  <button onClick={() => { setEditingTenant(false); setTenantDraft(tenantMemory.content) }}
                    className="text-gray-600 text-sm hover:text-gray-800">Cancel</button>
                </div>
              </div>
            ) : (
              <div>
                <div className="bg-gray-50 rounded p-3 text-sm whitespace-pre-wrap min-h-[100px]">
                  {tenantMemory.content || <span className="text-gray-400">No memory content.</span>}
                </div>
                {tenantMemory.versions && tenantMemory.versions.length > 0 && (
                  <div className="mt-3">
                    <h4 className="text-xs font-medium text-gray-500 mb-1">Version History</h4>
                    <div className="space-y-1 max-h-40 overflow-y-auto">
                      {tenantMemory.versions.map((v) => (
                        <div key={v.version} className="text-xs text-gray-500 bg-gray-50 p-2 rounded">
                          v{v.version} -- {new Date(v.updated_at).toLocaleString()}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )
          )}
        </div>
      </div>
    </div>
  )
}
