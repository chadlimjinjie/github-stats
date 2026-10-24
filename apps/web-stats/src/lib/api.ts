import { accessToken, GATEWAY_URL } from './auth'

export const CARDS = ['github', 'languages', 'streak', 'spotify'] as const
export type Card = (typeof CARDS)[number]
/** A user-written card; limits match stats-service's `validate`. */
export type Custom = { title: string; text: string }
export type Entry = Card | Custom
export const MAX_CUSTOM = 5

export type Profile = {
  slug: string
  githubLogin: string
  layout: { cards: Entry[]; columns: 1 | 2 }
  theme: 'light' | 'dark'
}

export const svgUrl = (slug: string) => `${GATEWAY_URL}/stats/u/${slug}.svg`

/** Authed JSON call to stats-service; throws the service's `error` message on failure. */
export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(`${GATEWAY_URL}/stats${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${await accessToken()}`, 'Content-Type': 'application/json' },
  })
  if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? `Request failed (${res.status})`)
  return (res.status === 204 ? undefined : await res.json()) as T
}
