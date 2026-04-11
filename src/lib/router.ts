import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { RootLayout } from "@/components/layout/RootLayout"
import { MeetingDetailPage } from "@/pages/MeetingDetailPage"
import { MeetingsPage } from "@/pages/MeetingsPage"
import { SettingsPage } from "@/pages/SettingsPage"
import { UsagePage } from "@/pages/UsagePage"

const rootRoute = createRootRoute({ component: RootLayout })

const meetingsRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/",
	component: MeetingsPage,
})

const meetingDetailRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/meetings/$id",
	component: MeetingDetailPage,
})

const usageRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/usage",
	component: UsagePage,
})

const settingsRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/settings",
	component: SettingsPage,
})

const routeTree = rootRoute.addChildren([
	meetingsRoute,
	meetingDetailRoute,
	usageRoute,
	settingsRoute,
])

export const router = createRouter({ routeTree })

declare module "@tanstack/react-router" {
	interface Register {
		router: typeof router
	}
}
