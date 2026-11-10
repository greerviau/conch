// Builds assets/logo.svg from a logarithmic spiral ribbon. Run via prepare-brand.mjs.
const TAU = Math.PI * 2;

export const params = {
  growth: 2.0, // radius multiplier per full turn
  band: 0.42, // ribbon width as a fraction of local radius
  turns: 2.3, // length of the ribbon in turns
  flareStart: 0.76, // fraction of the ribbon where the lip starts to open
  flare: 0.7, // extra outer-edge radius at the tip, as a fraction of radius
  startAngle: 200, // degrees, rotation of the whole mark
  waves: [] // optional sound arcs at the mouth: {radius, span, width}
};

const f = (n) => (Math.round(n * 100) / 100).toString();

function edges(p) {
  const total = p.turns * TAU;
  const rot = (p.startAngle * Math.PI) / 180;
  const k = Math.log(p.growth) / TAU;
  const step = (TAU / 360) * 6;
  const outer = [];
  const inner = [];
  for (let t = 0; t <= total + 1e-9; t += step) {
    const r = Math.exp(k * t);
    const u = Math.max(0, (t / total - p.flareStart) / (1 - p.flareStart));
    const lip = p.flare * u * u * (3 - 2 * u);
    const a = t + rot;
    outer.push([r * (1 + lip) * Math.cos(a), r * (1 + lip) * Math.sin(a)]);
    inner.push([r * (1 - p.band) * Math.cos(a), r * (1 - p.band) * Math.sin(a)]);
  }
  return { outer, inner };
}

// Catmull-Rom spline through points, emitted as cubic segments.
function spline(points) {
  let d = '';
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[Math.max(0, i - 1)];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[Math.min(points.length - 1, i + 2)];
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${f(c1[0])} ${f(c1[1])} ${f(c2[0])} ${f(c2[1])} ${f(p2[0])} ${f(p2[1])}`;
  }
  return d;
}

function buildMark(p, box = 128, margin = 10) {
  const { outer, inner } = edges(p);
  const last = outer.length - 1;
  const tipO = outer[last];
  const tipI = inner[last];
  const tip = [(tipO[0] + tipI[0]) / 2, (tipO[1] + tipI[1]) / 2];
  const tipR = Math.hypot(tipO[0] - tipI[0], tipO[1] - tipI[1]) / 2;
  const heading = Math.atan2(tipO[1] - outer[last - 1][1], tipO[0] - outer[last - 1][0]);
  const waves = p.waves.map((w) => {
    const r = tipR * w.radius;
    const at = (a) => [tip[0] + r * Math.cos(a), tip[1] + r * Math.sin(a)];
    return { r, width: w.width * tipR, ends: [at(heading - w.span), at(heading + w.span)], mid: at(heading) };
  });
  const all = [...outer, ...inner, ...waves.flatMap((w) => [...w.ends, w.mid])];
  const xs = all.map((q) => q[0]);
  const ys = all.map((q) => q[1]);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);
  const scale = (box - 2 * margin) / Math.max(maxX - minX, maxY - minY);
  const ox = box / 2 - ((minX + maxX) / 2) * scale;
  const oy = box / 2 - ((minY + maxY) / 2) * scale;
  const place = (q) => [q[0] * scale + ox, q[1] * scale + oy];
  const O = outer.map(place);
  const I = inner.map(place).reverse();
  const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
  const capEnd = dist(O[O.length - 1], I[0]) / 2;
  const capStart = dist(I[I.length - 1], O[0]) / 2;
  const d =
    `M${f(O[0][0])} ${f(O[0][1])}` +
    spline(O) +
    `A${f(capEnd)} ${f(capEnd)} 0 0 1 ${f(I[0][0])} ${f(I[0][1])}` +
    spline(I) +
    `A${f(capStart)} ${f(capStart)} 0 0 1 ${f(O[0][0])} ${f(O[0][1])}Z`;
  const arcs = waves.map((w) => {
    const [a, b] = w.ends.map(place);
    const r = w.r * scale;
    return { d: `M${f(a[0])} ${f(a[1])}A${f(r)} ${f(r)} 0 0 1 ${f(b[0])} ${f(b[1])}`, width: w.width * scale };
  });
  return { d, arcs };
}

export function buildLogo(p = params) {
  const { d, arcs } = buildMark(p);
  const waves = arcs
    .map((a) => `\n  <path d="${a.d}" fill="none" stroke="#f0906c" stroke-width="${f(a.width)}" stroke-linecap="round"/>`)
    .join('');
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" role="img" aria-labelledby="title">
  <title id="title">Conch</title>
  <path d="${d}" fill="#f4ecd8"/>${waves}
</svg>
`;
}
