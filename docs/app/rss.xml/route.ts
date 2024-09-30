import fs from 'node:fs/promises'
import path from 'node:path'
import { compileMdx } from 'nextra/compile'
import { toString } from 'hast-util-to-string'
import { type Root } from 'hast'
import { type Plugin } from 'unified'
import RSS from 'rss'

const SITE_URL = 'https://kty.dev'

const rehypeEnhanceFrontmatter: Plugin<[], Root> = () => (tree, file) => {
  const { frontMatter } = file.data as {
    frontMatter: Record<string, string | Date>
  }

  tree = {
    ...tree,
    children: tree.children.filter((node) => (node as any).tagName !== 'pre'),
  }

  const [filePath] = file.history

  frontMatter.description = toString(tree as any).trimStart()
  frontMatter.fileName = path.parse(filePath).name
  frontMatter.date = new Date(frontMatter.date)
}

export async function GET() {
  const files = await fs.readdir('./pages/blog')

  const blogs = await Promise.all(
    files
      .filter((filename) => /\.mdx?$/.test(filename))
      .map(async (filename) => {
        const filePath = path.join('./pages/blog', filename)
        const content = await fs.readFile(filePath, 'utf8')
        return await compileMdx(content, {
          filePath,
          mdxOptions: {
            rehypePlugins: [rehypeEnhanceFrontmatter],
          },
        })
      }),
  )

  blogs.sort((a, b) => b.frontMatter.date - a.frontMatter.date)

  const feed = new RSS({
    title: 'Blog',
    description: 'Blog',
    feed_url: `${SITE_URL}/rss.xml`,
    site_url: SITE_URL,
    language: 'en-US',
    pubDate: blogs[0].frontMatter.date.toUTCString(),
    ttl: 60,
  })

  for (const { frontMatter } of blogs) {
    feed.item({
      title: frontMatter.title,
      description: frontMatter.description.slice(0, 139) + '…',
      url: `${SITE_URL}/blog/${frontMatter.fileName}`,
      author: frontMatter.byline,
      date: frontMatter.date.toUTCString(),
    })
  }

  return new Response(feed.xml({ indent: true }), {
    headers: {
      'Content-Type': 'application/xml; charset=utf-8',
    },
  })
}
