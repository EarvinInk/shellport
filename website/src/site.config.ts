// Everything personal or deployment-specific lives here.
// Replace the placeholder values before publishing.

const repo = 'https://github.com/EarvinInk/shellport';

export const site = {
  name: 'Shellport',
  tagline: 'SSH, SFTP and Docker in one native Linux app.',
  url: 'https://shellport.example.com',
  version: '0.2.1',
  releaseDate: '2026-11-11',

  author: {
    name: 'Your Name',
    url: 'https://example.com',
    domainLabel: 'example.com',
    email: 'hello@example.com',
  },

  repo,
  releases: `${repo}/releases/latest`,
  links: {
    issues: `${repo}/issues`,
    newBug: `${repo}/issues/new?template=bug_report.yml`,
    newFeature: `${repo}/issues/new?template=feature_request.yml`,
    discussions: `${repo}/discussions`,
    security: `${repo}/security/advisories/new`,
    changelog: `${repo}/blob/main/CHANGELOG.md`,
    license: `${repo}/blob/main/LICENSE`,
  },

  downloads: {
    deb: { file: 'Shellport_0.2.1_amd64.deb', size: '~9 MB' },
    appimage: { file: 'Shellport_0.2.1_amd64.AppImage', size: '~85 MB' },
  },

  donate: {
    githubSponsors: 'https://github.com/sponsors/your-handle',
    kofi: 'https://ko-fi.com/your-handle',
    buyMeACoffee: 'https://buymeacoffee.com/your-handle',
  },

  // Shown in the footer under "More from …"
  otherProjects: [
    { name: 'Project One', description: 'A short one-line description.', url: 'https://one.example.com' },
    { name: 'Project Two', description: 'A short one-line description.', url: 'https://two.example.com' },
    { name: 'Project Three', description: 'A short one-line description.', url: 'https://three.example.com' },
  ],
} as const;
