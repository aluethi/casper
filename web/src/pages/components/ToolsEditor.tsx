import { useState } from 'react'

// ── Built-in tool config types ───────────────────────────────────
const BUILTIN_TOOLS = [
  { name: 'delegate', label: 'Delegate', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 300 }, { key: 'max_depth', label: 'Max depth', type: 'number', default: 3 }] },
  { name: 'ask_user', label: 'Ask User', fields: [] },
  { name: 'knowledge_search', label: 'Knowledge Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 5 }, { key: 'relevance_threshold', label: 'Threshold', type: 'number', default: 0.7 }] },
  { name: 'update_memory', label: 'Update Memory', fields: [{ key: 'max_document_tokens', label: 'Max tokens', type: 'number', default: 4000 }] },
  { name: 'web_search', label: 'Web Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 10 }] },
  { name: 'web_fetch', label: 'Web Fetch', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 30 }, { key: 'max_response_bytes', label: 'Max bytes', type: 'number', default: 1048576 }] },
]

// ── MCP server type ─────────────────────────────────────────────
export interface McpServer {
  name: string
  url: string
  api_key?: string
}

interface ToolsEditorProps {
  builtinTools: Record<string, Record<string, unknown>>
  setBuiltinTools: React.Dispatch<React.SetStateAction<Record<string, Record<string, unknown>>>>
  mcpServers: McpServer[]
  setMcpServers: React.Dispatch<React.SetStateAction<McpServer[]>>
}

export default function ToolsEditor({ builtinTools, setBuiltinTools, mcpServers, setMcpServers }: ToolsEditorProps) {
  const [showAddMcp, setShowAddMcp] = useState(false)
  const [newMcp, setNewMcp] = useState<McpServer>({ name: '', url: '' })

  const addMcpServer = () => {
    if (!newMcp.name.trim() || !newMcp.url.trim()) return
    // Prevent duplicate names
    if (mcpServers.some(s => s.name === newMcp.name.trim())) return
    setMcpServers([...mcpServers, {
      name: newMcp.name.trim(),
      url: newMcp.url.trim(),
      api_key: newMcp.api_key?.trim() || undefined,
    }])
    setNewMcp({ name: '', url: '' })
    setShowAddMcp(false)
  }

  const removeMcpServer = (name: string) => {
    setMcpServers(mcpServers.filter(s => s.name !== name))
  }

  const updateMcpServer = (idx: number, field: keyof McpServer, value: string) => {
    const next = [...mcpServers]
    next[idx] = { ...next[idx], [field]: value || undefined }
    setMcpServers(next)
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

      {/* MCP Servers */}
      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="font-display text-lg tracking-tight text-slate-900">MCP Servers</h2>
            <p className="text-xs text-slate-500 mt-0.5">Connect external tool servers via the Model Context Protocol</p>
          </div>
          <button
            onClick={() => setShowAddMcp(true)}
            className="rounded-lg bg-blue-600 text-white px-3 py-1.5 text-xs font-semibold hover:bg-blue-500 transition-colors"
          >
            + Add Server
          </button>
        </div>

        {/* Existing MCP servers */}
        <div className="space-y-3">
          {mcpServers.length === 0 && !showAddMcp && (
            <p className="text-sm text-slate-400 py-4 text-center">No MCP servers configured. Tools from MCP servers will be auto-discovered at runtime.</p>
          )}
          {mcpServers.map((server, idx) => (
            <div key={server.name} className="rounded-xl ring-1 ring-emerald-200 bg-emerald-50/30 px-4 py-3">
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-medium text-slate-900">{server.name}</span>
                <button
                  onClick={() => removeMcpServer(server.name)}
                  className="text-xs text-red-600 hover:text-red-800 transition-colors"
                >
                  Remove
                </button>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">URL</label>
                  <input
                    type="text"
                    value={server.url}
                    onChange={e => updateMcpServer(idx, 'url', e.target.value)}
                    placeholder="https://mcp-server.example.com/mcp"
                    className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm"
                  />
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">API Key (optional)</label>
                  <input
                    type="password"
                    value={server.api_key || ''}
                    onChange={e => updateMcpServer(idx, 'api_key', e.target.value)}
                    placeholder="Bearer token"
                    className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm"
                  />
                </div>
              </div>
              <p className="text-[11px] text-slate-400 mt-1.5">
                Tools will be registered as <code className="bg-slate-100 px-1 rounded">mcp__{server.name}__&lt;tool&gt;</code>
              </p>
            </div>
          ))}

          {/* Add MCP server form */}
          {showAddMcp && (
            <div className="rounded-xl ring-1 ring-blue-200 bg-blue-50/20 px-4 py-3">
              <div className="grid grid-cols-3 gap-3 mb-3">
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">Name</label>
                  <input
                    type="text"
                    value={newMcp.name}
                    onChange={e => setNewMcp({ ...newMcp, name: e.target.value.replace(/[^a-z0-9_-]/gi, '_').toLowerCase() })}
                    placeholder="e.g. jira, github"
                    className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm"
                    autoFocus
                  />
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">URL</label>
                  <input
                    type="text"
                    value={newMcp.url}
                    onChange={e => setNewMcp({ ...newMcp, url: e.target.value })}
                    placeholder="https://mcp-server.example.com/mcp"
                    className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm"
                  />
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">API Key (optional)</label>
                  <input
                    type="password"
                    value={newMcp.api_key || ''}
                    onChange={e => setNewMcp({ ...newMcp, api_key: e.target.value })}
                    placeholder="Bearer token"
                    className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm"
                  />
                </div>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={addMcpServer}
                  disabled={!newMcp.name.trim() || !newMcp.url.trim()}
                  className="rounded-lg bg-blue-600 text-white px-3 py-1 text-xs font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors"
                >
                  Add
                </button>
                <button
                  onClick={() => { setShowAddMcp(false); setNewMcp({ name: '', url: '' }) }}
                  className="rounded-lg ring-1 ring-slate-300 px-3 py-1 text-xs font-medium text-slate-600 hover:bg-slate-50 transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
