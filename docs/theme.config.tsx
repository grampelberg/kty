import React from 'react'
import { DocsThemeConfig, useConfig } from 'nextra-theme-docs'
import Image from 'next/image'
import { useTheme } from 'next-themes'
import { usePathname } from 'next/navigation'
import path from 'path'

const config: DocsThemeConfig = {
  logo: () => {
    const { resolvedTheme } = useTheme()

    return (
      <Image
        src={resolvedTheme == 'dark' ? '/logo-dark.gif' : '/logo-light.gif'}
        alt="kty"
        width={64}
        height={36}
      />
    )
  },
  project: {
    link: 'https://github.com/grampelberg/kty',
  },
  chat: {
    link: 'https://discord.gg/xMuN5csrQs',
  },
  docsRepositoryBase: 'https://github.com/grampelberg/kty',
  footer: {
    content: (
      <span>
        {new Date().getFullYear()} ©{' '}
        <a href="https://kty.dev" target="_blank">
          kty
        </a>
      </span>
    ),
  },
  head: () => {
    const config = useConfig()
    const pathname = usePathname()
    const title = `${config.title} – kty`
    const description =
      config.frontMatter.description || 'kty: Terminal for Kubernetes'

    const image = config.frontMatter.image || '/logo-dark.gif'

    return (
      <>
        <title>{title}</title>
        <meta property="og:title" content={title} />
        <meta name="description" content={description} />
        <meta property="og:description" content={description} />
        <meta name="og:image" content={image} />
        <meta name="og:image:alt" content={title} />
        <meta property="og:locale" content="en_us" />
        <meta property="og:url" content={`https://kty.dev${pathname}`} />
        <meta property="og:type" content="website" />
        <meta property="og:logo" content="/logo-dark.gif" />

        <meta name="msapplication-TileColor" content="#fff" />
        <meta httpEquiv="Content-Language" content="en" />
        <meta name="apple-mobile-web-app-title" content="kty" />
        <meta name="msapplication-TileImage" content={image} />

        <meta name="twitter:card" content="summary_large_image" />
        <meta name="twitter:site" content="https://kty.dev" />
        <link
          rel="apple-touch-icon"
          sizes="180x180"
          href="/logo-dark-500x500.png"
        />
        <link
          rel="icon"
          type="image/png"
          sizes="192x192"
          href="/logo-dark-500x500.png"
        />
        {/* <link
          rel="icon"
          type="image/png"
          sizes="32x32"
          href="/favicon-32x32.png"
        /> */}
        <link rel="icon" type="image/png" sizes="96x96" href="/ico-dark.png" />
        {/* <link
          rel="icon"
          type="image/png"
          sizes="16x16"
          href="/favicon-16x16.png"
        /> */}
      </>
    )
  },
}

export default config
