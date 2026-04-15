export interface User {
  subject: string
  tenant_id: string
  role: string
  scopes: string[]
}

export interface Tenant {
  id: string
  slug: string
  display_name: string
  status: string
}

export interface Agent {
  id: string
  name: string
  display_name: string
  description: string | null
  model_deployment: string
  version: number
  tenant_id: string
  status: string
  created_at: string
  updated_at: string
}

export interface Deployment {
  id: string
  slug: string
  name: string
  model_id: string
  tenant_id: string
  is_active: boolean
  default_params: Record<string, unknown>
  created_at: string
  updated_at: string
}

export interface ApiKey {
  id: string
  name: string
  prefix: string
  scopes: string[]
  tenant_id: string
  created_by: string
  expires_at: string | null
  created_at: string
  last_used_at: string | null
}

export interface Model {
  id: string
  name: string
  display_name: string
  provider: string
  context_window: number
  max_output_tokens: number
  embedding_dimensions: number | null
  cap_chat: boolean
  cap_embedding: boolean
  cap_thinking: boolean
  cap_vision: boolean
  cap_tool_use: boolean
  cap_json_output: boolean
  cap_audio_in: boolean
  cap_audio_out: boolean
  cap_image_gen: boolean
  cost_per_1k_input: number
  cost_per_1k_output: number
  cost_per_1k_cache_read: number
  cost_per_1k_cache_write: number
  published: boolean
}

export interface KnowledgeDocument {
  id: string
  agent_id: string
  filename: string
  content_type: string
  size_bytes: number
  chunk_count: number
  status: string
  created_at: string
}

export interface MemoryEntry {
  id: string
  scope: string
  scope_id: string
  key: string
  value: string
  created_at: string
  updated_at: string
}

export interface Conversation {
  id: string
  agent_id: string
  user_id: string
  title: string | null
  status: string
  created_at: string
  updated_at: string
}

export interface UsageRecord {
  id: string
  tenant_id: string
  model_id: string
  deployment_id: string | null
  input_tokens: number
  output_tokens: number
  cache_read_tokens: number
  cache_write_tokens: number
  cost: number
  created_at: string
}

export interface AuditEntry {
  id: string
  tenant_id: string
  actor: string
  action: string
  resource_type: string
  resource_id: string
  detail: Record<string, unknown>
  created_at: string
}

export interface Pagination {
  page: number
  per_page: number
  total: number
}

export interface PaginatedResponse<T> {
  data: T[]
  pagination: Pagination
}

export interface AuthTokens {
  access_token: string
  refresh_token: string
}

export interface AuthStatus {
  subject: string
  tenant_id: string
  role: string
  scopes: string[]
}
