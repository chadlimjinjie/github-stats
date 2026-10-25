import { accessToken, GATEWAY_URL } from './auth'

export const CARDS = ['github', 'languages', 'streak', 'spotify'] as const
export type Card = (typeof CARDS)[number]
/** A user-written card; limits match stats-service's `validate`. */
export type Custom = { title: string; text: string }
/** Pictures as JPEG `data:` URIs (GitHub strips external images in SVGs); limits match `validate`. */
export type Gallery = { title: string; images: string[] }
export type Entry = Card | Custom | Gallery
/** Custom cards and galleries together. */
export const MAX_CUSTOM = 5
export const MAX_IMAGES = 6
const MAX_IMAGE = 40_000

/** Center-crops `file` to a 240px square JPEG `data:` URI under stats-service's size cap. */
export async function shrink(file: File): Promise<string> {
  const img = await createImageBitmap(file)
  const side = Math.min(img.width, img.height)
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = 240
  canvas.getContext('2d')!.drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, 240, 240)
  img.close()
  for (const q of [0.8, 0.6, 0.4]) {
    const uri = canvas.toDataURL('image/jpeg', q)
    if (uri.length <= MAX_IMAGE) return uri
  }
  throw new Error(`${file.name} is too detailed to fit; try another picture`)
}

export type Profile = {
  slug: string
  githubLogin: string
  // `wide[i]`: card i spans both columns; `borderless[i]`: card i has no outline.
  // Both are missing on profiles saved before they existed.
  layout: { cards: Entry[]; columns: 1 | 2; wide?: boolean[]; borderless?: boolean[] }
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
