import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { AiProvider } from "./components/AiProvider";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AiProvider>
      <App />
    </AiProvider>
  </StrictMode>,
);
