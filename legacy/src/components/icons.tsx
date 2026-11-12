import type { SVGProps } from "react"

type IconProps = SVGProps<SVGSVGElement> & { size?: number }

function Icon({ size = 16, children, ...props }: IconProps) {
	return (
		<svg
			aria-hidden="true"
			width={size}
			height={size}
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth={1.75}
			strokeLinecap="round"
			strokeLinejoin="round"
			{...props}
		>
			{children}
		</svg>
	)
}

export const MeetingsIcon = (p: IconProps) => (
	<Icon {...p}>
		<rect x="3.5" y="4.5" width="17" height="15" rx="3" />
		<path d="M8 9.5h8M8 14h5" />
	</Icon>
)
export const MicIcon = (p: IconProps) => (
	<Icon {...p}>
		<rect x="9" y="3" width="6" height="11" rx="3" />
		<path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3" />
	</Icon>
)
export const ChartIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="M4 20V10M10 20V4M16 20v-7M21 20H3" />
	</Icon>
)
export const SettingsIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
		<circle cx="15" cy="7" r="2" />
		<circle cx="9" cy="17" r="2" />
	</Icon>
)
export const SearchIcon = (p: IconProps) => (
	<Icon {...p}>
		<circle cx="11" cy="11" r="6.5" />
		<path d="m20 20-4-4" />
	</Icon>
)
export const CheckIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="m5 12.5 4.5 4.5L19 7.5" />
	</Icon>
)
export const CopyIcon = (p: IconProps) => (
	<Icon {...p}>
		<rect x="8.5" y="8.5" width="11" height="11" rx="2.5" />
		<path d="M15.5 8.5v-2a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
	</Icon>
)
export const ArrowLeftIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="M19 12H5m6-6-6 6 6 6" />
	</Icon>
)
export const PauseIcon = (p: IconProps) => (
	<Icon {...p} fill="currentColor" stroke="none">
		<rect x="6" y="5" width="4" height="14" rx="1.2" />
		<rect x="14" y="5" width="4" height="14" rx="1.2" />
	</Icon>
)
export const PlayIcon = (p: IconProps) => (
	<Icon {...p} fill="currentColor" stroke="none">
		<path d="M7 5.2v13.6a1 1 0 0 0 1.5.86l11-6.8a1 1 0 0 0 0-1.72l-11-6.8A1 1 0 0 0 7 5.2Z" />
	</Icon>
)
export const StopIcon = (p: IconProps) => (
	<Icon {...p} fill="currentColor" stroke="none">
		<rect x="6" y="6" width="12" height="12" rx="2.5" />
	</Icon>
)
export const TrashIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 12.5h9l1-12.5" />
	</Icon>
)
export const RefreshIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4h-4" />
	</Icon>
)
export const SparkIcon = (p: IconProps) => (
	<Icon {...p}>
		<path d="M12 3.5v4M12 16.5v4M3.5 12h4M16.5 12h4M6 6l2.5 2.5M15.5 15.5 18 18M18 6l-2.5 2.5M8.5 15.5 6 18" />
	</Icon>
)
