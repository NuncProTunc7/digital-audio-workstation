import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";
import { defaultBackend } from "./backend.ts";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App backend={defaultBackend()} />
    </StrictMode>,
  );
}
