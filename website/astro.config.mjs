// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import { site } from './src/site.config.ts';

// GitHub Pages serves the site under /shellport/; the address in site.url
// says where. Internal links in .astro files go through `u()`
// (src/lib/url.ts); links in the Markdown docs are relative (e.g.
// "../install/"), so they work under any base path.
const base = new URL(site.url).pathname.replace(/\/$/, '') || '/';

export default defineConfig({
  site: site.url,
  base,
  integrations: [sitemap()],
  markdown: {
    shikiConfig: { theme: 'github-dark-dimmed', wrap: false },
  },
});
