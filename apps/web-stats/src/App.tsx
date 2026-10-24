import { useEffect, useState, type ReactNode } from 'react'
import { closestCenter, DndContext, KeyboardSensor, PointerSensor, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core'
import { arrayMove, rectSortingStrategy, SortableContext, sortableKeyboardCoordinates, useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { CopyIcon, GripVerticalIcon, PlusIcon, RectangleHorizontalIcon, Trash2Icon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { api, CARDS, MAX_CUSTOM, svgUrl, type Card, type Custom, type Entry, type Profile } from '@/lib/api'
import { userManager } from '@/lib/auth'
import { cn } from '@/lib/utils'

const LABELS: Record<Card, string> = {
  github: 'GitHub stats',
  languages: 'Top languages',
  streak: 'Contribution streak',
  spotify: 'Spotify now playing',
}

// `key` is only a React key: the card name, or a random id for custom cards (always on).
// `wide`: spans both columns when the layout has 2.
type Item = { key: string; on: boolean; wide: boolean; entry: Entry }

const custom = (entry: Custom, wide = false): Item => ({ key: crypto.randomUUID(), on: true, wide, entry })

/** Saved cards first, in their order, then the unused built-ins switched off. */
const toItems = (cards: Entry[], wide: boolean[] = []): Item[] => [
  ...cards.map((entry, i) => (typeof entry === 'string' ? { key: entry, on: true, wide: !!wide[i], entry } : custom(entry, !!wide[i]))),
  ...CARDS.filter((c) => !cards.includes(c)).map((card) => ({ key: card, on: false, wide: false, entry: card })),
]

/** A grid cell dragged only by the handle it passes to `children`, so its inputs stay typeable. */
function SortableCard({ id, label, className, children }: { id: string; label: string; className: string; children: (handle: ReactNode) => ReactNode }) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({ id })
  return (
    <li ref={setNodeRef} style={{ transform: CSS.Translate.toString(transform), transition }} className={cn(className, 'bg-background', isDragging && 'z-10 shadow-lg')}>
      {children(
        <Button type="button" size="icon" variant="ghost" ref={setActivatorNodeRef} className="cursor-grab touch-none" {...attributes} {...listeners} aria-label={`Drag ${label}`}>
          <GripVerticalIcon />
        </Button>,
      )}
    </li>
  )
}

const TEXTAREA =
  'w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 py-1 text-base transition-colors outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 md:text-sm dark:bg-input/30'

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
          setItems(toItems(profile.layout.cards, profile.layout.wide))
          setSaved({ slug: profile.slug, at: Date.now() })
        }
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoaded(true))
  }, [])

  const update = (key: string, patch: Partial<Item>) => setItems((xs) => xs.map((y) => (y.key === key ? { ...y, ...patch } : y)))
  const customCount = items.filter((x) => typeof x.entry !== 'string').length

  const sensors = useSensors(useSensor(PointerSensor), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }))
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (over && active.id !== over.id)
      setItems((xs) => arrayMove(xs, xs.findIndex((x) => x.key === active.id), xs.findIndex((x) => x.key === over.id)))
  }
  // Only switched-on cards sit in the grid, so its gaps match the rendered SVG.
  const shown = items.filter((x) => x.on)
  // Switching a card on puts it at the end of the grid.
  const turnOn = (key: string) => setItems((xs) => [...xs.filter((y) => y.key !== key), { ...xs.find((y) => y.key === key)!, on: true }])

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
        layout: { columns, cards: shown.map((x) => x.entry), wide: shown.map((x) => x.wide) },
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

      {(notice || error) && (
        <div className="grid gap-1">
          <p role="status" className="text-sm text-muted-foreground">
            {notice}
          </p>
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
        </div>
      )}

      <Tabs defaultValue="editor">
        <TabsList>
          <TabsTrigger value="editor">Editor</TabsTrigger>
          <TabsTrigger value="preview">Preview</TabsTrigger>
        </TabsList>

        <TabsContent value="editor">
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
              <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
                <SortableContext items={shown.map((x) => x.key)} strategy={rectSortingStrategy}>
                  <ul className={cn('grid gap-2', columns === 2 && 'sm:grid-cols-2')}>
                    {shown.map((x) => {
                      const name = typeof x.entry === 'string' ? LABELS[x.entry] : x.entry.title || 'custom card'
                      const widen = columns === 2 && (
                        <Button
                          type="button"
                          size="icon"
                          variant={x.wide ? 'secondary' : 'ghost'}
                          className="max-sm:hidden"
                          aria-label={`Span ${name} across both columns`}
                          aria-pressed={x.wide}
                          onClick={() => update(x.key, { wide: !x.wide })}
                        >
                          <RectangleHorizontalIcon />
                        </Button>
                      )
                      const cell = cn('flex gap-2 rounded-lg border px-3 py-2', columns === 2 && x.wide && 'sm:col-span-2')
                      if (typeof x.entry === 'string')
                        return (
                          <SortableCard key={x.key} id={x.key} label={name} className={cn(cell, 'items-center')}>
                            {(handle) => (
                              <>
                                {handle}
                                <Checkbox id={`card-${x.key}`} checked onCheckedChange={() => update(x.key, { on: false })} />
                                <Label htmlFor={`card-${x.key}`} className="flex-1">
                                  {name}
                                </Label>
                                {widen}
                              </>
                            )}
                          </SortableCard>
                        )
                      const c = x.entry
                      return (
                        <SortableCard key={x.key} id={x.key} label={name} className={cn(cell, 'items-start')}>
                          {(handle) => (
                            <>
                              {handle}
                              <div className="grid min-w-0 flex-1 gap-2">
                                <Input
                                  aria-label="Card title"
                                  placeholder="Title"
                                  maxLength={38}
                                  required
                                  value={c.title}
                                  onChange={(e) => update(x.key, { entry: { ...c, title: e.target.value } })}
                                />
                                <textarea
                                  aria-label={`Text for ${name}`}
                                  placeholder="Text"
                                  maxLength={280}
                                  rows={3}
                                  className={TEXTAREA}
                                  value={c.text}
                                  onChange={(e) => update(x.key, { entry: { ...c, text: e.target.value } })}
                                />
                              </div>
                              <div className="grid">
                                {widen}
                                <Button type="button" size="icon" variant="ghost" aria-label={`Delete ${name}`} onClick={() => setItems((xs) => xs.filter((y) => y.key !== x.key))}>
                                  <Trash2Icon />
                                </Button>
                              </div>
                            </>
                          )}
                        </SortableCard>
                      )
                    })}
                  </ul>
                </SortableContext>
              </DndContext>
              {items.some((x) => !x.on) && (
                <ul className="flex flex-wrap gap-2" aria-label="Cards not shown">
                  {items.map(
                    (x) =>
                      !x.on &&
                      typeof x.entry === 'string' && (
                        <li key={x.key} className="flex items-center gap-2 rounded-lg border border-dashed px-3 py-2 text-muted-foreground">
                          <Checkbox id={`card-${x.key}`} checked={false} onCheckedChange={() => turnOn(x.key)} />
                          <Label htmlFor={`card-${x.key}`}>{LABELS[x.entry]}</Label>
                        </li>
                      ),
                  )}
                </ul>
              )}
              <Button
                type="button"
                variant="outline"
                className="justify-self-start"
                disabled={customCount >= MAX_CUSTOM}
                onClick={() => setItems((xs) => [...xs, custom({ title: '', text: '' })])}
              >
                <PlusIcon /> Add custom card
              </Button>
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
            </div>
          </form>
        </TabsContent>

        <TabsContent value="preview">
          {saved ? (
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
          ) : (
            <p className="text-sm text-muted-foreground">Save your cards to see a preview.</p>
          )}
        </TabsContent>
      </Tabs>
    </main>
  )
}
