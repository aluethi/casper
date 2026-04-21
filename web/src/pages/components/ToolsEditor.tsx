import { useState, useEffect } from 'react'
import api from '../../lib/api'

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
export interface McpAuth {
  type: 'none' | 'bearer' | 'user_oauth' | 'mcp_oauth'
  token_ref?: string
  provider?: string
}

export interface McpServer {
  name: string
  url: string
  api_key?: string
  auth?: McpAuth
}

interface ToolsEditorProps {
  builtinTools: Record<string, Record<string, unknown>>
  setBuiltinTools: React.Dispatch<React.SetStateAction<Record<string, Record<string, unknown>>>>
  mcpServers: McpServer[]
  setMcpServers: React.Dispatch<React.SetStateAction<McpServer[]>>
}

interface OAuthProvider {
  name: string
  display_name: string
}

export default function ToolsEditor({ builtinTools, setBuiltinTools, mcpServers, setMcpServers }: ToolsEditorProps) {
  const [showAddMcp, setShowAddMcp] = useState(false)
  const [newMcp, setNewMcp] = useState<McpServer>({ name: '', url: '', auth: { type: 'none' } })
  const [oauthProviders, setOauthProviders] = useState<OAuthProvider[]>([])

  useEffect(() => {
    api.get('/api/v1/oauth-providers').then(r => {
      setOauthProviders((r.data || []).filter((p: { is_active: boolean }) => p.is_active))
    }).catch(() => {})
  }, [])

  const addMcpServer = () => {
    if (!newMcp.name.trim() || !newMcp.url.trim()) return
    if (mcpServers.some(s => s.name === newMcp.name.trim())) return
    const server: McpServer = {
      name: newMcp.name.trim(),
      url: newMcp.url.trim(),
    }
    const authType = newMcp.auth?.type || 'none'
    if (authType === 'bearer' && newMcp.api_key?.trim()) {
      server.api_key = newMcp.api_key.trim()
    }
    if (authType !== 'none') {
      server.auth = { ...newMcp.auth!, type: authType }
    }
    setMcpServers([...mcpServers, server])
    setNewMcp({ name: '', url: '', auth: { type: 'none' } })
    setShowAddMcp(false)
  }

  const removeMcpServer = (name: string) => {
    setMcpServers(mcpServers.filter(s => s.name !== name))
  }

  const updateMcpServer = (idx: number, updates: Partial<McpServer>) => {
    const next = [...mcpServers]
    next[idx] = { ...next[idx], ...updates }
    setMcpServers(next)
  }

  const updateMcpAuth = (idx: number, authType: string, provider?: string) => {
    const next = [...mcpServers]
    if (authType === 'none') {
      delete next[idx].auth
      delete next[idx].api_key
    } else if (authType === 'bearer') {
      next[idx].auth = { type: 'bearer' }
    } else if (authType === 'user_oauth') {
      next[idx].auth = { type: 'user_oauth', provider: provider || oauthProviders[0]?.name || '' }
      delete next[idx].api_key
    } else if (authType === 'mcp_oauth') {
      next[idx].auth = { type: 'mcp_oauth' }
      delete next[idx].api_key
    }
    setMcpServers(next)
  }

  const [discovering, setDiscovering] = useState(false)

  /** Discover OAuth config from the MCP server's .well-known URL,
   *  match against existing providers, or create a new one. */
  const discoverOAuthFromMcp = async (mcpUrl: string, applyTo: 'new' | number) => {
    if (!mcpUrl.trim()) return
    setDiscovering(true)
    try {
      // Derive possible .well-known base paths from the MCP URL
      // e.g. https://mcp.ventoo.ai/apps/mcp → try https://mcp.ventoo.ai/apps
      //      https://mcp.ventoo.ai/relay/mcp → try https://mcp.ventoo.ai/relay
      const url = new URL(mcpUrl.trim())
      const pathParts = url.pathname.replace(/\/+$/, '').split('/')
      // Try progressively shorter paths
      const basesToTry: string[] = []
      for (let i = pathParts.length; i >= 1; i--) {
        basesToTry.push(`${url.origin}${pathParts.slice(0, i).join('/')}`)
      }
      basesToTry.push(url.origin)

      let discovered: { authorization_url?: string; token_url?: string; revocation_url?: string; scopes_supported?: string[] } | null = null
      for (const base of basesToTry) {
        try {
          const res = await api.get('/api/v1/oauth-providers/discover', { params: { url: base } })
          if (res.data.authorization_url) { discovered = res.data; break }
        } catch { /* try next */ }
      }

      if (!discovered?.authorization_url) {
        alert('Could not find .well-known/openid-configuration at any path derived from the MCP server URL.')
        return
      }

      // TODO: match by provider details once authorization_url is in the list response

      // Try to match by fetching each provider's details
      let matchedProvider: string | null = null
      for (const p of oauthProviders) {
        try {
          const details = await api.get(`/api/v1/oauth-providers/${p.name}`)
          if (details.data.authorization_url === discovered.authorization_url) {
            matchedProvider = p.name
            break
          }
        } catch { /* skip */ }
      }

      if (matchedProvider) {
        // Auto-select the matched provider
        if (applyTo === 'new') {
          setNewMcp(m => ({ ...m, auth: { type: 'user_oauth', provider: matchedProvider! } }))
        } else {
          updateMcpAuth(applyTo, 'user_oauth', matchedProvider)
        }
      } else {
        // No match — prompt to create in Settings > Connections
        const name = prompt(
          `Discovered OAuth config:\n` +
          `  Auth: ${discovered.authorization_url}\n` +
          `  Token: ${discovered.token_url}\n\n` +
          `No matching provider found. Enter a name to create one (leave empty to cancel):`,
          url.hostname.split('.')[0]
        )
        if (!name) return

        // Create the provider (without client credentials — admin fills those in later)
        try {
          await api.post('/api/v1/oauth-providers', {
            name,
            display_name: name.charAt(0).toUpperCase() + name.slice(1),
            authorization_url: discovered.authorization_url,
            token_url: discovered.token_url,
            revocation_url: discovered.revocation_url || '',
            default_scopes: discovered.scopes_supported?.join(' ') || 'openid email profile',
            client_id: 'CONFIGURE_ME',
            client_secret: 'CONFIGURE_ME',
          })
          // Reload providers list
          const res = await api.get('/api/v1/oauth-providers')
          setOauthProviders((res.data || []).filter((p: { is_active: boolean }) => p.is_active))

          if (applyTo === 'new') {
            setNewMcp(m => ({ ...m, auth: { type: 'user_oauth', provider: name } }))
          } else {
            updateMcpAuth(applyTo, 'user_oauth', name)
          }
          alert(`Provider "${name}" created. Configure the client credentials in Settings > Connections.`)
        } catch (e: any) {
          alert(`Failed to create provider: ${e.response?.data?.message ?? e.message}`)
        }
      }
    } catch (e: any) {
      alert(`Discovery failed: ${e.message}`)
    } finally {
      setDiscovering(false)
    }
  }

  const getAuthType = (server: McpServer): string => {
    if (server.auth?.type) return server.auth.type
    if (server.api_key) return 'bearer'
    return 'none'
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
          {mcpServers.map((server, idx) => {
            const authType = getAuthType(server)
            return (
              <div key={server.name} className="rounded-xl ring-1 ring-emerald-200 bg-emerald-50/30 px-4 py-3">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium text-slate-900">{server.name}</span>
                  <button onClick={() => removeMcpServer(server.name)}
                    className="text-xs text-red-600 hover:text-red-800 transition-colors">Remove</button>
                </div>
                <div className="space-y-2">
                  <div>
                    <label className="block text-xs text-slate-500 mb-0.5">URL</label>
                    <input type="text" value={server.url}
                      onChange={e => updateMcpServer(idx, { url: e.target.value })}
                      placeholder="https://mcp-server.example.com/mcp"
                      className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-slate-500 mb-0.5">Authentication</label>
                    <div className="flex gap-3 items-center">
                      <select value={authType}
                        onChange={e => updateMcpAuth(idx, e.target.value)}
                        className="rounded-lg ring-1 ring-slate-300 bg-white px-2 py-1 text-sm">
                        <option value="none">None</option>
                        <option value="bearer">Bearer token</option>
                        <option value="user_oauth">User OAuth (manual)</option>
                        <option value="mcp_oauth">MCP OAuth 2.1 (auto-discover)</option>
                      </select>
                      {authType === 'bearer' && (
                        <input type="password" value={server.api_key || ''}
                          onChange={e => updateMcpServer(idx, { api_key: e.target.value || undefined })}
                          placeholder="Bearer token"
                          className="flex-1 rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                      )}
                      {authType === 'user_oauth' && (<>
                        <select value={server.auth?.provider || ''}
                          onChange={e => updateMcpAuth(idx, 'user_oauth', e.target.value)}
                          className="rounded-lg ring-1 ring-slate-300 bg-white px-2 py-1 text-sm">
                          {oauthProviders.length === 0 && <option value="">No providers configured</option>}
                          {oauthProviders.map(p => (
                            <option key={p.name} value={p.name}>{p.display_name}</option>
                          ))}
                        </select>
                        <button onClick={() => discoverOAuthFromMcp(server.url, idx)} disabled={discovering}
                          className="text-xs text-blue-600 hover:text-blue-800 font-medium whitespace-nowrap disabled:opacity-40">
                          {discovering ? 'Discovering...' : 'Discover'}
                        </button>
                      </>)}
                    </div>
                    {authType === 'user_oauth' && (
                      <p className="text-[11px] text-amber-600 mt-1">
                        Each user must connect their {server.auth?.provider || 'provider'} account in Settings &gt; Connections.
                      </p>
                    )}
                  </div>
                </div>
                <p className="text-[11px] text-slate-400 mt-1.5">
                  Tools will be registered as <code className="bg-slate-100 px-1 rounded">mcp__{server.name}__&lt;tool&gt;</code>
                </p>
              </div>
            )
          })}

          {/* Add MCP server form */}
          {showAddMcp && (
            <div className="rounded-xl ring-1 ring-blue-200 bg-blue-50/20 px-4 py-3">
              <div className="space-y-3 mb-3">
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-xs text-slate-500 mb-0.5">Name</label>
                    <input type="text" value={newMcp.name}
                      onChange={e => setNewMcp({ ...newMcp, name: e.target.value.replace(/[^a-z0-9_-]/gi, '_').toLowerCase() })}
                      placeholder="e.g. jira, microsoft365"
                      className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm"
                      autoFocus />
                  </div>
                  <div>
                    <label className="block text-xs text-slate-500 mb-0.5">URL</label>
                    <input type="text" value={newMcp.url}
                      onChange={e => setNewMcp({ ...newMcp, url: e.target.value })}
                      placeholder="https://mcp-server.example.com/mcp"
                      className="w-full rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                  </div>
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">Authentication</label>
                  <div className="flex gap-3 items-center">
                    <select value={newMcp.auth?.type || 'none'}
                      onChange={e => {
                        const t = e.target.value as McpAuth['type']
                        if (t === 'user_oauth') {
                          setNewMcp({ ...newMcp, api_key: undefined, auth: { type: 'user_oauth', provider: oauthProviders[0]?.name || '' } })
                        } else if (t === 'bearer') {
                          setNewMcp({ ...newMcp, auth: { type: 'bearer' } })
                        } else {
                          setNewMcp({ ...newMcp, api_key: undefined, auth: { type: 'none' } })
                        }
                      }}
                      className="rounded-lg ring-1 ring-slate-300 bg-white px-2 py-1 text-sm">
                      <option value="none">None</option>
                      <option value="bearer">Bearer token</option>
                      <option value="user_oauth">User OAuth</option>
                    </select>
                    {newMcp.auth?.type === 'bearer' && (
                      <input type="password" value={newMcp.api_key || ''}
                        onChange={e => setNewMcp({ ...newMcp, api_key: e.target.value })}
                        placeholder="Bearer token"
                        className="flex-1 rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                    )}
                    {newMcp.auth?.type === 'user_oauth' && (<>
                      <select value={newMcp.auth?.provider || ''}
                        onChange={e => setNewMcp({ ...newMcp, auth: { type: 'user_oauth', provider: e.target.value } })}
                        className="rounded-lg ring-1 ring-slate-300 bg-white px-2 py-1 text-sm">
                        {oauthProviders.length === 0 && <option value="">No providers configured</option>}
                        {oauthProviders.map(p => (
                          <option key={p.name} value={p.name}>{p.display_name}</option>
                        ))}
                      </select>
                      <button onClick={() => discoverOAuthFromMcp(newMcp.url, 'new')} disabled={discovering || !newMcp.url.trim()}
                        className="text-xs text-blue-600 hover:text-blue-800 font-medium whitespace-nowrap disabled:opacity-40">
                        {discovering ? 'Discovering...' : 'Discover'}
                      </button>
                    </>)}
                  </div>
                  {newMcp.auth?.type === 'user_oauth' && (
                    <p className="text-[11px] text-amber-600 mt-1">
                      Each user must connect their account in Settings &gt; Connections before using this tool.
                    </p>
                  )}
                </div>
              </div>
              <div className="flex gap-2">
                <button onClick={addMcpServer}
                  disabled={!newMcp.name.trim() || !newMcp.url.trim()}
                  className="rounded-lg bg-blue-600 text-white px-3 py-1 text-xs font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                  Add
                </button>
                <button
                  onClick={() => { setShowAddMcp(false); setNewMcp({ name: '', url: '', auth: { type: 'none' } }) }}
                  className="rounded-lg ring-1 ring-slate-300 px-3 py-1 text-xs font-medium text-slate-600 hover:bg-slate-50 transition-colors">
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
