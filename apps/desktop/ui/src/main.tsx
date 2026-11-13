import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { Preview } from "./Preview";
import "./styles.css";

// `?view=preview` = the docked preview window (`umr-desktop --preview`); otherwise the wizard
const preview = new URLSearchParams(window.location.search).get("view") === "preview";

createRoot(document.getElementById("root")!).render(
  <StrictMode>{preview ? <Preview /> : <App />}</StrictMode>,
);
