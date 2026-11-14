/**
 * Runs in <head> before the first paint: applies the saved theme, or the system's, so the page
 * never flashes the wrong theme. Without script the CSS media query does the same.
 */
const script = `(function(){try{var s=localStorage.getItem("theme");var t=(s==="light"||s==="dark")?s:(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light");document.documentElement.setAttribute("data-theme",t);}catch(e){}})();`;

export function ThemeScript() {
  return <script dangerouslySetInnerHTML={{ __html: script }} />;
}
