import { useState, useEffect } from 'react'
import { useSearchParams } from 'react-router-dom'
import api from '../lib/api'
import type { McpConnection } from '../types'

interface OAuthProvider {
  id: string
  name: string
  display_name: string
  authorization_url: string
  token_url: string
  revocation_url: string | null
  client_id: string
  default_scopes: string
  icon_url: string | null
  is_active: boolean
  created_at: string
}

interface UserConnection {
  id: string
  user_subject: string
  provider: string
  granted_scopes: string
  external_email: string | null
  token_expires_at: string | null
  created_at: string
}

interface AvailableProvider {
  name: string
  display_name: string
  icon_url: string | null
  connected: boolean
}

interface MyConnection {
  id: string
  provider: string
  granted_scopes: string
  external_email: string | null
  token_expires_at: string | null
  created_at: string
  updated_at: string
}

const emptyForm = {
  name: '', display_name: '', authorization_url: '', token_url: '',
  revocation_url: '', client_id: '', client_secret: '', default_scopes: '', icon_url: '',
}

type McpAuthType = 'none' | 'bearer' | 'user_oauth' | 'mcp_oauth'
interface McpFormState {
  name: string
  display_name: string
  url: string
  auth_type: McpAuthType
  api_key: string
  auth_provider: string
}

const emptyMcpForm: McpFormState = {
  name: '', display_name: '', url: '', auth_type: 'none', api_key: '', auth_provider: '',
}

type TabKey = 'connections' | 'providers' | 'mcp_servers'

export default function ConnectionsPage() {
  const [searchParams, setSearchParams] = useSearchParams()
  const [providers, setProviders] = useState<OAuthProvider[]>([])
  const [connections, setConnections] = useState<UserConnection[]>([])
  const [available, setAvailable] = useState<AvailableProvider[]>([])
  const [myConnections, setMyConnections] = useState<MyConnection[]>([])
  const [mcpConnections, setMcpConnections] = useState<McpConnection[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')
  const [connecting, setConnecting] = useState<string | null>(null)

  // Create form
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ ...emptyForm })
  const [saving, setSaving] = useState(false)
  const [discoverUrl, setDiscoverUrl] = useState('')
  const [discovering, setDiscovering] = useState(false)

  // MCP registration
  const [showMcpRegister, setShowMcpRegister] = useState(false)
  const [mcpUrl, setMcpUrl] = useState('')
  const [mcpDisplayName, setMcpDisplayName] = useState('')
  const [registering, setRegistering] = useState(false)

  // Re-register MCP
  const [reregisterName, setReregisterName] = useState<string | null>(null)
  const [reregisterUrl, setReregisterUrl] = useState('')
  const [reregistering, setReregistering] = useState(false)

  // Edit
  const [editName, setEditName] = useState<string | null>(null)
  const [editForm, setEditForm] = useState({ ...emptyForm })

  // MCP connection create/edit
  const [showMcpCreate, setShowMcpCreate] = useState(false)
  const [mcpForm, setMcpForm] = useState<McpFormState>({ ...emptyMcpForm })
  const [mcpSaving, setMcpSaving] = useState(false)
  const [mcpEditName, setMcpEditName] = useState<string | null>(null)
  const [mcpEditForm, setMcpEditForm] = useState<McpFormState>({ ...emptyMcpForm })

  // Tab
  const initialTab = (searchParams.get('tab') as TabKey) || 'connections'
  const [tab, setTab] = useState<TabKey>(initialTab)

  // Handle ?connected= param from OAuth callback redirect
  useEffect(() => {
    const connectedProvider = searchParams.get('connected')
    if (connectedProvider) {
      setTab('connections')
      setSuccess(`Successfully connected to ${connectedProvider}!`)
      setSearchParams({}, { replace: true })
    }
  }, [])

  useEffect(() => { reload() }, [])

  const reload = async () => {
    setLoading(true)
    try {
      const [provRes, connRes, availRes, myRes, mcpRes] = await Promise.all([
        api.get('/api/v1/oauth-providers').catch(() => ({ data: [] })),
        api.get('/api/v1/connections/all').catch(() => ({ data: [] })),
        api.get('/api/v1/connections/available').catch(() => ({ data: [] })),
        api.get('/api/v1/connections').catch(() => ({ data: [] })),
        api.get('/api/v1/mcp-connections').catch(() => ({ data: [] })),
      ])
      setProviders(provRes.data || [])
      setConnections(connRes.data || [])
      setAvailable(availRes.data || [])
      setMyConnections(myRes.data || [])
      setMcpConnections(mcpRes.data || [])
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setLoading(false)
    }
  }

  const createProvider = async () => {
    setSaving(true); setError('')
    try {
      const body: Record<string, string> = {}
      for (const [k, v] of Object.entries(form)) {
        if (v.trim()) body[k] = v.trim()
      }
      await api.post('/api/v1/oauth-providers', body)
      setForm({ ...emptyForm })
      setShowCreate(false)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setSaving(false) }
  }

  const discoverConfig = async () => {
    if (!discoverUrl.trim()) return
    setDiscovering(true); setError('')
    try {
      const res = await api.get('/api/v1/oauth-providers/discover', {
        params: { url: discoverUrl.trim() },
      })
      const d = res.data
      setForm(f => ({
        ...f,
        authorization_url: d.authorization_url || f.authorization_url,
        token_url: d.token_url || f.token_url,
        revocation_url: d.revocation_url || f.revocation_url,
        default_scopes: d.scopes_supported?.length
          ? d.scopes_supported.join(' ')
          : f.default_scopes,
      }))
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setDiscovering(false) }
  }

  const startEdit = (p: OAuthProvider) => {
    setEditName(p.name)
    setEditForm({
      name: p.name, display_name: p.display_name,
      authorization_url: p.authorization_url, token_url: p.token_url,
      revocation_url: p.revocation_url || '', client_id: p.client_id,
      client_secret: '', default_scopes: p.default_scopes, icon_url: p.icon_url || '',
    })
  }

  const saveEdit = async () => {
    if (!editName) return
    setSaving(true); setError('')
    try {
      const body: Record<string, string> = {}
      for (const [k, v] of Object.entries(editForm)) {
        if (k === 'name') continue
        if (k === 'client_secret' && !v.trim()) continue
        if (v.trim()) body[k] = v.trim()
      }
      await api.patch(`/api/v1/oauth-providers/${editName}`, body)
      setEditName(null)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setSaving(false) }
  }

  const toggleActive = async (name: string, active: boolean) => {
    try {
      await api.patch(`/api/v1/oauth-providers/${name}`, { is_active: active })
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const deleteProvider = async (name: string) => {
    if (!confirm(`Deactivate provider "${name}"? Existing user connections will stop working.`)) return
    try {
      await api.delete(`/api/v1/oauth-providers/${name}`)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const revokeConnection = async (userSubject: string, provider: string) => {
    if (!confirm(`Revoke ${provider} connection for ${userSubject}?`)) return
    try {
      await api.delete(`/api/v1/connections/${encodeURIComponent(userSubject)}/${provider}`)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const startConnect = async (providerName: string) => {
    setConnecting(providerName); setError('')
    try {
      const res = await api.post(`/api/v1/connections/${encodeURIComponent(providerName)}/start`)
      window.location.href = res.data.redirect_url
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
      setConnecting(null)
    }
  }

  const disconnectMe = async (providerName: string) => {
    if (!confirm(`Disconnect from ${providerName}?`)) return
    setError('')
    try {
      await api.delete(`/api/v1/connections/${encodeURIComponent(providerName)}`)
      setSuccess(`Disconnected from ${providerName}.`)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const registerMcp = async () => {
    if (!mcpUrl.trim()) return
    setRegistering(true); setError('')
    try {
      const body: Record<string, string> = { mcp_url: mcpUrl.trim() }
      if (mcpDisplayName.trim()) body.display_name = mcpDisplayName.trim()
      await api.post('/api/v1/oauth-providers/register-mcp', body)
      setMcpUrl(''); setMcpDisplayName('')
      setShowMcpRegister(false)
      setSuccess('MCP server registered successfully.')
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setRegistering(false) }
  }

  const startReregister = (p: OAuthProvider) => {
    const host = p.name.replace(/^mcp:/, '')
    setReregisterName(p.name)
    setReregisterUrl(`https://${host}/`)
  }

  const reregisterMcp = async () => {
    if (!reregisterUrl.trim() || !reregisterName) return
    setReregistering(true); setError('')
    try {
      await api.post('/api/v1/oauth-providers/register-mcp', { mcp_url: reregisterUrl.trim() })
      setReregisterName(null); setReregisterUrl('')
      setSuccess('MCP server re-registered successfully. Client credentials have been updated.')
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setReregistering(false) }
  }

  // ── MCP Connection CRUD ──────────────────────────────────────────

  const createMcpConnection = async () => {
    setMcpSaving(true); setError('')
    try {
      const body: Record<string, string> = {
        name: mcpForm.name.trim(),
        display_name: mcpForm.display_name.trim(),
        url: mcpForm.url.trim(),
        auth_type: mcpForm.auth_type,
      }
      if (mcpForm.auth_type === 'bearer' && mcpForm.api_key.trim()) {
        body.api_key = mcpForm.api_key.trim()
      }
      if ((mcpForm.auth_type === 'user_oauth') && mcpForm.auth_provider.trim()) {
        body.auth_provider = mcpForm.auth_provider.trim()
      }
      await api.post('/api/v1/mcp-connections', body)
      setMcpForm({ ...emptyMcpForm })
      setShowMcpCreate(false)
      setSuccess('MCP connection created.')
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setMcpSaving(false) }
  }

  const startMcpEdit = (c: McpConnection) => {
    setMcpEditName(c.name)
    setMcpEditForm({
      name: c.name,
      display_name: c.display_name,
      url: c.url,
      auth_type: c.auth_type,
      api_key: '',
      auth_provider: c.auth_provider || '',
    })
  }

  const saveMcpEdit = async () => {
    if (!mcpEditName) return
    setMcpSaving(true); setError('')
    try {
      const body: Record<string, string | undefined> = {}
      if (mcpEditForm.display_name.trim()) body.display_name = mcpEditForm.display_name.trim()
      if (mcpEditForm.url.trim()) body.url = mcpEditForm.url.trim()
      body.auth_type = mcpEditForm.auth_type
      if (mcpEditForm.api_key.trim()) body.api_key = mcpEditForm.api_key.trim()
      if (mcpEditForm.auth_provider.trim()) body.auth_provider = mcpEditForm.auth_provider.trim()
      await api.patch(`/api/v1/mcp-connections/${mcpEditName}`, body)
      setMcpEditName(null)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally { setMcpSaving(false) }
  }

  const toggleMcpActive = async (name: string, active: boolean) => {
    try {
      await api.patch(`/api/v1/mcp-connections/${name}`, { is_active: active })
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const deleteMcpConnection = async (name: string) => {
    if (!confirm(`Deactivate MCP connection "${name}"? Agents using it will lose access to its tools.`)) return
    try {
      await api.delete(`/api/v1/mcp-connections/${name}`)
      await reload()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Connections</h1>
        {tab === 'providers' && (
          <div className="flex gap-2">
            <button onClick={() => { setShowMcpRegister(!showMcpRegister); setShowCreate(false) }}
              className="bg-slate-700 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-slate-600 transition-colors">
              + Add MCP Server
            </button>
            <button onClick={() => { setShowCreate(!showCreate); setShowMcpRegister(false) }}
              className="rounded-full text-sm font-semibold text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">
              + Manual Provider
            </button>
          </div>
        )}
        {tab === 'mcp_servers' && (
          <button onClick={() => setShowMcpCreate(!showMcpCreate)}
            className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 transition-colors">
            + Add MCP Connection
          </button>
        )}
      </div>
      <p className="text-sm text-slate-500 mb-4">
        Manage MCP server connections, OAuth providers, and user connections.
      </p>

      {success && (
        <div className="bg-green-50 text-green-700 p-3 rounded-xl ring-1 ring-green-200 text-sm mb-4">
          {success}<button onClick={() => setSuccess('')} className="ml-2 underline">dismiss</button>
        </div>
      )}

      {error && (
        <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">
          {error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button>
        </div>
      )}

      {/* Tabs */}
      <div className="border-b border-slate-200 mb-6">
        <div className="flex gap-6">
          {([['connections', 'My Connections'], ['mcp_servers', 'MCP Servers'], ['providers', 'OAuth Providers']] as const).map(([t, label]) => (
            <button key={t} onClick={() => setTab(t as TabKey)}
              className={`pb-3 text-sm font-medium border-b-2 transition-colors ${
                tab === t ? 'border-blue-600 text-blue-600' : 'border-transparent text-slate-500 hover:text-slate-700'
              }`}>
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* MCP Servers tab */}
      {tab === 'mcp_servers' && (
        <>
          {/* Create form */}
          {showMcpCreate && (
            <div className="bg-white rounded-2xl ring-1 ring-blue-200 shadow-sm p-6 mb-6">
              <h3 className="text-sm font-semibold text-slate-900 mb-3">New MCP Connection</h3>
              <div className="grid grid-cols-2 gap-3 mb-3">
                <Field label="Name (slug)" value={mcpForm.name}
                  onChange={v => setMcpForm({ ...mcpForm, name: v.replace(/[^a-z0-9_-]/gi, '_').toLowerCase() })}
                  placeholder="e.g. jira, github, asana" />
                <Field label="Display Name" value={mcpForm.display_name}
                  onChange={v => setMcpForm({ ...mcpForm, display_name: v })}
                  placeholder="Jira Cloud" />
                <Field label="MCP Server URL" value={mcpForm.url}
                  onChange={v => setMcpForm({ ...mcpForm, url: v })}
                  placeholder="https://mcp-server.example.com/mcp" className="col-span-2" />
                <div>
                  <label className="block text-xs text-slate-500 mb-0.5">Authentication</label>
                  <select value={mcpForm.auth_type}
                    onChange={e => setMcpForm({ ...mcpForm, auth_type: e.target.value as McpAuthType, api_key: '', auth_provider: '' })}
                    className="w-full rounded-lg ring-1 ring-slate-300 bg-white px-3 py-1.5 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none">
                    <option value="none">None</option>
                    <option value="bearer">Bearer token</option>
                    <option value="user_oauth">User OAuth (manual provider)</option>
                    <option value="mcp_oauth">MCP OAuth 2.1 (auto-discover)</option>
                  </select>
                </div>
                {mcpForm.auth_type === 'bearer' && (
                  <Field label="API Key / Bearer Token" value={mcpForm.api_key}
                    onChange={v => setMcpForm({ ...mcpForm, api_key: v })}
                    type="password" placeholder="sk-..." />
                )}
                {mcpForm.auth_type === 'user_oauth' && (
                  <div>
                    <label className="block text-xs text-slate-500 mb-0.5">OAuth Provider</label>
                    <select value={mcpForm.auth_provider}
                      onChange={e => setMcpForm({ ...mcpForm, auth_provider: e.target.value })}
                      className="w-full rounded-lg ring-1 ring-slate-300 bg-white px-3 py-1.5 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none">
                      <option value="">Select provider...</option>
                      {providers.filter(p => p.is_active).map(p => (
                        <option key={p.name} value={p.name}>{p.display_name}</option>
                      ))}
                    </select>
                  </div>
                )}
              </div>
              {mcpForm.auth_type === 'user_oauth' && (
                <p className="text-[11px] text-amber-600 mb-3">
                  Each user must connect their account in the "My Connections" tab before this server's tools can authenticate on their behalf.
                </p>
              )}
              <div className="flex gap-2">
                <button onClick={createMcpConnection}
                  disabled={mcpSaving || !mcpForm.name.trim() || !mcpForm.url.trim() || !mcpForm.display_name.trim()}
                  className="bg-blue-600 text-white px-4 py-1.5 rounded-full text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                  {mcpSaving ? 'Creating...' : 'Create'}
                </button>
                <button onClick={() => { setShowMcpCreate(false); setMcpForm({ ...emptyMcpForm }) }}
                  className="rounded-full text-sm font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-1.5 transition-colors">
                  Cancel
                </button>
              </div>
            </div>
          )}

          {/* MCP connections table */}
          <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
            <table className="w-full text-sm">
              <thead className="bg-slate-50 text-xs uppercase text-slate-500">
                <tr>
                  <th className="text-left px-4 py-3 font-medium">Connection</th>
                  <th className="text-left px-4 py-3 font-medium">URL</th>
                  <th className="text-left px-4 py-3 font-medium">Auth</th>
                  <th className="text-left px-4 py-3 font-medium">Status</th>
                  <th className="text-right px-4 py-3 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {mcpConnections.length === 0 && (
                  <tr><td colSpan={5} className="px-4 py-8 text-center text-slate-400">No MCP connections configured. Click "+ Add MCP Connection" to get started.</td></tr>
                )}
                {mcpConnections.map(c => {
                  const isEditing = mcpEditName === c.name
                  return isEditing ? (
                    <tr key={c.name} className="bg-blue-50/50">
                      <td colSpan={5} className="px-4 py-4">
                        <h4 className="text-xs font-semibold text-blue-800 mb-2">Editing: {c.name}</h4>
                        <div className="grid grid-cols-2 gap-3 mb-3">
                          <Field label="Display Name" value={mcpEditForm.display_name}
                            onChange={v => setMcpEditForm({ ...mcpEditForm, display_name: v })} />
                          <Field label="URL" value={mcpEditForm.url}
                            onChange={v => setMcpEditForm({ ...mcpEditForm, url: v })} />
                          <div>
                            <label className="block text-xs text-slate-500 mb-0.5">Authentication</label>
                            <select value={mcpEditForm.auth_type}
                              onChange={e => setMcpEditForm({ ...mcpEditForm, auth_type: e.target.value as McpAuthType })}
                              className="w-full rounded-lg ring-1 ring-slate-300 bg-white px-3 py-1.5 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none">
                              <option value="none">None</option>
                              <option value="bearer">Bearer token</option>
                              <option value="user_oauth">User OAuth</option>
                              <option value="mcp_oauth">MCP OAuth 2.1</option>
                            </select>
                          </div>
                          {mcpEditForm.auth_type === 'bearer' && (
                            <Field label="API Key (leave empty to keep)" value={mcpEditForm.api_key}
                              onChange={v => setMcpEditForm({ ...mcpEditForm, api_key: v })} type="password" />
                          )}
                          {mcpEditForm.auth_type === 'user_oauth' && (
                            <div>
                              <label className="block text-xs text-slate-500 mb-0.5">OAuth Provider</label>
                              <select value={mcpEditForm.auth_provider}
                                onChange={e => setMcpEditForm({ ...mcpEditForm, auth_provider: e.target.value })}
                                className="w-full rounded-lg ring-1 ring-slate-300 bg-white px-3 py-1.5 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none">
                                <option value="">Select provider...</option>
                                {providers.filter(p => p.is_active).map(p => (
                                  <option key={p.name} value={p.name}>{p.display_name}</option>
                                ))}
                              </select>
                            </div>
                          )}
                        </div>
                        <div className="flex gap-2">
                          <button onClick={saveMcpEdit} disabled={mcpSaving}
                            className="bg-blue-600 text-white px-4 py-1.5 rounded-full text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                            {mcpSaving ? 'Saving...' : 'Save'}
                          </button>
                          <button onClick={() => setMcpEditName(null)}
                            className="rounded-full text-sm font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-1.5 transition-colors">
                            Cancel
                          </button>
                        </div>
                      </td>
                    </tr>
                  ) : (
                    <tr key={c.name} className={`hover:bg-slate-50 ${!c.is_active ? 'opacity-50' : ''}`}>
                      <td className="px-4 py-3">
                        <div className="font-medium text-slate-900">{c.display_name}</div>
                        <div className="text-xs text-slate-400 font-mono">{c.name}</div>
                      </td>
                      <td className="px-4 py-3 text-xs text-slate-500 max-w-64 truncate font-mono">{c.url}</td>
                      <td className="px-4 py-3 text-xs text-slate-500">
                        {c.auth_type === 'none' && 'None'}
                        {c.auth_type === 'bearer' && 'Bearer token'}
                        {c.auth_type === 'user_oauth' && `OAuth (${c.auth_provider || '?'})`}
                        {c.auth_type === 'mcp_oauth' && 'MCP OAuth 2.1'}
                      </td>
                      <td className="px-4 py-3">
                        <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ${
                          c.is_active
                            ? 'bg-green-50 text-green-700 ring-green-600/20'
                            : 'bg-slate-50 text-slate-500 ring-slate-300'
                        }`}>
                          {c.is_active ? 'Active' : 'Inactive'}
                        </span>
                      </td>
                      <td className="px-4 py-3 text-right space-x-2">
                        <button onClick={() => startMcpEdit(c)} className="text-xs text-blue-600 hover:text-blue-800">Edit</button>
                        {c.is_active ? (
                          <button onClick={() => toggleMcpActive(c.name, false)} className="text-xs text-amber-600 hover:text-amber-800">Deactivate</button>
                        ) : (
                          <button onClick={() => toggleMcpActive(c.name, true)} className="text-xs text-green-600 hover:text-green-800">Activate</button>
                        )}
                        <button onClick={() => deleteMcpConnection(c.name)} className="text-xs text-red-600 hover:text-red-800">Delete</button>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </>
      )}

      {/* Providers tab */}
      {tab === 'providers' && (
        <>
          {/* MCP registration form */}
          {showMcpRegister && (
            <div className="bg-white rounded-2xl ring-1 ring-blue-200 shadow-sm p-6 mb-6">
              <h3 className="text-sm font-semibold text-slate-900 mb-1">Register MCP Server</h3>
              <p className="text-xs text-slate-500 mb-4">
                Auto-discovers OAuth endpoints and registers a client via Dynamic Client Registration (RFC 7591).
              </p>
              <div className="grid grid-cols-2 gap-3 mb-3">
                <Field label="MCP Server URL" value={mcpUrl} onChange={setMcpUrl}
                  placeholder="https://mcp.example.com/v1/mcp" className="col-span-2" />
                <Field label="Display Name (optional)" value={mcpDisplayName} onChange={setMcpDisplayName}
                  placeholder="Derived from URL if empty" />
              </div>
              <div className="flex gap-2">
                <button onClick={registerMcp} disabled={registering || !mcpUrl.trim()}
                  className="bg-blue-600 text-white px-4 py-1.5 rounded-full text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                  {registering ? 'Discovering & registering...' : 'Register'}
                </button>
                <button onClick={() => { setShowMcpRegister(false); setMcpUrl(''); setMcpDisplayName('') }}
                  className="rounded-full text-sm font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-1.5 transition-colors">
                  Cancel
                </button>
              </div>
            </div>
          )}

          {/* Create form */}
          {showCreate && (
            <div className="bg-white rounded-2xl ring-1 ring-blue-200 shadow-sm p-6 mb-6">
              <h3 className="text-sm font-semibold text-slate-900 mb-3">New OAuth Provider</h3>

              {/* Auto-discovery */}
              <div className="mb-4 p-3 bg-slate-50 rounded-xl ring-1 ring-slate-200">
                <label className="block text-xs font-medium text-slate-600 mb-1">Auto-discover from .well-known URL</label>
                <div className="flex gap-2">
                  <input type="text" value={discoverUrl}
                    onChange={e => setDiscoverUrl(e.target.value)}
                    onKeyDown={e => { if (e.key === 'Enter') discoverConfig() }}
                    placeholder="https://login.microsoftonline.com/common/v2.0 or full .well-known URL"
                    className="flex-1 rounded-lg ring-1 ring-slate-300 px-3 py-1.5 text-sm focus:ring-2 focus:ring-blue-600 focus:outline-none" />
                  <button onClick={discoverConfig} disabled={discovering || !discoverUrl.trim()}
                    className="rounded-lg bg-slate-700 text-white px-4 py-1.5 text-sm font-medium hover:bg-slate-600 disabled:opacity-40 transition-colors whitespace-nowrap">
                    {discovering ? 'Discovering...' : 'Discover'}
                  </button>
                </div>
                <p className="text-[11px] text-slate-400 mt-1">
                  Fetches authorization, token, and revocation endpoints automatically. You still need to provide client credentials.
                </p>
              </div>

              <div className="grid grid-cols-2 gap-3 mb-3">
                <Field label="Name (slug)" value={form.name} onChange={v => setForm({ ...form, name: v.replace(/[^a-z0-9_-]/gi, '').toLowerCase() })} placeholder="microsoft365" />
                <Field label="Display Name" value={form.display_name} onChange={v => setForm({ ...form, display_name: v })} placeholder="Microsoft 365" />
                <Field label="Authorization URL" value={form.authorization_url} onChange={v => setForm({ ...form, authorization_url: v })} placeholder="https://login.microsoftonline.com/common/oauth2/v2.0/authorize" />
                <Field label="Token URL" value={form.token_url} onChange={v => setForm({ ...form, token_url: v })} placeholder="https://login.microsoftonline.com/common/oauth2/v2.0/token" />
                <Field label="Client ID" value={form.client_id} onChange={v => setForm({ ...form, client_id: v })} />
                <Field label="Client Secret" value={form.client_secret} onChange={v => setForm({ ...form, client_secret: v })} type="password" />
                <Field label="Default Scopes" value={form.default_scopes} onChange={v => setForm({ ...form, default_scopes: v })} placeholder="openid email profile offline_access" className="col-span-2" />
                <Field label="Revocation URL (optional)" value={form.revocation_url} onChange={v => setForm({ ...form, revocation_url: v })} />
                <Field label="Icon URL (optional)" value={form.icon_url} onChange={v => setForm({ ...form, icon_url: v })} />
              </div>
              <div className="flex gap-2">
                <button onClick={createProvider} disabled={saving || !form.name || !form.client_id || !form.client_secret}
                  className="bg-blue-600 text-white px-4 py-1.5 rounded-full text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                  {saving ? 'Creating...' : 'Create Provider'}
                </button>
                <button onClick={() => { setShowCreate(false); setForm({ ...emptyForm }) }}
                  className="rounded-full text-sm font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-1.5 transition-colors">
                  Cancel
                </button>
              </div>
            </div>
          )}

          {/* Providers table */}
          <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
            <table className="w-full text-sm">
              <thead className="bg-slate-50 text-xs uppercase text-slate-500">
                <tr>
                  <th className="text-left px-4 py-3 font-medium">Provider</th>
                  <th className="text-left px-4 py-3 font-medium">Client ID</th>
                  <th className="text-left px-4 py-3 font-medium">Scopes</th>
                  <th className="text-left px-4 py-3 font-medium">Status</th>
                  <th className="text-left px-4 py-3 font-medium">Users</th>
                  <th className="text-right px-4 py-3 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {providers.length === 0 && (
                  <tr><td colSpan={6} className="px-4 py-8 text-center text-slate-400">No OAuth providers configured.</td></tr>
                )}
                {providers.map(p => {
                  const userCount = connections.filter(c => c.provider === p.name).length
                  const isMcp = p.name.startsWith('mcp:')
                  const isEditing = editName === p.name
                  const isReregistering = reregisterName === p.name
                  return isEditing ? (
                    <tr key={p.name} className="bg-blue-50/50">
                      <td colSpan={6} className="px-4 py-4">
                        <h4 className="text-xs font-semibold text-blue-800 mb-2">Editing: {p.name}</h4>
                        <div className="grid grid-cols-2 gap-3 mb-3">
                          <Field label="Display Name" value={editForm.display_name} onChange={v => setEditForm({ ...editForm, display_name: v })} />
                          <Field label="Client ID" value={editForm.client_id} onChange={v => setEditForm({ ...editForm, client_id: v })} />
                          <Field label="Authorization URL" value={editForm.authorization_url} onChange={v => setEditForm({ ...editForm, authorization_url: v })} />
                          <Field label="Token URL" value={editForm.token_url} onChange={v => setEditForm({ ...editForm, token_url: v })} />
                          <Field label="Client Secret (leave empty to keep)" value={editForm.client_secret} onChange={v => setEditForm({ ...editForm, client_secret: v })} type="password" />
                          <Field label="Revocation URL" value={editForm.revocation_url} onChange={v => setEditForm({ ...editForm, revocation_url: v })} />
                          <Field label="Default Scopes" value={editForm.default_scopes} onChange={v => setEditForm({ ...editForm, default_scopes: v })} className="col-span-2" />
                        </div>
                        <div className="flex gap-2">
                          <button onClick={saveEdit} disabled={saving}
                            className="bg-blue-600 text-white px-4 py-1.5 rounded-full text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                            {saving ? 'Saving...' : 'Save'}
                          </button>
                          <button onClick={() => setEditName(null)}
                            className="rounded-full text-sm font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-1.5 transition-colors">
                            Cancel
                          </button>
                        </div>
                      </td>
                    </tr>
                  ) : isReregistering ? (
                    <tr key={p.name} className="bg-amber-50/50">
                      <td colSpan={6} className="px-4 py-4">
                        <h4 className="text-xs font-semibold text-amber-800 mb-1">Re-register: {p.display_name}</h4>
                        <p className="text-xs text-slate-500 mb-3">
                          Re-runs OAuth discovery and Dynamic Client Registration with the current server redirect URI.
                        </p>
                        <div className="flex gap-2 items-end">
                          <Field label="MCP Server URL" value={reregisterUrl} onChange={setReregisterUrl}
                            placeholder="https://mcp.example.com/apps/mcp" className="flex-1" />
                          <button onClick={reregisterMcp} disabled={reregistering || !reregisterUrl.trim()}
                            className="bg-amber-600 text-white px-4 py-1.5 rounded-full text-sm font-semibold hover:bg-amber-500 disabled:opacity-40 transition-colors whitespace-nowrap">
                            {reregistering ? 'Re-registering...' : 'Re-register'}
                          </button>
                          <button onClick={() => { setReregisterName(null); setReregisterUrl('') }}
                            className="rounded-full text-sm font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-1.5 transition-colors">
                            Cancel
                          </button>
                        </div>
                      </td>
                    </tr>
                  ) : (
                    <tr key={p.name} className={`hover:bg-slate-50 ${!p.is_active ? 'opacity-50' : ''}`}>
                      <td className="px-4 py-3">
                        <div className="font-medium text-slate-900">{p.display_name}</div>
                        <div className="text-xs text-slate-400">{p.name}</div>
                      </td>
                      <td className="px-4 py-3 text-slate-600 font-mono text-xs">{p.client_id.slice(0, 20)}...</td>
                      <td className="px-4 py-3 text-xs text-slate-500 max-w-48 truncate">{p.default_scopes}</td>
                      <td className="px-4 py-3">
                        <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ${
                          p.is_active
                            ? 'bg-green-50 text-green-700 ring-green-600/20'
                            : 'bg-slate-50 text-slate-500 ring-slate-300'
                        }`}>
                          {p.is_active ? 'Active' : 'Inactive'}
                        </span>
                      </td>
                      <td className="px-4 py-3 text-slate-600">{userCount}</td>
                      <td className="px-4 py-3 text-right space-x-2">
                        {isMcp && (
                          <button onClick={() => startReregister(p)} className="text-xs text-amber-600 hover:text-amber-800">Re-register</button>
                        )}
                        <button onClick={() => startEdit(p)} className="text-xs text-blue-600 hover:text-blue-800">Edit</button>
                        {p.is_active ? (
                          <button onClick={() => toggleActive(p.name, false)} className="text-xs text-amber-600 hover:text-amber-800">Deactivate</button>
                        ) : (
                          <button onClick={() => toggleActive(p.name, true)} className="text-xs text-green-600 hover:text-green-800">Activate</button>
                        )}
                        <button onClick={() => deleteProvider(p.name)} className="text-xs text-red-600 hover:text-red-800">Delete</button>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </>
      )}

      {/* User Connections tab */}
      {tab === 'connections' && (
        <>
          {available.length === 0 ? (
            <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-8 text-center">
              <p className="text-slate-400">No OAuth providers available. An admin needs to configure providers first.</p>
            </div>
          ) : (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
              {available.map(p => {
                const conn = myConnections.find(c => c.provider === p.name)
                const isExpired = conn?.token_expires_at
                  ? new Date(conn.token_expires_at) < new Date()
                  : false
                return (
                  <div key={p.name} className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-5 flex flex-col">
                    <div className="flex items-center gap-3 mb-3">
                      {p.icon_url ? (
                        <img src={p.icon_url} alt="" className="h-10 w-10 rounded-lg object-contain" />
                      ) : (
                        <div className="h-10 w-10 rounded-lg bg-slate-100 flex items-center justify-center">
                          <svg className="h-5 w-5 text-slate-400" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M13.19 8.688a4.5 4.5 0 0 1 1.242 7.244l-4.5 4.5a4.5 4.5 0 0 1-6.364-6.364l1.757-1.757m9.86-2.04a4.5 4.5 0 0 0-1.242-7.244l-4.5-4.5a4.5 4.5 0 0 0-6.364 6.364L4.34 8.798" />
                          </svg>
                        </div>
                      )}
                      <div className="flex-1 min-w-0">
                        <h3 className="font-medium text-slate-900 truncate">{p.display_name}</h3>
                        <p className="text-xs text-slate-400">{p.name}</p>
                      </div>
                    </div>

                    {conn ? (
                      <div className="mt-auto">
                        <div className="flex items-center gap-2 mb-3">
                          <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ${
                            isExpired
                              ? 'bg-red-50 text-red-700 ring-red-600/20'
                              : 'bg-green-50 text-green-700 ring-green-600/20'
                          }`}>
                            {isExpired ? 'Expired' : 'Connected'}
                          </span>
                          {conn.external_email && (
                            <span className="text-xs text-slate-500 truncate">{conn.external_email}</span>
                          )}
                        </div>
                        <div className="text-xs text-slate-400 mb-3">
                          Connected {new Date(conn.created_at).toLocaleDateString()}
                        </div>
                        <div className="flex gap-2">
                          {isExpired && (
                            <button onClick={() => startConnect(p.name)} disabled={connecting === p.name}
                              className="flex-1 bg-blue-600 text-white px-3 py-1.5 rounded-full text-xs font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                              {connecting === p.name ? 'Redirecting...' : 'Reconnect'}
                            </button>
                          )}
                          <button onClick={() => disconnectMe(p.name)}
                            className="flex-1 rounded-full text-xs font-medium text-red-600 ring-1 ring-red-200 hover:bg-red-50 px-3 py-1.5 transition-colors">
                            Disconnect
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div className="mt-auto">
                        <button onClick={() => startConnect(p.name)} disabled={connecting === p.name}
                          className="w-full bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 disabled:opacity-40 transition-colors">
                          {connecting === p.name ? 'Redirecting...' : 'Connect'}
                        </button>
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          )}

          {/* Admin: all connections table */}
          {connections.length > 0 && (
            <div className="mt-8">
              <h3 className="text-sm font-semibold text-slate-700 mb-3">All User Connections (Admin)</h3>
              <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
                <table className="w-full text-sm">
                  <thead className="bg-slate-50 text-xs uppercase text-slate-500">
                    <tr>
                      <th className="text-left px-4 py-3 font-medium">User</th>
                      <th className="text-left px-4 py-3 font-medium">Provider</th>
                      <th className="text-left px-4 py-3 font-medium">Email</th>
                      <th className="text-left px-4 py-3 font-medium">Scopes</th>
                      <th className="text-left px-4 py-3 font-medium">Expires</th>
                      <th className="text-left px-4 py-3 font-medium">Connected</th>
                      <th className="text-right px-4 py-3 font-medium">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100">
                    {connections.map(c => (
                      <tr key={c.id} className="hover:bg-slate-50">
                        <td className="px-4 py-3 text-slate-900 font-medium text-xs">{c.user_subject}</td>
                        <td className="px-4 py-3 text-slate-600">{c.provider}</td>
                        <td className="px-4 py-3 text-slate-500 text-xs">{c.external_email || '-'}</td>
                        <td className="px-4 py-3 text-xs text-slate-400 max-w-40 truncate">{c.granted_scopes}</td>
                        <td className="px-4 py-3 text-xs">
                          {c.token_expires_at ? (
                            <span className={new Date(c.token_expires_at) < new Date() ? 'text-red-600 font-medium' : 'text-slate-500'}>
                              {new Date(c.token_expires_at) < new Date() ? 'Expired' : new Date(c.token_expires_at).toLocaleDateString()}
                            </span>
                          ) : <span className="text-slate-400">-</span>}
                        </td>
                        <td className="px-4 py-3 text-xs text-slate-500">{new Date(c.created_at).toLocaleDateString()}</td>
                        <td className="px-4 py-3 text-right">
                          <button onClick={() => revokeConnection(c.user_subject, c.provider)}
                            className="text-xs text-red-600 hover:text-red-800">Revoke</button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}

// ── Reusable form field ─────────────────────────────────────────

function Field({ label, value, onChange, placeholder, type = 'text', className = '' }: {
  label: string; value: string; onChange: (v: string) => void
  placeholder?: string; type?: string; className?: string
}) {
  return (
    <div className={className}>
      <label className="block text-xs text-slate-500 mb-0.5">{label}</label>
      <input type={type} value={value} onChange={e => onChange(e.target.value)} placeholder={placeholder}
        className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-1.5 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
    </div>
  )
}
