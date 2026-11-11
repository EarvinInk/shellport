// Everything personal or deployment-specific lives here. Leave a value
// empty ('') to hide what uses it: no email hides the email links, no donate
// links hide the Donate page, an empty otherProjects hides that footer column.

const repo = 'https://github.com/EarvinInk/shellport';

export const site = {
  name: 'Shellport',
  tagline: 'SSH, SFTP and Docker in one native Linux app.',
  url: 'https://earvinink.github.io/shellport',
  version: '0.2.1',
  releaseDate: '2026-11-11',

  author: {
    name: 'EarvinInk',
    url: 'https://github.com/EarvinInk',
    domainLabel: 'github.com/EarvinInk',
    email: '',
  },

  repo,
  releases: `${repo}/releases/latest`,
  links: {
    issues: `${repo}/issues`,
    newBug: `${repo}/issues/new?template=bug_report.yml`,
    newFeature: `${repo}/issues/new?template=feature_request.yml`,
    discussions: '', // turn on Discussions in the repo, then use `${repo}/discussions`
    security: `${repo}/security/advisories/new`,
    changelog: `${repo}/blob/main/CHANGELOG.md`,
    license: `${repo}/blob/main/LICENSE`,
  },

  downloads: {
    deb: { file: 'Shellport_0.2.1_amd64.deb', size: '~9 MB' },
    appimage: { file: 'Shellport_0.2.1_amd64.AppImage', size: '~85 MB' },
  },

  donate: {
    githubSponsors: '',
    kofi: '',
    buyMeACoffee: '',
  },

  // Shown in the footer under "More from …", e.g.
  // { name: 'Project One', description: 'A short one-line description.', url: 'https://one.example.com' },
  otherProjects: [] as { name: string; description: string; url: string }[],
} as const;

/** Show donate links only once at least one is set. */
export const hasDonate = Object.values(site.donate).some(Boolean);
