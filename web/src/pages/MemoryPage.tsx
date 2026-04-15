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

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">Memory</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Agent Memory */}
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4">
          <div className="flex items-center justify-between mb-3">
            <h2 className="font-semibold text-slate-900">Agent Memory</h2>
            {selectedAgent && agentMemory && !editingAgent && (
              <button onClick={() => setEditingAgent(true)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
            )}
          </div>
          <select value={selectedAgent} onChange={(e) => { setSelectedAgent(e.target.value); setEditingAgent(false) }}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow mb-3">
            <option value="">Select an agent...</option>
            {agents.map((a) => <option key={a.id} value={a.name}>{a.display_name || a.name}</option>)}
          </select>
          {selectedAgent && agentMemory && (
            editingAgent ? (
              <div className="space-y-2">
                <textarea value={agentDraft} onChange={(e) => setAgentDraft(e.target.value)}
                  className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow font-mono" rows={10} />
                <div className="flex gap-2">
                  <button onClick={saveAgentMemory} disabled={saving}
                    className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
                    {saving ? 'Saving...' : 'Save'}
                  </button>
                  <button onClick={() => { setEditingAgent(false); setAgentDraft(agentMemory.content) }}
                    className="text-slate-600 text-sm font-medium hover:text-slate-800 transition-colors">Cancel</button>
                </div>
              </div>
            ) : (
              <div>
                <div className="bg-slate-50 rounded-lg p-3 text-sm whitespace-pre-wrap min-h-[100px]">
                  {agentMemory.content || <span className="text-slate-400">No memory content.</span>}
                </div>
                {agentMemory.versions && agentMemory.versions.length > 0 && (
                  <div className="mt-3">
                    <h4 className="text-xs font-medium text-slate-500 mb-1">Version History</h4>
                    <div className="space-y-1 max-h-40 overflow-y-auto">
                      {agentMemory.versions.map((v) => (
                        <div key={v.version} className="text-xs text-slate-500 bg-slate-50 p-2 rounded-lg">
                          v{v.version} -- {new Date(v.updated_at).toLocaleString()}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )
          )}
          {selectedAgent && !agentMemory && <p className="text-slate-400 text-sm">Loading memory...</p>}
        </div>

        {/* Tenant Memory */}
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4">
          <div className="flex items-center justify-between mb-3">
            <h2 className="font-semibold text-slate-900">Tenant Memory</h2>
            {tenantMemory && !editingTenant && (
              <button onClick={() => setEditingTenant(true)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
            )}
          </div>
          {tenantMemory && (
            editingTenant ? (
              <div className="space-y-2">
                <textarea value={tenantDraft} onChange={(e) => setTenantDraft(e.target.value)}
                  className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow font-mono" rows={10} />
                <div className="flex gap-2">
                  <button onClick={saveTenantMemory} disabled={saving}
                    className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
                    {saving ? 'Saving...' : 'Save'}
                  </button>
                  <button onClick={() => { setEditingTenant(false); setTenantDraft(tenantMemory.content) }}
                    className="text-slate-600 text-sm font-medium hover:text-slate-800 transition-colors">Cancel</button>
                </div>
              </div>
            ) : (
              <div>
                <div className="bg-slate-50 rounded-lg p-3 text-sm whitespace-pre-wrap min-h-[100px]">
                  {tenantMemory.content || <span className="text-slate-400">No memory content.</span>}
                </div>
                {tenantMemory.versions && tenantMemory.versions.length > 0 && (
                  <div className="mt-3">
                    <h4 className="text-xs font-medium text-slate-500 mb-1">Version History</h4>
                    <div className="space-y-1 max-h-40 overflow-y-auto">
                      {tenantMemory.versions.map((v) => (
                        <div key={v.version} className="text-xs text-slate-500 bg-slate-50 p-2 rounded-lg">
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
