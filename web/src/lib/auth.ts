import { create } from 'zustand'
import type { User, AuthTokens, AuthStatus } from '../types'

interface AuthState {
  accessToken: string | null
  refreshToken: string | null
  user: User | null
  isAuthenticated: boolean
  login: (email: string) => Promise<void>
  logout: () => Promise<void>
  refresh: () => Promise<void>
  loadStatus: () => Promise<void>
}

export const useAuthStore = create<AuthState>((set, get) => ({
  accessToken: localStorage.getItem('access_token'),
  refreshToken: localStorage.getItem('refresh_token'),
  user: null,
  isAuthenticated: !!localStorage.getItem('access_token'),

  login: async (email: string) => {
    const res = await fetch('/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email }),
    })
    if (!res.ok) {
      const err = await res.json()
      throw new Error(err.message || 'Login failed')
    }
    const tokens: AuthTokens = await res.json()
    localStorage.setItem('access_token', tokens.access_token)
    localStorage.setItem('refresh_token', tokens.refresh_token)
    set({
      accessToken: tokens.access_token,
      refreshToken: tokens.refresh_token,
      isAuthenticated: true,
    })
    await get().loadStatus()
  },

  logout: async () => {
    const token = get().refreshToken
    try {
      await fetch('/auth/logout', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ refresh_token: token }),
      })
    } catch {
      // Ignore logout errors
    }
    localStorage.removeItem('access_token')
    localStorage.removeItem('refresh_token')
    set({
      accessToken: null,
      refreshToken: null,
      user: null,
      isAuthenticated: false,
    })
  },

  refresh: async () => {
    const token = get().refreshToken
    if (!token) {
      throw new Error('No refresh token')
    }
    const res = await fetch('/auth/refresh', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: token }),
    })
    if (!res.ok) {
      await get().logout()
      throw new Error('Token refresh failed')
    }
    const tokens: AuthTokens = await res.json()
    localStorage.setItem('access_token', tokens.access_token)
    localStorage.setItem('refresh_token', tokens.refresh_token)
    set({
      accessToken: tokens.access_token,
      refreshToken: tokens.refresh_token,
      isAuthenticated: true,
    })
  },

  loadStatus: async () => {
    const token = get().accessToken
    if (!token) return
    const res = await fetch('/auth/status', {
      headers: { Authorization: `Bearer ${token}` },
    })
    if (!res.ok) {
      if (res.status === 401) {
        await get().logout()
      }
      return
    }
    const status: AuthStatus = await res.json()
    set({
      user: {
        subject: status.subject,
        tenant_id: status.tenant_id,
        role: status.role,
        scopes: status.scopes,
      },
    })
  },
}))
