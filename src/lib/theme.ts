export type ThemePref = "system" | "light" | "dark"

const KEY = "minutae-theme"

export function getThemePref(): ThemePref {
	try {
		const value = localStorage.getItem(KEY)
		return value === "light" || value === "dark" ? value : "system"
	} catch {
		return "system"
	}
}

export function setThemePref(pref: ThemePref) {
	try {
		localStorage.setItem(KEY, pref)
	} catch {
		// storage can be unavailable; the theme still applies for this session
	}
	applyTheme(pref)
}

export function applyTheme(pref: ThemePref) {
	const root = document.documentElement
	if (pref === "system") root.removeAttribute("data-theme")
	else root.setAttribute("data-theme", pref)
}
