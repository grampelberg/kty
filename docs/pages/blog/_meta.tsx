import { useConfig } from 'nextra-theme-docs'
import NextLink from 'next/link'

export default {
  '*': {
    display: 'hidden',
    theme: {
      sidebar: false,
      timestamp: true,
      layout: 'default',
      topContent: function TopContent() {
        const { frontMatter } = useConfig()
        const { title, byline } = frontMatter
        const date = new Date(frontMatter.date)

        return (
          <>
            <h1 className="text-balance">{title}</h1>
            <div className="text-gray-500 text-center">
              <time dateTime={date.toISOString()}>
                {date.toLocaleDateString('en', {
                  month: 'long',
                  day: 'numeric',
                  year: 'numeric',
                })}
              </time>{' '}
              by {byline}
            </div>
          </>
        )
      },
    },
  },
}
