-- Pane's icon set: small stroke icons drawn for Pane (24-unit grid, 1.75
-- stroke, round caps), tinted by the text colour around them. File and
-- folder icons stay the real shell icons; these are for the chrome.

local M = {}

local paths: { [string]: string } = {
	back = "M15 5l-7 7 7 7",
	forward = "M9 5l7 7-7 7",
	up = "M12 19V6M6 11l6-6 6 6",
	refresh = "M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v3.9h-3.9",
	plus = "M12 5v14M5 12h14",
	close = "M6.5 6.5l11 11M17.5 6.5l-11 11",
	more = "M6 12h.01M12 12h.01M18 12h.01",
	details = "M4 6h16M4 12h16M4 18h16M8.5 4v16",
	list = "M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01",
	grid = "M4.5 4.5h6v6h-6zM13.5 4.5h6v6h-6zM4.5 13.5h6v6h-6zM13.5 13.5h6v6h-6z",
	sort = "M7 5v14M4 16l3 3 3-3M14 7h6M14 12h4.5M14 17h3",
	search = "M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5L20 20",
	pin = "M9 4h6l-1 5 3 3H7l3-3-1-5zM12 15v5",
	home = "M4 11l8-6.5 8 6.5M6.5 9.5V19h11V9.5",
	overview = "M4 19V12M9.3 19V7M14.6 19v-9M20 19V5",
	drive = "M4 14h16v5H4zM4 14l2.5-8h11L20 14M16.5 16.5h.01",
	computer = "M4 5h16v11H4zM9 20h6M12 16v4",
	folder = "M3.5 7.5V18a1 1 0 0 0 1 1h15a1 1 0 0 0 1-1V9.5a1 1 0 0 0-1-1h-7l-2-2.5h-6a1 1 0 0 0-1 1z",
	desktop = "M4 5h16v10H4zM8 19h8M12 15v4",
	download = "M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14",
	doc = "M7 3.5h7l4 4V20H7zM14 3.5V8h4M9.5 12.5h5M9.5 16h5",
	image = "M4.5 5h15v14h-15zM4.5 16l4.5-4.5 4 4 2.5-2.5 4 4M15 9.5h.01",
	video = "M4 6.5h11v11H4zM15 10.5l5-3v9l-5-3",
	music = "M9 18V6l10-2v12M9 18a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0zM19 16a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z",
	apps = "M5 5h5.5v5.5H5zM13.5 5H19v5.5h-5.5zM5 13.5h5.5V19H5zM16.25 13.5v5.5M13.5 16.25H19",
	games = "M7 9h10a4 4 0 0 1 3.9 4.9l-.6 2.6a2 2 0 0 1-3.4.9L15 15.5H9l-1.9 1.9a2 2 0 0 1-3.4-.9l-.6-2.6A4 4 0 0 1 7 9zM8 11.5v3M6.5 13h3M15.5 12.5h.01M17 14h.01",
	code = "M8.5 7L4 12l4.5 5M15.5 7l4.5 5-4.5 5M13.5 5l-3 14",
	broom = "M14.5 4l-4 8M6 13h9l1.5 7h-12zM9 16.5v3.5M12 16.5v3.5",
	chip = "M7 7h10v10H7zM10 3.5V7M14 3.5V7M10 17v3.5M14 17v3.5M3.5 10H7M3.5 14H7M17 10h3.5M17 14h3.5",
	trash = "M5 7h14M9.5 7V4.5h5V7M6.5 7l1 12.5h9l1-12.5M10.5 11v5M13.5 11v5",
	dots = "M6 12h.01M12 12h.01M18 12h.01",
	sparkle = "M12 4l1.6 4.8L18.5 10.5l-4.9 1.7L12 17l-1.6-4.8L5.5 10.5l4.9-1.7zM18.5 16.5l.6 1.8 1.9.6-1.9.7-.6 1.8-.6-1.8-1.9-.7 1.9-.6z",
	palette = "M12 4a8 8 0 1 0 0 16c1.2 0 1.8-.8 1.8-1.7 0-1.2-1-1.6-1-2.6 0-.9.7-1.6 1.6-1.6H17a3 3 0 0 0 3-3C20 7.3 16.4 4 12 4zM8 11h.01M10.5 7.8h.01M14.5 7.8h.01",
	bolt = "M13 3.5L5.5 13.5h6L11 20.5l7.5-10h-6z",
	shield = "M12 3.5l7 3v5.5c0 4-3 7.2-7 8.5-4-1.3-7-4.5-7-8.5V6.5z",
	check = "M5 12.5l4.5 4.5L19 7.5",
	chevron = "M9 6l6 6-6 6",
	external = "M14 4h6v6M20 4l-8 8M18 14v5H5V6h5",
}

M.names = paths

-- An icon node: `size` points, tinted `color` (default: the text colour).
function M.icon(name: string, size: number?, color: any?): Node
	local d = paths[name] or paths.dots
	local s = size or 16
	return ui.svg {
		code = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='1.75' stroke-linecap='round' stroke-linejoin='round'><path d='{d}'/></svg>`,
		w = s,
		h = s,
		shrink = 0,
		tint = color or "currentColor",
		aria_hidden = true,
	}
end

return M
