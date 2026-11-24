-- Pane's design language, "Pane Amber": warm charcoal surfaces, hairline
-- borders, and an amber signature colour that the customizer can change.
-- Two registered themes (dark first, light supported) and a reactive
-- override (`scope`) that applies the accent and glow from Settings.

local M = {}

-- Signature colour presets (Customize look…).
M.presets = {
	{ id = "amber", label = "Amber", color = "#FFB020" },
	{ id = "violet", label = "Violet", color = "#9D7BFF" },
	{ id = "lime", label = "Lime", color = "#B4F000" },
	{ id = "ice", label = "Ice", color = "#5EC8FF" },
	{ id = "rose", label = "Rose", color = "#FF5D8F" },
	{ id = "graphite", label = "Graphite", color = "#C9C2B8" },
}
M.default_accent = "#FFB020"

-- Storage categories: id, label, colour, icon. Order is the meter's order.
M.categories = {
	{ id = "apps", label = "Apps", color = "#FFB020", icon = "apps" },
	{ id = "games", label = "Games", color = "#FF7A45", icon = "games" },
	{ id = "developer", label = "Developer", color = "#5EC8FF", icon = "code" },
	{ id = "photos", label = "Photos", color = "#C58CFF", icon = "image" },
	{ id = "videos", label = "Videos", color = "#FF5D8F", icon = "video" },
	{ id = "music", label = "Music", color = "#4FE0B0", icon = "music" },
	{ id = "documents", label = "Documents", color = "#7FA8FF", icon = "doc" },
	{ id = "caches", label = "Caches", color = "#E8D35A", icon = "broom" },
	{ id = "system", label = "System", color = "#8C8478", icon = "chip" },
	{ id = "recycle", label = "Recycle Bin", color = "#FF5D4D", icon = "trash" },
	{ id = "other", label = "Other", color = "#6A6359", icon = "dots" },
}
M.category_index = {}
for i, c in M.categories do
	M.category_index[c.id] = i
end

-- Colour helpers ----------------------------------------------------------

function M.parse(hex: string): (number, number, number)
	local h = string.gsub(hex, "#", "")
	return tonumber(string.sub(h, 1, 2), 16) or 0, tonumber(string.sub(h, 3, 4), 16) or 0, tonumber(string.sub(h, 5, 6), 16) or 0
end

function M.hex(r: number, g: number, b: number, a: number?): string
	local function c(v: number): number
		return math.clamp(math.floor(v + 0.5), 0, 255)
	end
	local s = string.format("#%02X%02X%02X", c(r), c(g), c(b))
	if a then
		s ..= string.format("%02X", c(a * 255))
	end
	return s
end

function M.alpha(hex: string, a: number): string
	local r, g, b = M.parse(hex)
	return M.hex(r, g, b, a)
end

function M.mix(a: string, b: string, t: number): string
	local r1, g1, b1 = M.parse(a)
	local r2, g2, b2 = M.parse(b)
	return M.hex(r1 + (r2 - r1) * t, g1 + (g2 - g1) * t, b1 + (b2 - b1) * t)
end

-- A colour from a hue (0-360), for the customizer's hue slider.
function M.from_hue(h: number): string
	local s, l = 1.0, 0.56
	local c = (1 - math.abs(2 * l - 1)) * s
	local x = c * (1 - math.abs((h / 60) % 2 - 1))
	local m = l - c / 2
	local r, g, b = 0, 0, 0
	if h < 60 then
		r, g, b = c, x, 0
	elseif h < 120 then
		r, g, b = x, c, 0
	elseif h < 180 then
		r, g, b = 0, c, x
	elseif h < 240 then
		r, g, b = 0, x, c
	elseif h < 300 then
		r, g, b = x, 0, c
	else
		r, g, b = c, 0, x
	end
	return M.hex((r + m) * 255, (g + m) * 255, (b + m) * 255)
end

-- Relative luminance, for picking text on the accent.
local function luminance(hex: string): number
	local r, g, b = M.parse(hex)
	return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255
end

-- Themes -----------------------------------------------------------------

-- Pane's own tokens, with defaults in standard terms, so any theme styles
-- them (and the design lint knows them).
ui.theme.tokens {
	app = {
		glow = "alpha({accent}, 0.3)",
		pip = "{accent}",
		led_off = "alpha({text.primary}, 0.07)",
		grid = "alpha({text.primary}, 0.035)",
		highlight = "alpha(#FFFFFF, 0.05)",
		panel = "{surface.base}",
		-- Behind the tab strip, toolbar and sidebar: clear in dark (the
		-- window's backdrop shows), a paper tint in light (the backdrop
		-- follows Windows' own mode, which may be dark).
		chrome = "#00000000",
	},
}

local common = {
	font = { mono = "Geist Mono" },
	radius = { sm = 6, md = 8, lg = 12, xl = 16 },
	app = {
		-- The dim dot of an unlit LED, and the hairline grid behind data.
		led_off = "alpha({text.primary}, 0.07)",
		grid = "alpha({text.primary}, 0.035)",
		-- A machined top edge on raised cards.
		highlight = "alpha(#FFFFFF, 0.05)",
		pip = "{accent}",
	},
}

local function merged(base: { [string]: any }, extra: { [string]: any }): { [string]: any }
	local out = table.clone(base)
	for k, v in extra do
		if type(v) == "table" and type(out[k]) == "table" then
			out[k] = merged(out[k], v)
		else
			out[k] = v
		end
	end
	return out
end

ui.theme.register {
	name = "pane-dark",
	type = "dark",
	extends = "zgx-dark",
	tokens = merged(common, {
		background = "#0F0E0C",
		surface = { sunken = "#0B0A08", base = "#15130F", raised = "#1D1A15", overlay = "#221E18" },
		text = { primary = "#F4EEE4", secondary = "#AFA597", tertiary = "#6F675C", disabled = "#4A443C" },
		border = { value = "#FFE4BE1F", subtle = "#FFE4BE14", strong = "#FFE4BE33" },
		accent = { value = M.default_accent, hover = "#FFC24D", text = "#1A1307" },
		danger = "#FF5D4D",
		success = "#7FD48A",
		warning = "#FFB020",
		hover = "#FFE4BE14",
		pressed = "#FFE4BE1F",
		selection = { value = "alpha({accent}, 0.30)", item = "alpha({accent}, 0.13)" },
		["focus.ring"] = "alpha({accent}, 0.65)",
		scrollbar = { thumb = "#FFE4BE26", ["thumb.active"] = "#FFE4BE40" },
		shadow = {
			sm = "0 1 2 0 #00000066",
			md = "0 6 18 0 #0000007A",
			lg = "0 18 48 0 #0000008C",
		},
		app = { glow = "alpha({accent}, 0.30)", panel = "#15130FF2" },
	}),
}

ui.theme.register {
	name = "pane-light",
	type = "light",
	extends = "zgx-light",
	tokens = merged(common, {
		background = "#F6F2EA",
		surface = { sunken = "#EFEAE0", base = "#FBF8F2", raised = "#FFFFFF", overlay = "#FFFFFF" },
		text = { primary = "#1F1B16", secondary = "#5E564B", tertiary = "#958B7E", disabled = "#BDB4A7" },
		border = { value = "#3A2A1221", subtle = "#3A2A1214", strong = "#3A2A1238" },
		accent = { value = "#D98A00", hover = "#C27A00", text = "#FFFFFF" },
		danger = "#D93A2B",
		success = "#2F9E4F",
		warning = "#D98A00",
		hover = "#3A2A1210",
		pressed = "#3A2A121A",
		selection = { value = "alpha({accent}, 0.25)", item = "alpha({accent}, 0.14)" },
		["focus.ring"] = "alpha({accent}, 0.6)",
		shadow = {
			sm = "0 1 2 0 #3A2A121A",
			md = "0 6 18 0 #3A2A1222",
			lg = "0 18 48 0 #3A2A122E",
		},
		app = { glow = "alpha({accent}, 0.18)", panel = "#FBF8F2F2", chrome = "#F1ECE2F0" },
	}),
}

-- Settings → overrides ---------------------------------------------------

-- The accent the settings ask for, made legible on the current theme.
function M.accent_for(setting: string?, light: boolean): string
	local accent = if setting and setting ~= "system" and string.sub(setting, 1, 1) == "#" then setting else M.default_accent
	if light then
		-- Light paper needs a deeper tone of the same hue.
		accent = M.mix(accent, "#000000", 0.18)
	end
	return accent
end

-- The token overrides for the window, following the settings (a function:
-- `theme = theme.scope(settings)` keeps the window's tokens current).
function M.scope(setting: (name: string) -> any): () -> { [string]: any }
	return function()
		local light = ui.theme.kind() == "light"
		local accent = M.accent_for(setting("accent"), light)
		local glow = (setting("glow") or 70) / 100
		local reduced = ui.prefers.reduced_transparency:get()
		local strength = (if light then 0.18 else 0.34) * glow * (if reduced then 0.4 else 1)
		local r = if setting("corners") == "sharp" then 0.5 else 1
		return {
			accent = accent,
			["accent.hover"] = M.mix(accent, "#FFFFFF", 0.18),
			["accent.text"] = if luminance(accent) > 0.55 then "#1A1307" else "#FFFFFF",
			["app.glow"] = M.alpha(accent, strength),
			["app.pip"] = accent,
			["selection.item"] = M.alpha(accent, if light then 0.14 else 0.13),
			["focus.ring"] = M.alpha(accent, 0.65),
			["radius.sm"] = 6 * r,
			["radius.md"] = 8 * r,
			["radius.lg"] = 12 * r,
			["radius.xl"] = 16 * r,
		}
	end
end

-- Applies dark / light / system from Settings.
function M.follow(setting: (name: string) -> any)
	ui.effect(function()
		local choice = setting("theme")
		if choice == "light" then
			ui.theme.set("pane-light")
		elseif choice == "dark" then
			ui.theme.set("pane-dark")
		else
			ui.theme.follow_system { dark = "pane-dark", light = "pane-light" }
		end
	end)
end

-- Motion: off when the system or Settings say so.
function M.motion(setting: (name: string) -> any): () -> boolean
	return function()
		local a = setting("animations")
		if a == "off" then
			return false
		elseif a == "on" then
			return true
		end
		return not ui.prefers.reduced_motion:get()
	end
end

return M
