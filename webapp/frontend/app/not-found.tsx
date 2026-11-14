import Link from "next/link";
import { ArrowRight } from "lucide-react";

export default function NotFound() {
  return (
    <div className="wrap grid min-h-[60dvh] content-center pb-24 pt-16">
      <p className="eyebrow">Error 404</p>
      <h1 className="t-page mt-5 max-w-2xl">This page does not exist</h1>
      <p className="t-lead mt-5 max-w-xl">The link may be old, or the page may have moved into the documentation.</p>
      <ul className="mt-10 flex flex-wrap gap-3">
        <li>
          <Link href="/" className="btn btn-primary h-11 px-5">
            Go to the home page
            <ArrowRight aria-hidden className="btn-icon size-4" />
          </Link>
        </li>
        <li>
          <Link href="/docs/" className="btn h-11 px-5">Open the documentation</Link>
        </li>
        <li>
          <Link href="/docs/troubleshooting/" className="btn btn-ghost h-11 px-5">See troubleshooting</Link>
        </li>
      </ul>
    </div>
  );
}
