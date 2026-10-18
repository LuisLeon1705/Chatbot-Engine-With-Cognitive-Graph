import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { loadBundle, setBundle } from "./i18n";
import "./styles.css";

// The dictionary arrives before the first render, so no text shows up untranslated.
loadBundle()
  .then(setBundle)
  .catch((e) => console.error("dictionary", e))
  .finally(() =>
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        <App />
      </StrictMode>,
    ),
  );
