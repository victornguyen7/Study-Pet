import { useEffect, useState } from 'react'

// Geometry on a 1024 canvas, sampled onto a 32×32 pixel grid
const BODY = { cx: 512, cy: 610, rx: 390, ry: 270, n: 2.6, floor: 790 }
const EAR: [number, number][] = [
  [196, 520],
  [232, 352],
  [350, 430],
]
const CLOVER = { cx: 736, cy: 382, r: 46, off: 44 }
const CORE = { cx: 580, cy: 690, rx: 230, ry: 82 }
const SHINE = { cx: 520, cy: 430, rx: 130, ry: 40 }

const C = {
  bg: '#0E1630',
  tile: '#16224A',
  outline: '#0A1230',
  body: '#5B8FE8',
  bodyLight: '#8DB6F7',
  core: '#2C4FA8',
  shine: '#E6F1FF',
  line: '#1B2A66',
  clover: '#CFE3FF',
}

function inBody(x: number, y: number) {
  if (y > BODY.floor) return false
  const dx = Math.abs(x - BODY.cx) / BODY.rx
  const dy = Math.abs(y - BODY.cy) / BODY.ry
  return dx ** BODY.n + dy ** BODY.n <= 1
}
function inTri(x: number, y: number) {
  const [a, b, c] = EAR
  const s = (p: number[], q: number[]) => (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0])
  const d1 = s(a, b), d2 = s(b, c), d3 = s(c, a)
  return !((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0))
}
const inEll = (x: number, y: number, e: { cx: number; cy: number; rx: number; ry: number }) =>
  ((x - e.cx) / e.rx) ** 2 + ((y - e.cy) / e.ry) ** 2 <= 1
const cloverCenters = [
  [0, -1],
  [1, 0],
  [0, 1],
  [-1, 0],
].map(([a, b]) => [CLOVER.cx + a * CLOVER.off, CLOVER.cy + b * CLOVER.off])
const inClover = (x: number, y: number) =>
  cloverCenters.some(([cx, cy]) => (x - cx) ** 2 + (y - cy) ** 2 <= CLOVER.r ** 2)
const inSlime = (x: number, y: number) => inBody(x, y) || inTri(x, y)

const N = 32
const S = 1024 / N
function PixelLogo({ eyes: eyeState = 'closed' }: { eyes?: 'open' | 'closed' }) {
  const cells: { x: number; y: number; f: string }[] = []
  const at = (i: number, j: number) => [i * S + S / 2, j * S + S / 2] as const
  const solid = (i: number, j: number) => {
    const [x, y] = at(i, j)
    return inSlime(x, y) || inClover(x, y)
  }
  const closedEyes = new Set(['10,17', '11,18', '12,18', '13,17', '18,17', '19,18', '20,18', '21,17'])
  const openEyes = new Set(['11,16', '12,16', '11,17', '12,17', '11,18', '12,18', '19,16', '20,16', '19,17', '20,17', '19,18', '20,18'])
  const glints = new Set(['11,16', '19,16'])
  const eyes = eyeState === 'open' ? openEyes : closedEyes
  for (let j = 0; j < N; j++)
    for (let i = 0; i < N; i++) {
      const [x, y] = at(i, j)
      if (!solid(i, j)) continue
      const edge = !solid(i - 1, j) || !solid(i + 1, j) || !solid(i, j - 1) || !solid(i, j + 1)
      let f = C.body
      if (edge) f = C.outline
      else if (inClover(x, y)) f = C.clover
      else if (eyeState === 'open' && glints.has(`${i},${j}`)) f = C.shine
      else if (eyes.has(`${i},${j}`)) f = C.line
      else if (inEll(x, y, SHINE)) f = C.shine
      else if (inEll(x, y, CORE)) f = C.core
      else if (y < BODY.cy - 60 && x < BODY.cx) f = C.bodyLight
      cells.push({ x: i * S, y: j * S, f })
    }
  return (
    <svg viewBox="0 0 1024 1024" className="w-full h-auto" shapeRendering="crispEdges" role="img" aria-label={`Study Pet, eyes ${eyeState}`}>
      {cells.map((c, k) => (
        <rect key={k} x={c.x} y={c.y} width={S} height={S} fill={c.f} />
      ))}
    </svg>
  )
}

// Splash: the slime naps, then wakes up when the app opens
function Splash({ onDone }: { onDone: () => void }) {
  const [eyes, setEyes] = useState<'open' | 'closed'>('closed')
  const [leaving, setLeaving] = useState(false)
  useEffect(() => {
    const t = [
      setTimeout(() => setEyes('open'), 1100),
      setTimeout(() => setEyes('closed'), 1500),
      setTimeout(() => setEyes('open'), 1650),
      setTimeout(() => setLeaving(true), 2600),
      setTimeout(onDone, 3000),
    ]
    return () => t.forEach(clearTimeout)
  }, [onDone])
  return (
    <div
      className={`fixed inset-0 z-50 flex flex-col items-center justify-center gap-6 bg-[#0E1630] transition-opacity duration-400 ${leaving ? 'opacity-0' : 'opacity-100'}`}
    >
      <div className="w-[min(60vmin,360px)] origin-bottom animate-[squish_1.2s_steps(4)_infinite]">
        <PixelLogo eyes={eyes} />
      </div>
      <p className="font-mono text-sm tracking-[0.4em] uppercase text-[#8DB6F7]">Study Pet</p>
    </div>
  )
}

export default function App() {
  const [splash, setSplash] = useState(true)
  return (
    <div className="min-h-screen bg-[#0E1630] flex flex-wrap items-center justify-center gap-12 p-10">
      {splash && <Splash onDone={() => setSplash(false)} />}
      {(['closed', 'open'] as const).map((e) => (
        <figure key={e} className="w-[min(80vw,360px)] flex flex-col items-center gap-4">
          <div className="w-full checker">
            <PixelLogo eyes={e} />
          </div>
          <figcaption className="font-mono text-sm tracking-[0.3em] uppercase text-[#8DB6F7]">Eyes {e}</figcaption>
        </figure>
      ))}
    </div>
  )
}
