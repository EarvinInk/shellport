// The site can live under a sub-path (GitHub Pages serves it at /shellport/).
// Write internal links as "/docs/…" and pass them through `u()`.
const base = import.meta.env.BASE_URL.replace(/\/$/, '');

/** "/docs/" → "/shellport/docs/" (unchanged when the site sits at the root). */
export const u = (path: string) => (path.startsWith('/') && !path.startsWith('//') ? base + path : path);
