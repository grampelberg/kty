import Link from 'next/link'
import { getPagesUnderRoute } from 'nextra/context'
import { useRouter } from 'next/router'
import clsx from 'clsx'
import { FrontMatter, MdxFile } from 'nextra'

const Blog = () => {
  const { asPath } = useRouter()

  let linkClasses = [
    'border',
    'border-zinc-200',
    'dark:border-[#414141]',
    'p-8',
    'lg:p-12',
    'bg-white',
    'dark:bg-neutral-800',
    'rounded-none',
    'hover:!border-primary',
    'hover:dark:bg-neutral-700/50',
    'hover:border-violet-300',
    'hover:shadow-2xl',
    'hover:shadow-primary/10',
    'dark:shadow-none',
    'transition-colors',
    'flex',
    'flex-col',
  ]

  const items = getPagesUnderRoute('/blog').map(
    ({ route, frontMatter }: MdxFile) => {
      const { title, byline, date } = frontMatter as FrontMatter

      return (
        <Link href={route} className={clsx(linkClasses)}>
          <div className="font-extrabold text-xl md:text-3xl text-balance">
            {title}
          </div>
          <div className="opacity-50 text-sm my-7 flex gap-2">
            <time dateTime={date.toISOString()}>
              {date.toLocaleDateString('en', {
                month: 'long',
                day: 'numeric',
                year: 'numeric',
              })}
            </time>
            <span className="border-r border-gray-500" />
            <span>by {byline}</span>
          </div>
          <span className="text-primary block font-bold mt-auto">
            Read more →
          </span>
        </Link>
      )
    },
  )

  return (
    <div className="container grid md:grid-cols-2 gap-7 pb-10 pt-10">
      {items}
    </div>
  )
}

export default Blog
