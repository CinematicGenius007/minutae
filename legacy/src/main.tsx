import React from "react"
import ReactDOM from "react-dom/client"
import { Toaster } from "sonner"
import App from "./App"
import "./index.css"
import { applyTheme, getThemePref } from "./lib/theme"

applyTheme(getThemePref())

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
	<React.StrictMode>
		<App />
		<Toaster
			position="bottom-right"
			theme="system"
			toastOptions={{
				style: {
					background: "var(--color-bg)",
					border: "1px solid var(--color-border)",
					color: "var(--color-text)",
					fontSize: "13px",
					boxShadow: "var(--shadow-pop)",
				},
			}}
		/>
	</React.StrictMode>,
)
