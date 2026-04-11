import React from "react"
import ReactDOM from "react-dom/client"
import { Toaster } from "sonner"
import App from "./App"
import "./index.css"

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
	<React.StrictMode>
		<App />
		<Toaster
			position="bottom-right"
			theme="dark"
			toastOptions={{
				style: {
					background: "var(--color-surface-raised)",
					border: "1px solid var(--color-border)",
					color: "var(--color-text)",
					fontSize: "13px",
				},
			}}
		/>
	</React.StrictMode>,
)
