// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import { site } from './src/site.config.ts';

// GitHub Pages serves the site under /shellport/; the address in site.url
// says where. Internal links are written from the root ("/docs/…") and get
// this prefix: in .astro files through `u()` (src/lib/url.ts), in the docs'
// Markdown through the small rehype plugin below.
const base = new URL(site.url).pathname.replace(/\/$/, '') || '/';

/** Prefix root-relative links in Markdown with the base path. */
function rehypeBaseLinks() {
  const prefix = base === '/' ? '' : base;
  /** @param {any} node */
  const walk = (node) => {
    const href = node.properties?.href;
    if (node.tagName === 'a' && typeof href === 'string' && href.startsWith('/') && !href.startsWith('//')) {
      node.properties.href = prefix + href;
    }
    node.children?.forEach(walk);
  };
  return walk;
}

export default defineConfig({
  site: site.url,
  base,
  integrations: [sitemap()],
  markdown: {
    shikiConfig: { theme: 'github-dark-dimmed', wrap: false },
    rehypePlugins: [rehypeBaseLinks],
  },
});
