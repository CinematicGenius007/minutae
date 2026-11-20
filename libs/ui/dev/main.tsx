import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../src/styles/index.css";
import "../src/fonts";
import { Gallery } from "../src/gallery/Gallery";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Gallery />
    </StrictMode>,
  );
}
