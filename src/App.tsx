import { useEffect, useState } from 'react'
import { ArrowDownIcon, ArrowUpIcon, CopyIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select'
import { api, CARDS, svgUrl, type Card, type Profile } from '@/lib/api'
import { userManager } from '@/lib/auth'

const LABELS: Record<Card, string> = {
  github: 'GitHub stats',
  languages: 'Top languages',
  streak: 'Contribution streak',
  spotify: 'Spotify now playing',
}

type Item = { card: Card; on: boolean }

/** Saved cards first, in their order, then the rest switched off. */
const toItems = (cards: Card[]): Item[] => [
  ...cards.map((card) => ({ card, on: true })),
  ...CARDS.filter((c) => !cards.includes(c)).map((card) => ({ card, on: false })),
]

export default function App() {
  const [loaded, setLoaded] = useState(false)
  const [slug, setSlug] = useState('')
  const [githubLogin, setGithubLogin] = useState('')
  const [theme, setTheme] = useState<Profile['theme']>('light')
  const [columns, setColumns] = useState<1 | 2>(1)
  const [items, setItems] = useState<Item[]>(toItems(['github', 'languages', 'streak']))
  const [spotifyLinked, setSpotifyLinked] = useState(false)
  // slug + timestamp of the last save, so the preview reloads only for saved changes
  const [saved, setSaved] = useState<{ slug: string; at: number } | null>(null)
  const [error, setError] = useState('')
  // stats-service's Spotify callback returns here with ?spotify=linked|error
  const [notice, setNotice] = useState(() => {
    const spotify = new URLSearchParams(location.search).get('spotify')
    if (!spotify) return ''
    return spotify === 'linked' ? 'Spotify connected.' : 'Could not connect Spotify. Try again.'
  })
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (location.search) history.replaceState(null, '', '/')
    api<{ profile: Profile | null; spotifyLinked: boolean }>('/me')
      .then(({ profile, spotifyLinked }) => {
        setSpotifyLinked(spotifyLinked)
        if (profile) {
          setSlug(profile.slug)
          setGithubLogin(profile.githubLogin)
          setTheme(profile.theme)
          setColumns(profile.layout.columns)
          setItems(toItems(profile.layout.cards))
          setSaved({ slug: profile.slug, at: Date.now() })
        }
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoaded(true))
  }, [])

  const move = (i: number, by: -1 | 1) =>
    setItems((xs) => {
      const next = [...xs]
      ;[next[i], next[i + by]] = [next[i + by], next[i]]
      return next
    })

  async function run(fn: () => Promise<void>) {
    setBusy(true)
    setError('')
    setNotice('')
    try {
      await fn()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const save = () =>
    run(async () => {
      const body: Profile = {
        slug,
        githubLogin,
        theme,
        layout: { columns, cards: items.filter((x) => x.on).map((x) => x.card) },
      }
      const p = await api<Profile>('/me', { method: 'PUT', body: JSON.stringify(body) })
      setSlug(p.slug)
      setSaved({ slug: p.slug, at: Date.now() })
      setNotice('Saved.')
    })

  const linkSpotify = () =>
    run(async () => {
      const { url } = await api<{ url: string }>('/spotify/link', { method: 'POST' })
      location.href = url
    })

  const unlinkSpotify = () =>
    run(async () => {
      await api('/spotify', { method: 'DELETE' })
      setSpotifyLinked(false)
    })

  const snippet = saved ? `![My stats](${svgUrl(saved.slug)})` : ''

  if (!loaded) return <main className="p-8 text-sm text-muted-foreground">Loading…</main>

  return (
    <main className="mx-auto grid max-w-3xl gap-8 p-4 sm:p-8">
      <header className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">README stats</h1>
          <p className="text-sm text-muted-foreground">Pick and order your cards, then paste one link in your README.</p>
        </div>
        <Button variant="ghost" onClick={() => userManager.signoutRedirect()}>
          Sign out
        </Button>
      </header>

      <form
        className="grid gap-6"
        onSubmit={(e) => {
          e.preventDefault()
          save()
        }}
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="grid gap-2">
            <Label htmlFor="slug">Page name</Label>
            <Input id="slug" value={slug} onChange={(e) => setSlug(e.target.value)} placeholder="your-name" required />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="github">GitHub username</Label>
            <Input id="github" value={githubLogin} onChange={(e) => setGithubLogin(e.target.value)} placeholder="octocat" />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="theme">Theme</Label>
            <NativeSelect id="theme" value={theme} onChange={(e) => setTheme(e.target.value as Profile['theme'])}>
              <NativeSelectOption value="light">Light</NativeSelectOption>
              <NativeSelectOption value="dark">Dark</NativeSelectOption>
            </NativeSelect>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="columns">Columns</Label>
            <NativeSelect id="columns" value={columns} onChange={(e) => setColumns(Number(e.target.value) as 1 | 2)}>
              <NativeSelectOption value={1}>1</NativeSelectOption>
              <NativeSelectOption value={2}>2</NativeSelectOption>
            </NativeSelect>
          </div>
        </div>

        <fieldset className="grid gap-2">
          <legend className="mb-2 text-sm font-medium">Cards</legend>
          <ul className="grid gap-2">
            {items.map((x, i) => (
              <li key={x.card} className="flex items-center gap-3 rounded-lg border px-3 py-2">
                <Checkbox
                  id={`card-${x.card}`}
                  checked={x.on}
                  onCheckedChange={(on) => setItems((xs) => xs.map((y) => (y.card === x.card ? { ...y, on } : y)))}
                />
                <Label htmlFor={`card-${x.card}`} className="flex-1">
                  {LABELS[x.card]}
                </Label>
                <Button type="button" size="icon" variant="ghost" aria-label={`Move ${LABELS[x.card]} up`} disabled={i === 0} onClick={() => move(i, -1)}>
                  <ArrowUpIcon />
                </Button>
                <Button type="button" size="icon" variant="ghost" aria-label={`Move ${LABELS[x.card]} down`} disabled={i === items.length - 1} onClick={() => move(i, 1)}>
                  <ArrowDownIcon />
                </Button>
              </li>
            ))}
          </ul>
        </fieldset>

        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit" disabled={busy}>
            Save
          </Button>
          {spotifyLinked ? (
            <Button type="button" variant="outline" disabled={busy} onClick={unlinkSpotify}>
              Disconnect Spotify
            </Button>
          ) : (
            <Button type="button" variant="outline" disabled={busy} onClick={linkSpotify}>
              Connect Spotify
            </Button>
          )}
          <p role="status" className="text-sm text-muted-foreground">
            {notice}
          </p>
        </div>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
      </form>

      {saved && (
        <section className="grid gap-3">
          <h2 className="text-lg font-semibold">Your README link</h2>
          <div className="flex gap-2">
            <Input readOnly value={snippet} aria-label="Markdown snippet" onFocus={(e) => e.target.select()} />
            <Button type="button" variant="outline" onClick={() => navigator.clipboard.writeText(snippet).then(() => setNotice('Copied.'))}>
              <CopyIcon /> Copy
            </Button>
          </div>
          <img src={`${svgUrl(saved.slug)}?v=${saved.at}`} alt="Preview of your stats cards" className="max-w-full" />
        </section>
      )}
    </main>
  )
}
