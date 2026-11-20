import React from "react";
import ReactDOM from "react-dom/client";
import "@minutae/ui/styles.css";
import "@minutae/ui/fonts";
import { ThemeProvider, ToastProvider } from "@minutae/ui";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <ToastProvider>
        <App />
      </ToastProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
