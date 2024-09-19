export default {
  '*': {
    theme: {
      breadcrumb: false,
    },
  },
  blog: {
    type: 'page',
    title: 'Blog',
    theme: {
      layout: 'raw',
      typesetting: 'article',
      timestamp: false,
      breadcrumb: true,
      pagination: false,
    },
  },
  index: {
    title: 'Overview',
    display: 'hidden',
  },
  'getting-started': 'Getting Started',
  architecture: 'Architecture',
  legal: {
    title: 'Legal',
    display: 'hidden',
  },
}
