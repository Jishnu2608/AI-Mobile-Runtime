import { compileMDX } from "next-mdx-remote/rsc";
import remarkGfm from "remark-gfm";
import rehypeSlug from "rehype-slug";
import rehypePrettyCode from "rehype-pretty-code";
import { createCssVariablesTheme } from "shiki";
import type { ComponentProps, ReactNode } from "react";
import Link from "next/link";
import { CodeBlock } from "@/components/docs/CodeBlock";
import { Callout } from "@/components/docs/Callout";
import { Steps, Step } from "@/components/docs/Steps";
import { Status } from "@/components/ui/Status";
import { McpToolTable } from "@/components/docs/McpToolTable";
import { RequirementTable } from "@/components/docs/RequirementTable";
import { Issue } from "@/components/docs/Issue";
import { VirtualizationDiagram } from "@/components/docs/VirtualizationDiagram";
import { rewriteHref } from "@/lib/docs";

const theme = createCssVariablesTheme({ name: "css-variables", variablePrefix: "--shiki-", fontStyle: true });

function A({ href, children, ...rest }: ComponentProps<"a">) {
  const to = rewriteHref(href);
  if (to && to.startsWith("/")) {
    return (
      <Link href={to} {...rest}>
        {children}
      </Link>
    );
  }
  const external = to && /^https?:/.test(to);
  return (
    <a href={to} {...rest} {...(external ? { rel: "noreferrer" } : {})}>
      {children}
    </a>
  );
}

const components = {
  a: A,
  pre: (p: ComponentProps<"pre">) => <CodeBlock {...p} />,
  table: (p: ComponentProps<"table">) => (
    <div className="table-wrap" tabIndex={0} role="region" aria-label="Table">
      <table {...p} />
    </div>
  ),
  Callout,
  Steps,
  Step,
  Status,
  McpToolTable,
  RequirementTable,
  Issue,
  VirtualizationDiagram,
};

export async function renderMarkdown(source: string, format: "md" | "mdx"): Promise<ReactNode> {
  const { content } = await compileMDX({
    source,
    components,
    options: {
      // Site-authored .mdx passes array props to components (RequirementTable rows), so JS
      // expressions are allowed; dangerous ones (eval, imports, globals) stay blocked. Repository
      // documents render as plain markdown ("md"), which never evaluates expressions.
      blockJS: format !== "mdx",
      mdxOptions: {
        format,
        remarkPlugins: [remarkGfm],
        rehypePlugins: [
          rehypeSlug,
          [rehypePrettyCode, { theme, keepBackground: false, defaultLang: "text" }],
        ],
      },
    },
  });
  return content;
}
