/**
 * Application entry point.
 *
 * The window role comes from the query string the backend put on the URL, so
 * one document serves the main window, a compare window, and the About window.
 * Subscriptions are established before the first render of repository content,
 * so no event can arrive while the interface is still assembling.
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "./styles/global.css";
import { App } from "./app/App";

const container = document.getElementById("root");
if (!container) {
  throw new Error("missing #root container");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
