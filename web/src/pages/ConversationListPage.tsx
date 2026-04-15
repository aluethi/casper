import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { Conversation } from '../types'

interface Message { role: string; content: string; created_at: string }

const statusColors: Record<string, string> = {
  active: 'bg-green-100 text-green-700',
  completed: 'bg-blue-100 text-blue-700',
  failed: 'bg-red-100 text-red-700',
}

export default function ConversationListPage() {
  const [conversations, setConversations] = useState<(Conversation & { outcome?: string })[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [agentFilter, setAgentFilter] = useState('')
  const [statusFilter, setStatusFilter] = useState('')
  const [selected, setSelected] = useState<string | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [loadingMsgs, setLoadingMsgs] = useState(false)

  useEffect(() => {
    api.get('/api/v1/conversations')
      .then((r) => setConversations(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }, [])

  const openConversation = async (id: string) => {
    if (selected === id) { setSelected(null); return }
    setSelected(id)
    setLoadingMsgs(true)
    try {
      const res = await api.get(`/api/v1/conversations/${id}/messages`)
      setMessages(res.data.data ?? res.data)
    } catch {
      setMessages([])
    } finally {
      setLoadingMsgs(false)
    }
  }

  const setOutcome = async (id: string, outcome: string) => {
    try {
      await api.patch(`/api/v1/conversations/${id}`, { outcome })
      setConversations((c) => c.map((cv) => cv.id === id ? { ...cv, outcome } : cv))
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const agents = [...new Set(conversations.map((c) => c.agent_id))].sort()
  const statuses = [...new Set(conversations.map((c) => c.status))].sort()
  const filtered = conversations.filter((c) =>
    (!agentFilter || c.agent_id === agentFilter) && (!statusFilter || c.status === statusFilter)
  )

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 mb-4">Conversations</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      <div className="flex gap-3 mb-4">
        <select value={agentFilter} onChange={(e) => setAgentFilter(e.target.value)}
          className="border border-gray-300 rounded px-3 py-2 text-sm">
          <option value="">All Agents</option>
          {agents.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
        <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)}
          className="border border-gray-300 rounded px-3 py-2 text-sm">
          <option value="">All Statuses</option>
          {statuses.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
      </div>

      {filtered.length === 0 ? (
        <p className="text-gray-500">No conversations yet.</p>
      ) : (
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-left text-gray-600">
              <tr>
                <th className="px-4 py-3">Agent</th><th className="px-4 py-3">Title</th>
                <th className="px-4 py-3">Status</th><th className="px-4 py-3">Outcome</th>
                <th className="px-4 py-3">Date</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {filtered.map((c) => (
                <tr key={c.id} className={`hover:bg-gray-50 cursor-pointer ${selected === c.id ? 'bg-blue-50' : ''}`}>
                  <td className="px-4 py-3 text-gray-500" onClick={() => openConversation(c.id)}>{c.agent_id}</td>
                  <td className="px-4 py-3 font-medium text-gray-900" onClick={() => openConversation(c.id)}>
                    {c.title || 'Untitled'}
                  </td>
                  <td className="px-4 py-3" onClick={() => openConversation(c.id)}>
                    <span className={`text-xs px-2 py-0.5 rounded-full ${statusColors[c.status] ?? 'bg-gray-100 text-gray-600'}`}>
                      {c.status}
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <select value={c.outcome ?? ''} onChange={(e) => setOutcome(c.id, e.target.value)}
                      className="border border-gray-200 rounded px-2 py-1 text-xs">
                      <option value="">--</option>
                      <option value="success">Success</option>
                      <option value="failure">Failure</option>
                      <option value="partial">Partial</option>
                    </select>
                  </td>
                  <td className="px-4 py-3 text-gray-500" onClick={() => openConversation(c.id)}>
                    {new Date(c.created_at).toLocaleDateString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {selected && (
            <div className="border-t border-gray-200 p-4 bg-gray-50">
              <h3 className="font-medium text-gray-700 mb-2">Messages</h3>
              {loadingMsgs ? (
                <p className="text-gray-400 text-sm">Loading messages...</p>
              ) : messages.length === 0 ? (
                <p className="text-gray-400 text-sm">No messages.</p>
              ) : (
                <div className="space-y-2 max-h-60 overflow-y-auto">
                  {messages.map((m, i) => (
                    <div key={i} className={`text-sm rounded p-2 ${m.role === 'user' ? 'bg-blue-100 text-blue-900' : 'bg-white text-gray-800'}`}>
                      <span className="font-medium text-xs text-gray-500">{m.role}</span>
                      <p className="whitespace-pre-wrap">{m.content}</p>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
