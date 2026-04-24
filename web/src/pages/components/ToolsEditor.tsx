import { useState, useEffect } from 'react'
import { Link } from 'react-router-dom'
import api from '../../lib/api'
import type { McpConnection } from '../../types'

// ── Built-in tool config types ───────────────────────────────────
const BUILTIN_TOOLS = [
  { name: 'delegate', label: 'Delegate', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 300 }, { key: 'max_depth', label: 'Max depth', type: 'number', default: 3 }] },
  { name: 'ask_user', label: 'Ask User', fields: [] },
  { name: 'knowledge_search', label: 'Knowledge Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 5 }, { key: 'relevance_threshold', label: 'Threshold', type: 'number', default: 0.7 }] },
  { name: 'update_memory', label: 'Update Memory', fields: [{ key: 'max_document_tokens', label: 'Max tokens', type: 'number', default: 4000 }] },
  { name: 'web_search', label: 'Web Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 10 }] },
  { name: 'web_fetch', label: 'Web Fetch', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 30 }, { key: 'max_response_bytes', label: 'Max bytes', type: 'number', default: 1048576 }] },
]

interface ToolsEditorProps {
  builtinTools: Record<string, Record<string, unknown>>
  setBuiltinTools: React.Dispatch<React.SetStateAction<Record<string, Record<string, unknown>>>>
  mcpConnectionNames: string[]
  setMcpConnectionNames: React.Dispatch<React.SetStateAction<string[]>>
}

export default function ToolsEditor({ builtinTools, setBuiltinTools, mcpConnectionNames, setMcpConnectionNames }: ToolsEditorProps) {
  const [availableConnections, setAvailableConnections] = useState<McpConnection[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    api.get('/api/v1/mcp-connections')
      .then(r => {
        setAvailableConnections((r.data || []).filter((c: McpConnection) => c.is_active))
      })
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const toggleConnection = (name: string) => {
    if (mcpConnectionNames.includes(name)) {
      setMcpConnectionNames(mcpConnectionNames.filter(n => n !== name))
    } else {
      setMcpConnectionNames([...mcpConnectionNames, name])
    }
  }

  const authLabel = (c: McpConnection) => {
    switch (c.auth_type) {
      case 'bearer': return 'Bearer token'
      case 'user_oauth': return `OAuth (${c.auth_provider || '?'})`
      case 'mcp_oauth': return 'MCP OAuth 2.1'
      default: return 'None'
    }
  }

  return (
    <div className="space-y-6">
      {/* Built-in Tools */}
      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
        <h2 className="font-display text-lg tracking-tight text-slate-900 mb-4">Built-in Tools</h2>
        <div className="space-y-3">
          {BUILTIN_TOOLS.map(tool => {
            const enabled = tool.name in builtinTools
            const config = builtinTools[tool.name] || {}
            return (
              <div key={tool.name} className={`rounded-xl ring-1 px-4 py-3 transition-all ${enabled ? 'ring-blue-200 bg-blue-50/30' : 'ring-slate-200'}`}>
                <div className="flex items-center justify-between">
                  <label className="flex items-center gap-3 cursor-pointer">
                    <input type="checkbox" checked={enabled} onChange={e => {
                      const next = { ...builtinTools }
                      if (e.target.checked) {
                        const defaults: Record<string, unknown> = {}
                        tool.fields.forEach(f => { defaults[f.key] = f.default })
                        next[tool.name] = defaults
                      } else { delete next[tool.name] }
                      setBuiltinTools(next)
                    }} className="rounded border-slate-300 text-blue-600 focus:ring-blue-500" />
                    <span className="text-sm font-medium text-slate-900">{tool.label}</span>
                  </label>
                </div>
                {enabled && tool.fields.length > 0 && (
                  <div className="mt-3 flex gap-4 pl-8">
                    {tool.fields.map(f => (
                      <div key={f.key}>
                        <label className="block text-xs text-slate-500 mb-0.5">{f.label}</label>
                        <input type="number" value={(config[f.key] as number) ?? f.default}
                          onChange={e => setBuiltinTools({ ...builtinTools, [tool.name]: { ...config, [f.key]: +e.target.value } })}
                          className="w-28 rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </div>

      {/* MCP Connections */}
      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="font-display text-lg tracking-tight text-slate-900">MCP Servers</h2>
            <p className="text-xs text-slate-500 mt-0.5">Select MCP server connections to enable for this agent</p>
          </div>
          <Link
            to="/settings/connections?tab=mcp_servers"
            className="rounded-lg ring-1 ring-slate-300 px-3 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-50 transition-colors"
          >
            Manage Connections
          </Link>
        </div>

        {loading ? (
          <p className="text-sm text-slate-400 py-4 text-center">Loading connections...</p>
        ) : availableConnections.length === 0 ? (
          <div className="text-center py-6">
            <p className="text-sm text-slate-400 mb-2">No MCP connections configured yet.</p>
            <Link
              to="/settings/connections?tab=mcp_servers"
              className="text-sm text-blue-600 hover:text-blue-800 font-medium"
            >
              Create one in Settings &gt; Connections
            </Link>
          </div>
        ) : (
          <div className="space-y-2">
            {availableConnections.map(conn => {
              const enabled = mcpConnectionNames.includes(conn.name)
              return (
                <div
                  key={conn.name}
                  className={`rounded-xl ring-1 px-4 py-3 transition-all cursor-pointer ${
                    enabled ? 'ring-emerald-200 bg-emerald-50/30' : 'ring-slate-200 hover:ring-slate-300'
                  }`}
                  onClick={() => toggleConnection(conn.name)}
                >
                  <div className="flex items-center gap-3">
                    <input
                      type="checkbox"
                      checked={enabled}
                      onChange={() => toggleConnection(conn.name)}
                      onClick={e => e.stopPropagation()}
                      className="rounded border-slate-300 text-emerald-600 focus:ring-emerald-500"
                    />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-slate-900">{conn.display_name}</span>
                        <span className="text-xs text-slate-400 font-mono">{conn.name}</span>
                      </div>
                      <div className="flex items-center gap-3 mt-0.5">
                        <span className="text-xs text-slate-500 truncate">{conn.url}</span>
                        <span className="text-xs text-slate-400">{authLabel(conn)}</span>
                      </div>
                    </div>
                  </div>
                  {enabled && (
                    <p className="text-[11px] text-slate-400 mt-1.5 pl-8">
                      Tools registered as <code className="bg-slate-100 px-1 rounded">mcp__{conn.name}__&lt;tool&gt;</code>
                    </p>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
