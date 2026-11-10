// The outline of the logo's shell, redrawn with even turns so it can animate as
// a loader. The line starts at the center, so growing and then erasing it from
// the center end draws the shell and lets the center catch up.
const OUTLINE =
  "M51.7 81.4 L51.1 76.1 L53.4 71.6 L57.5 68.3 L62.7 66.9 L68.3 67.3 L73.9 69.2 L78.9 72.9 L82.8 78.2 L85.0 84.9 L84.7 92.3 L81.8 99.6 L76.2 105.3 L69.0 108.6 L61.2 109.7 L52.3 110.5 L43.1 107.9 L35.1 101.9 L29.3 93.1 L26.8 82.6 L27.8 71.5 L32.4 61.0 L40.4 52.1 L50.9 46.0 L63.2 43.3 L76.1 44.5 L88.5 49.7 L99.0 58.8 L106.4 71.1 L109.8 85.6 L108.4 100.9 L102.0 115.4 L91.2 127.5 L76.7 135.6 L60.0 138.8 L42.9 136.4 L26.9 129.2 L13.1 117.4 L2.9 101.6 L-2.2 82.6 L-1.1 61.9 L6.8 41.7 L21.4 24.3 L41.6 12.0 L65.4 6.6 L90.5 9.3 L113.9 20.2 L113.9 20.2";
export function Spiral() {
  return (
    <svg className="spiral" viewBox="-18.3 -1.4 148.2 148.2" aria-hidden="true">
      <path pathLength="100" strokeWidth={12.2} d={OUTLINE} />
    </svg>
  );
}
