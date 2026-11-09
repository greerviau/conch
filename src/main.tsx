import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { RecordingIndicator } from "./RecordingIndicator";
import "./style.css";
const overlay = new URLSearchParams(window.location.search).has("overlay");
if (overlay) document.body.classList.add("overlay-surface");
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {overlay ? <RecordingIndicator /> : <App />}
  </React.StrictMode>,
);
