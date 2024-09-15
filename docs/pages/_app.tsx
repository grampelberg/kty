import '../styles/globals.css'

import React, { useEffect } from 'react'
import { useRouter } from 'next/router'
import mermaid from 'mermaid'

import posthog from 'posthog-js'
import { PostHogProvider } from 'posthog-js/react'

if (typeof window !== 'undefined' && process.env.NEXT_PUBLIC_POSTHOG) {
  posthog.init(process.env.NEXT_PUBLIC_POSTHOG, {
    api_host: 'https://us.i.posthog.com',
    loaded: (posthog) => {
      if (process.env.NODE_ENV === 'development') posthog.debug()
    },
  })
}

mermaid.registerIconPacks([
  {
    name: 'logos',
    loader: () => import('@iconify-json/logos').then((module) => module.icons),
  },
  {
    name: 'line-md',
    loader: () =>
      import('@iconify-json/line-md').then((module) => module.icons),
  },
  {
    name: 'carbon',
    loader: () => import('@iconify-json/carbon').then((module) => module.icons),
  },
  {
    name: 'material-symbols',
    loader: () =>
      import('@iconify-json/material-symbols').then((module) => module.icons),
  },
  {
    name: 'mdi',
    loader: () => import('@iconify-json/mdi').then((module) => module.icons),
  },
])

export default function App({ Component, pageProps }) {
  const router = useRouter()

  useEffect(() => {
    // Track page views
    const handleRouteChange = () => posthog?.capture('$pageview')
    router.events.on('routeChangeComplete', handleRouteChange)

    return () => {
      router.events.off('routeChangeComplete', handleRouteChange)
    }
  }, [])

  return (
    <PostHogProvider client={posthog}>
      <Component {...pageProps} />
    </PostHogProvider>
  )
}
