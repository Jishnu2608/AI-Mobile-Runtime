import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { Preview } from "./Preview";
import "./styles.css";

// `?view=preview` = the docked preview window (`tumr-desktop --preview`); the VS Code extension's
// panel sets `window.__TUMR_VIEW__`; otherwise the wizard
const preview =
  new URLSearchParams(window.location.search).get("view") === "preview" ||
  (window as unknown as { __TUMR_VIEW__?: string }).__TUMR_VIEW__ === "preview";

createRoot(document.getElementById("root")!).render(
  <StrictMode>{preview ? <Preview /> : <App />}</StrictMode>,
);
