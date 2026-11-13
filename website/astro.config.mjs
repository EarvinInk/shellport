// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import { site, hasDonate } from './src/site.config.ts';

// GitHub Pages serves the site under /shellport/; the address in site.url
// says where. Internal links in .astro files go through `u()`
// (src/lib/url.ts); links in the Markdown docs are relative (e.g.
// "../install/"), so they work under any base path.
const base = new URL(site.url).pathname.replace(/\/$/, '') || '/';

export default defineConfig({
  site: site.url,
  base,
  // The Donate page only redirects home while no donate links are set; keep it out of the sitemap.
  integrations: [sitemap({ filter: (page) => hasDonate || !page.includes('/donate/') })],
  markdown: {
    shikiConfig: { theme: 'github-dark-dimmed', wrap: false },
  },
});
